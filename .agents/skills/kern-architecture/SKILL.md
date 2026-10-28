---
name: kern-architecture
description: Deep architectural reference, storage geometries (ORDS-01), zero-cost driver discovery, dynamic RF arbitration, and firmware toolchain conventions for the Kern amateur radio microkernel.
---

# Kern Architecture & Design Reference

This skill provides comprehensive architectural guidelines, invariant specifications, and implementation patterns for the Kern microkernel and firmware ecosystem.

## When to Consult this Skill
- Modifying or extending storage table formats, filesystem drivers, or configuration layouts.
- Developing or registering new hardware drivers (e.g. Si4732 HF, GNSS/GPS, BLE NUS/SPP, Color ST7735/ST7789 displays).
- Adding or refactoring microkernel RF service contracts, modulation profiles, or tone synthesizers.
- Interacting with or extending the hardened `./kern` build toolchain.

---

## 1. Storage & Database Architecture (ORDS-01)

### Power-of-Two Geometries ($2^N$ Bytes)
The microkernel storage layer enforces exact power-of-two record sizes to ensure that offset arithmetic on Cortex-M0+ compiles to a single logical left shift (`<<`) without integer division or remainder instructions:

$$\text{record\_offset}(i) = 16 + (i \ll N)$$

| Record Type | Size | Exponent ($N$) | Sector Density (4 KB Sector) | File Identifier (FourCC) |
| :--- | :--- | :--- | :--- | :--- |
| **Header** | 16 B | $N=4$ | First 16 bytes of file | `CHAN`, `CONT`, etc. |
| **FM Station** | 16 B | $N=4$ | 255 records / sector | `FMBC` (`/user/fm_radio.fm`) |
| **Contact** | 32 B | $N=5$ | 127 records / sector | `CONT` (`/user/contacts.ct`) |
| **Band Plan** | 32 B | $N=5$ | 127 records / sector | `BNDP` (`/user/bandplans.bnd`) |
| **Zone** | 32 B | $N=5$ | 127 records / sector | `ZONE` (`/user/zones.zn`) |
| **Scanner Hit**| 32 B | $N=5$ | 127 records / sector | `HITS` (`/user/scanner.hit`) |
| **Channel** | 64 B | $N=6$ | 63 records / sector | `CHAN` (`/user/channels.ch`) |
| **VFO Snapshot**| 160 B | Composite | 25 files / sector | `VFO ` (`/user/vfo_a.vfo`) |
| **Calibration**| 512 B | $N=9$ | 7 files / sector | `CAL1` (`/sys/calib.dat`) |

### LittleFS Alignment & Crash Safety
- Records never straddle 4 KB sector boundaries.
- Flat file architecture directly on LittleFS provides atomic, journaling updates without container unpacking overhead or RAM pressure.
- Strictly **no archive containers** (`.cplg`, `.tar`, `.zip`, `.qsh`).

### Two-Tier VFO Resilience
- **Tier 1 (Volatile SRAM)**: Rotary dial tuning and scanning update internal `RadioState` in SRAM. Zero flash wear during active user interaction.
- **Tier 2 (Non-Volatile Flash)**: On idle timeout (> 5 s) or graceful shutdown, the composite state is written to `/user/vfo_a.vfo` and `/user/vfo_b.vfo` (32-byte header + 2x 64-byte channel records = 160 bytes). Cold boot restores the full radio state in under 1.8 ms.

### Opaque Application Extensions
To allow userland apps to attach private metadata to core records without altering table schemas, core records include a 16-byte extension slot:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct ChannelExtension {
    pub tag: [u8; 4],      // Application FourCC identifier (e.g. b"APRS", b"SCAN")
    pub payload: [u8; 12],  // Opaque application-managed payload
}
```

---

## 2. Hardware Abstraction & Driver Discovery

### Zero-Cost Driver Trait
Peripherals implement the static `Driver` contract:
```rust
pub trait Driver {
    fn descriptor(&self) -> PeripheralDescriptor;
    fn bus(&self) -> BusType;
    fn init(&mut self) -> Result<(), ()>;
}
```

### Typed Bus Descriptors
Monolithic bitmasks (`expansion_chips: u32`) are replaced with typed, zero-alloc bus descriptors:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    Internal,
    I2c { address: u8 },
    Spi { cs_pin: u8 },
    Uart { port: u8 },
    Usb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeripheralDescriptor {
    pub driver_class: DeviceType,
    pub driver_id: u16,
    pub bus_type: BusType,
    pub capabilities: CapabilitySet,
}
```

### Static Dispatch & Zero Flash Overhead
- Peripherals are bound statically in the Board Support Package (`bsp::Board`).
- `#[cfg(feature = "...")]` compilation removes unused drivers completely at compile time.
- Applications query capability tokens via the $O(1)$ `CapsCheck` trait (`ctx.has_cap(Cap::BroadBandRx)`) rather than querying dynamic trait objects (`dyn Driver`).

---

## 3. Dynamic RF Arbitration & Modulation Profiles

### Decoupled Subsystem Facets
The microkernel divides radio capabilities into granular, decoupled traits:
- `RfChannel`: Frequency tuning, channel bandwidth, step size.
- `RfModulation`: Dynamic modulation switching (FM, AM, USB, LSB, CW, FSK).
- `RfSquelch`: Squelch threshold and carrier detect.
- `RfSubaudible`: CTCSS tone encode/decode, DCS digital code squelch.
- `RfFskPhy`: Physical FSK register access, preamble alignment, and FIFO management.
- `RfToneGenerator`: Dual-tone frequency synthesizers, DTMF, Morse/CW, Rogers.
- `RadioService`: Full composite trait unifying all radio operations.

### Dynamic Modulation Profiles
Applications request operational modes via atomic `ModulationProfile` configurations arbitrated by the microkernel `RadioCoordinator`:
```rust
pub struct ModulationProfile {
    pub modulation: ModulationType,
    pub rf_bandwidth_hz: u32,
    pub af_highpass_hz: u16,
    pub af_lowpass_hz: u16,
    pub agc_speed: AgcSpeed,
    pub squelch_level: u8,
    pub audio_route: AudioRoute,
}
```
This isolates application logic from hardware-specific register timing and gain curves.

---

## 4. Build Toolchain Standards

The unified `./kern` script drives all development operations:

```bash
./kern check     # Fast type & syntax check (0 warnings required)
./kern build     # Compile optimized release binary
./kern size      # Display section memory usage (.text, .rodata, .data, .bss)
./kern fmt       # Auto-format codebase with rustfmt
./kern disasm    # Disassemble Cortex-M0+ release instructions
./kern flash     # Flash target radio via serial/USB bootloader
./kern clean     # Remove build artifacts
```

### Resource Budgets (PY32F071xB)
- **Flash ROM**: 128 KB (120,832 bytes usable firmware budget; target < 80 KB used).
- **SRAM**: 16 KB (16,384 bytes budget; target < 5 KB static used).

---

## 5. Operational Directives & Invariants

- **Master Plan Fidelity**: Maintain all architecture specifications, Cortex-M0+ silicon guidelines, and memory maps without truncation or loss of detail.
- **State & Transition Testing**: App exit, navigation switching, and view restoration logic must be backed by unit tests verifying zero state degradation.
- **Sub-30ms UI Latency**: Audio tone warm-up and filesystem operations must never stall the UI loop; package and storage metadata must be cached in memory during navigation.
- **Crate Hygiene**: Workspace crates must remain strictly deduplicated and up-to-date with Rust 2024 idioms.
- **Subsystem Isolation**: Avoid modifying files actively owned by concurrent agents (e.g. boot splash files).
