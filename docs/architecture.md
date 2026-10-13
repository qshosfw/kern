# kern — Architecture Specification

`kern` is a zero-heap, zero-cost-abstraction, memory-safe microkernel written in `no_std` Rust for the **Quansheng UV-K1 / UV-K5 v3** radios powered by the **PY32F071** ARM Cortex-M0+ microcontroller.

---

## 1. System Constraints & Hard Limits

| Hardware Dimension | Hard Boundary | Engineering Strategy |
|---|---|---|
| **CPU Core** | 32-bit ARM Cortex-M0+ @ 8MHz – 72MHz | DVFS dynamic frequency scaling; WFI sleep on idle |
| **Flash Memory** | 128 KB Total (`0x08000000` - `0x08020000`) | 10 KB reserved for DFU Bootloader; **118 KB usable application budget** |
| **SRAM** | 16 KB Total (`0x20000000` - `0x20004000`) | Static arena allocation; zero heap (`alloc` disallowed); `.ramfunc` for hot paths |
| **Storage** | 2 MB SPI Flash (PY25Q16) | LittleFS filesystem with wear leveling & `lfs_attr` metadata tags |
| **Display** | ST7565 128×64 Monochrome LCD | SPI + DMA page-buffered update pipeline |
| **Transceiver** | Beken BK4819 | SPI-controlled RF core, internal audio codec, FSK modem |
| **HF Receiver** | Si4732 / Si4735 (Hardware Mod) | Optional I2C slave for SW/MW/LW SSB reception |

---

## 2. The 5-Layer Workspace Topology

To guarantee non-circular dependencies, zero component coupling, and strict architectural isolation, `kern` enforces a rigid 5-layer system hierarchy:

```
┌────────────────────────────────────────────────────────────────────────┐
│                      Layer 4: Applications (app)                        │
│         Dual VFO  │  Spectrum Analyzer  │  Channel Scanner  │  Settings │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Depends on L3
┌───────────────────────────────────▼────────────────────────────────────┐
│                    Layer 3: System Services & Framework                │
│    Radio Engine (radio) │ Graphics (gfx) │ Filesystem (fs) │ Proto/KWP   │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Depends on L2
┌───────────────────────────────────▼────────────────────────────────────┐
│                    Layer 2: Device Drivers (dev)                       │
│    BK4819 RF  │  ST7565 LCD  │  PY25Q16 Flash  │  Keyboard Matrix       │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Depends on L0.5 & L1
┌───────────────────────────────────▼────────────────────────────────────┐
│                  Layer 1: Core Microkernel (kern)                      │
│   Event Loop │ Task Scheduler │ Static Arena │ Stack Monitor │ Flight  │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Depends on L0 & L0.5
┌───────────────────────────────────▼────────────────────────────────────┐
│             Layer 0.5: Platform Abstraction Traits (platform)          │
│   DisplayBackend │ RadioBackend │ StorageBackend │ InputBackend │ Serial │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Implemented by L0 (HW) & sim (SDL2)
┌───────────────────────────────────┴────────────────────────────────────┐
│                Layer 0: Peripheral Hardware (arch)                     │
│    PY32F071 Minimal PAC │ RCC Clocks │ GPIO Typestates │ SPI/DMA │ UART  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Core Architectural Principles

### 3.1. Zero-Heap Static Arena Allocation

Dynamic heap allocation (`malloc` / `free` or Rust's `alloc::alloc`) is strictly prohibited. All system state, framebuffers, task blocks, and IO buffers are allocated at compile time using static memory or managed via a **bump-allocator arena** initialized during boot:

```rust
pub struct KernelArena<const SIZE: usize> {
    buffer: [u8; SIZE],
    offset: AtomicUsize,
}

impl<const SIZE: usize> KernelArena<SIZE> {
    pub const fn new() -> Self {
        Self {
            buffer: [0u8; SIZE],
            offset: AtomicUsize::new(0),
        }
    }

    pub fn alloc_slice<T: Copy>(&self, count: usize) -> Option<&'static mut [T]> {
        let bytes_needed = count * core::mem::size_of::<T>();
        let current = self.offset.fetch_add(bytes_needed, Ordering::SeqCst);
        if current + bytes_needed <= SIZE {
            let ptr = (self.buffer.as_ptr() as usize + current) as *mut T;
            unsafe { Some(core::slice::from_raw_parts_mut(ptr, count)) }
        } else {
            None // OOM protection at boot
        }
    }
}
```

### 3.2. Hardware Peripheral Ownership (Compile-Time Singletons)

Peripherals follow Rust's strict move semantics. Each physical hardware peripheral can only be claimed once at system startup. Attempts to instantiate multiple driver handles to the same hardware fail at compile time:

```rust
pub struct Peripherals {
    pub gpioa: Option<GPIOA>,
    pub gpiob: Option<GPIOB>,
    pub spi1:  Option<SPI1>,
    pub usart1: Option<USART1>,
    pub dma1:   Option<DMA1>,
}

impl Peripherals {
    pub fn take() -> Option<Self> {
        // Enforces atomic singleton access to raw PAC registers
    }
}
```

### 3.3. Dual Target Abstraction (Hardware vs Desktop Simulator)

By routing all device access through `platform/` traits, the application layer, graphics engine, and radio logic compile unchanged for two completely different target targets:

1. **`firmware/` (Target: `thumbv6m-none-eabi`)**: Executes directly on PY32F071 bare metal with zero OS dependencies.
2. **`sim/` (Target: `x86_64-unknown-linux-gnu`)**: Executes natively on Linux using SDL2 for LCD rendering, keyboard input, synthetic RF RSSI generation, and file-backed SPI Flash simulation.

---

## 4. Execution Pipeline & Real-Time Event Dispatch

```
                    ┌─────────────────────────┐
                    │   Hardware Interrupt    │
                    │   (GPIO/Timer/DMA/UART) │
                    └────────────┬────────────┘
                                 │ Writes event bit mask
                                 ▼
                    ┌─────────────────────────┐
                    │ Event Ring Buffer (SRAM)│
                    │  (Atomic lock-free queue│
                    └────────────┬────────────┘
                                 │ Scanned by kernel
                                 ▼
                    ┌─────────────────────────┐
                    │ Cooperative Scheduler   │
                    │   (Dispatches to App)   │
                    └────────────┬────────────┘
                                 │
                 ┌───────────────┴───────────────┐
                 ▼                               ▼
     ┌──────────────────────┐        ┌──────────────────────┐
     │  Render Dirty LCD    │        │ Run Power State Machine│
     │  Pages via SPI DMA   │        │ (8MHz WFI Sleep Mode)│
     └──────────────────────┘        └──────────────────────┘
```

---

## 5. Performance Engineering & SRAM Function Execution

To bypass flash access latency (0-wait-state @ 24MHz, but requiring wait states at 72MHz), critical hot-path loops are placed directly into SRAM using custom linker sections (`.ramfunc`):

- **RF Register Polling & RSSI Sweeps**: Fast spectrum scanning runs directly out of SRAM.
- **SPI DMA Page Transfers**: LCD page update routines reside in SRAM to maintain maximum SPI clock speeds (up to 18MHz SPI master clock).
- **Raw PHY FIFO Polling & Bitstream Processing**: Hardware modem FIFO reads and PHY signal processing run in SRAM.

```rust
#[link_section = ".ramfunc"]
pub fn bk4819_fast_rssi_sweep(results: &mut [u16; 128]) {
    // Executes from SRAM with 0 wait-states @ 72MHz
}
```

---

## 6. Modular Physical Layer (PHY) Driver Abstraction

`kern` decouples high-level protocol implementations (like AX.25, POCSAG, or custom telemetry) from the base firmware kernel image. 

The microkernel exposes modular **Physical Layer (PHY) driver traits** (`FskPhy`, `SubAudiblePhy`, `ScramblerPhy`, `DtmfPhy`):

- **FskPhy**: Provides raw hardware access to the BK4819 FSK/FFSK transmit/receive FIFO without protocol lock-in.
- **SubAudiblePhy**: Controls sub-audible CTCSS tones and 23-bit DCS codewords, allowing apps to manipulate sub-audible signaling dynamically.
- **ScramblerPhy**: Controls frequency-inversion voice scrambling registers, enabling dynamic CTCSS-synced frequency hopping scramblers.
- **DtmfPhy**: Dual-tone generation and decoding for selective calling and telemetry beacons.

Higher-level protocol stacks can be implemented as standalone application modules, services, or host co-processor applications.

