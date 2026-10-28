# KERN Realtime eXecutable (`.krx`) Architecture & Dynamic Execution Subsystem Specification

---

## 1. Architectural Vision & Scope

The KERN microkernel operating system is transitioning from a static, compile-time linked monolithic application model (declared via `kernel::define_os!` into fixed `MaybeUninit` slots) toward a **fully dynamic, modular, filesystem-indexed loadable application subsystem**.

### Core Goals
- **Unified Self-Contained Packages (`.krx`)**: Applications exist as standalone `.krx` (KERN Realtime eXecutable) files stored anywhere in the SPI NOR flash filesystem (KernFS under `/apps/`, `/user/apps/`, or custom directories). No complex unpacking, installation scripts, or multi-file directories are needed.
- **Dynamic Indexing & Discovery**: Zero-allocation directory scanner rapidly reads headers and manifests in a single 128-byte SPI flash burst, dynamically populating the launcher grid with icons, titles, and metadata in under 1 ms per app.
- **Unified Capability Tokens (Not Security Limits)**: In accordance with pure microkernel architecture (e.g. `src/kern/src/caps.rs`), capabilities are not arbitrary policy limits or permission prompts. A **Capability IS a typed authorization token** granting access to a hardware peripheral or kernel service. Requesting a capability token binds the corresponding interface VTable into the application's runtime.
- **Execution Agnostic**: A unified package container hosting four single-word execution classes: **`Native`**, **`Paged`**, **`VMachine`**, and **`Declare`**.
- **Infinite Native Scaling Without Internal Flash Wear**: Solves the Cortex-M0+ 16 KB SRAM barrier through **Demand-Paged Software Overlays with LR-Thunking and PLT Interception**, streaming code from SPI flash without touching internal MCU flash.
- **Zero-Overhead Late-Bound APIs**: Applications link directly against kernel service function pointers via Global Offset Table (GOT) patching at load time, achieving identical execution latency to monolithic firmware builds with zero syscall context-switch overhead.
- **Position-Independent Relocatable Code**: Replaces brittle pinned VMAs with **Single-PIC Base Relocations via Differential Deltas**, allowing native applications to execute anywhere in transient SRAM without rebuilding for specific firmware versions.
- **Zero-RAM Hardware DMA Asset Streaming**: Bitmaps and audio streams are indexed directly in the package header, allowing the OS to stream assets via SPI DMA straight to the display controller or audio DAC without consuming application execution SRAM.
- **True Background Multitasking**: Preserves 64-byte Cortex-M0+ hardware register contexts and compact stacks, enabling concurrent resident applications managed via a hardware-accelerated Symbian-style Task Manager.
- **Platform-Wide Adoption**: Engineered as an open, vendor-neutral standard (`krx-abi`) adoptable by any firmware built on the KERN microkernel or compatible embedded transceivers.

---

## 2. Hardware Constraints & Execution Realities (PY32F071)

| Silicon Feature | Reality & Impact on `.krx` Architecture |
| :--- | :--- |
| **CPU Core** | **ARM Cortex-M0+ (ARMv6-M)** @ 48 MHz (or 72 MHz overclock). 2-stage in-order pipeline, Thumb-1 (16-bit) instruction set with minimal 32-bit extensions (`BL`, `MRS`, `MSR`, `DSB`, `ISB`). Hardware 32×32 multiplier (1 cycle). **No hardware division** (`UDIV`/`SDIV` absent). Flat memory model lacking a hardware Memory Management Unit (MMU). **Strict 4-byte alignment required** (unaligned 32-bit access generates HardFault). |
| **Internal SRAM** | **16 KB total** (`0x2000_0000` – `0x2000_3FFF`). Static kernel allocations, DMA buffers, stacks, and display canvas consume ~8–10 KB. **Available shared dynamic application arena: 3.5 KB**. |
| **Internal Flash** | **128 KB on-chip Flash**. High-speed XIP (zero-wait-state up to 24 MHz, 1 wait-state at 48/72 MHz). Occupied by primary KERN microkernel (~68 KB). Flashing at runtime is strictly avoided to prevent wear and execution stalls. |
| **External Flash** | **2 MB SPI NOR Flash (PY25Q16)**. Connected over high-speed hardware SPI (`bsp::spi`). **No hardware memory-mapped QSPI/XIP controller**. Code residing in external SPI flash cannot be executed directly by the CPU instruction fetcher without copying or streaming to SRAM. |
| **Display** | 128×64 monochrome LCD (ST7565 / UC1701). Framebuffer is 1024 bytes (1-bit packed). |
| **Radio & Audio** | BK4829 transceiver controlled via bit-bang SPI bus. Audio PA (LN4898) controlled via GPIO + DAC/PWM. |

---

## 3. Package File Identification & Magic Number

To eliminate string-parsing bloat, maintain a strict KERN identity, and prevent transfer corruption, `.krx` packages utilize a single, non-ASCII, symmetry-optimized 32-bit magic number:

```
Hex Bytes (Little Endian):  0x7F, 0x4B, 0x52, 0x58   ("\x7FKRX")
Little-Endian u32:          0x58524B7F
```

```rust
pub const KRX_MAGIC_LE: u32 = 0x58524B7F; // "\x7FKRX"
```

### Rationale
- **High-Bit Guard (`0x7F`)**: Identical to the ELF standard (`\x7FELF`). Immediately catches accidental ASCII transfers (e.g. FTP or serial text streaming), non-binary corruption, and null dereferences.
- **Canonical KERN Signature**: Bytes `K`, `R`, `X` uniquely identify the binary as a KERN Realtime eXecutable across all tools and hex editors.
- **Strict Singleton**: One universal magic number. No legacy or alternative variants.

---

## 4. Execution Topologies (Four Intuitive Classes)

Applications are categorized into four distinct, single-word execution classes:

| Class Name | Execution Medium | Architectural Mechanism | Size Limit | Primary Use Cases |
| :--- | :--- | :--- | :--- | :--- |
| **`Native`** | SRAM Overlay | Loaded entirely into the 3.5 KB dynamic RAM arena with differential base relocations. Max speed, lowest latency. | ≤ 3.5 KB | Fast Spectrum waterfall, real-time RF DSP loops, 60 FPS games. |
| **`Paged`** | SPI Streamed SRAM Cache | Software-managed virtual memory via LR-Thunking and PLT Interception. 1 KB RAM cache. | **Arbitrary** (Paged from 2 MB flash) | Complex ported apps, full protocol stacks (APRS, DMR, AX.25), large games. |
| **`VMachine`** | Bytecode VM (KVM-16) | Cortex-M0+ optimized 16-bit virtual registers. High density, 100% sandboxed. | **Arbitrary** (Streamed from SPI flash) | Portable utilities, calculators, automation scripts, community tools. |
| **`Declare`** | Serialized State Engine | Interpreted structural UI trees and state graphs. Zero code compilation. | **Arbitrary** (Streamed from SPI flash) | Interactive menus, repeater guides, frequency books, cheatsheets, config wizards. |

---

## 5. Position-Independent Native Relocation (Eliminating Pinned VMAs)

In prior firmware overlay implementations (such as F4HWN), applications were linked against a hardcoded virtual memory address (`APP_VMA = 0x20000280`). Any change in kernel RAM layout broke all existing binary packages.

`.krx` solves this using **Single-Base PIC Relocations (`-fPIC -msingle-pic-base -mpic-register=r9`)**:

### 1. Code Independence
- All function calls within the `.text` segment compile to PC-relative branches (`B`, `BL`).
- Constant literal reads compile to PC-relative loads (`LDR rX, [PC, #offset]`).
- The code segment requires **zero text relocations** and can execute at any memory address.

### 2. Data & GOT Access via `r9`
- Global variables in `.data` and `.bss`, as well as GOT function pointers, are addressed relative to the static base register `r9` (`LDR rX, [r9, #offset]`).
- Upon launch, the OS loads the app into whatever contiguous RAM slice is free, sets `r9 = app_data_base`, and jumps to the entry point.

### 3. Differential Delta Relocation Table
The only values requiring relocation are absolute function pointers stored inside initialized `.data` structures. `.krx` encodes these using a **byte-delta offset stream**:
```rust
// Stream of byte offsets between words needing relocation:
// Example: Relocations at offsets 0x10, 0x14, 0x18, 0x30
// Delta stream: [0x10, 0x04, 0x04, 0x18, 0x00]  (Only 5 bytes!)
```
Loader execution takes less than 15 microseconds:
```rust
let mut offset = 0usize;
for &delta in reloc_deltas {
    if delta == 0 { break; }
    offset += delta as usize;
    unsafe {
        let ptr = app_data_ptr.add(offset) as *mut u32;
        *ptr += app_ram_base;
    }
}
```

---

## 6. Deep Dive: "Paged" Native Apps & Software Demand Paging

Because the ARM Cortex-M0+ operates on a flat memory model and entirely lacks a hardware Memory Management Unit (MMU), virtual memory scaling for the **`Paged`** class is achieved purely through software:

```
┌────────────────────────────────────────────────────────────────────────┐
│               Software Demand Paging Pipeline (Paged Apps)             │
└────────────────────────────────────────────────────────────────────────┘

 [ Function in Block A ] ──> calls Function in Block B (Non-Resident)
            │
            ▼
    [ PLT Trampoline ]
            │
            ▼ (Block B not in SRAM Cache)
  [ OS Page-Fault Handler ]
            │
            ├─► 1. Overwrite Link Register (LR) with OS Return-Thunk
            ├─► 2. Trigger high-speed SPI DMA burst fetch of Block B (1 KB)
            └─► 3. Patch PLT stub with active Block B entry point
            │
            ▼
  [ Execute Function in Block B ]
            │
            ▼ (Function Returns via LR)
    [ OS Return-Thunk ]
            │
            └─► Restores Block A into SRAM cache if evicted, resumes caller
```

### 1. Linker Overlays & Partitioning
- The application is linked via a custom GCC/LLVM linker script with `-mlong-calls`.
- Code (`.text`) and read-only data (`.rodata`) are sliced into discrete **1 KB or 512-byte blocks** stored sequentially in the `.krx` payload.

### 2. PLT (Procedure Linkage Table) Interception
- Function calls across block boundaries do not branch directly to fixed memory addresses. Instead, they jump to a compact 8-byte PLT trampoline stub.
- The stub checks whether the target block is currently resident in the active SRAM cache slot.

### 3. LR-Thunking for Return Safety
- When a page fault occurs, the OS intercepts the jump and **overwrites the ARM Link Register (`LR`) with an OS Return-Thunk**.
- When Block B finishes execution, the CPU returns to the return-thunk instead of blindly returning to a potentially evicted Block A.
- The return-thunk verifies whether Block A is still in the cache; if not, it transparently page-faults Block A back into RAM and resumes the caller.
- This provides **infinite call-depth scaling** without risking internal MCU flash wear or hardware stalls.

---

## 7. Optimized Binary Declarative Container Specification

The container eliminates string-based FourCC tags in favor of a **minimalist, numeric declarative binary layout** naturally aligned to 4-byte boundaries. The complete metadata header for a typical application requires **less than 128 bytes total**.

```
0x0000 ┌──────────────────────────────────────────────┐
       │ Core Preamble (16 Bytes Fixed)               │
       ├──────────────────────────────────────────────┤
       │ Section 0x0001: Identity (36 Bytes)          │
       ├──────────────────────────────────────────────┤
       │ Section 0x0002: Memory & Execution (20 Bytes)│
       ├──────────────────────────────────────────────┤
       │ Section 0x0003: Capability Tokens (12-24 B)  │
       ├──────────────────────────────────────────────┤
       │ Section 0x0004: Asset Directory (36-64 B)    │
0x0080 ├──────────────────────────────────────────────┤
       │ Executable / Data Payload (4-Byte Aligned)   │
       │  - Native Code (.text, .rodata, .data)       │
       │  - or Paged 1KB Execution Blocks             │
       │  - or KVM-16 Bytecode Stream                 │
       │  - or Serialized Declare UI Tree             │
       ├──────────────────────────────────────────────┤
       │ Asset Payloads (Bitmaps, Audio, Fonts)       │
       └──────────────────────────────────────────────┘
```

### 7.1. Core Preamble (16 Bytes Fixed)

```rust
#[repr(C, packed)]
pub struct KrxPreamble {
    pub magic: u32,             // 0x58524B7F ("\x7FKRX")
    pub app_type: u8,           // 0=Native, 1=Paged, 2=VMachine, 3=Declare
    pub format_version: u8,     // Container ABI version (0x01)
    pub section_count: u16,     // Number of declarative sections (typically 4)
    pub header_len: u16,        // Total header byte length (preamble + all sections)
    pub flags: u16,             // Bit 0: Signed, Bit 1: Compressed, Bit 2: Has Assets
    pub payload_crc32: u32,     // CRC32 checksum over the executable payload
}
```

### 7.2. Declarative Section Header (4 Bytes Fixed)

Immediately following the preamble are `section_count` contiguous sections. Each begins with a 4-byte descriptor:

```rust
#[repr(C, packed)]
pub struct SectionHeader {
    pub section_id: u16,        // Numeric section identifier
    pub length: u16,            // Byte length of section data following this header
}
```

---

## 8. Declarative Section Definitions

### 8.1. Section `0x0001`: Identity & Metadata (36 Bytes)

```rust
#[repr(C, packed)]
pub struct SectionIdentity {
    pub app_uid: [u8; 16],      // Unique alphanumeric identifier (e.g. "kern.aprs\0...")
    pub version_major: u8,      // SemVer Major
    pub version_minor: u8,      // SemVer Minor
    pub version_patch: u8,      // SemVer Patch
    pub min_kernel_abi: u8,     // Minimum KERN kernel ABI version required
    pub author: [u8; 12],       // Author handle or callsign (e.g. "F4HWN\0...")
    pub build_timestamp: u32,   // UNIX Epoch build time
}
```

### 8.2. Section `0x0002`: Memory & Execution Manifest (20 Bytes)

Defines exact hardware resource requirements, allowing the OS to allocate the transient arena deterministically:

```rust
#[repr(C, packed)]
pub struct SectionMemoryManifest {
    pub min_stack_bytes: u16,   // Declared stack requirement (e.g. 256 bytes)
    pub min_heap_bytes: u16,    // Declared heap requirement (e.g. 512 bytes, 0 if none)
    pub page_size: u16,         // Cache block size for Paged apps (512 or 1024)
    pub cache_blocks: u8,       // Number of SRAM cache blocks (typically 1 or 2)
    pub dma_burst_size: u8,     // Optimal SPI DMA transfer burst size (e.g. 64)
    pub entry_point: u32,       // Byte offset of entry point relative to payload start
    pub payload_size: u32,      // Total byte size of executable payload
    pub reloc_offset: u16,      // Offset to Differential Delta Relocation Table (0 if none)
    pub reloc_count: u16,       // Number of relocation entries
}
```

---

## 9. Capability Tokens & Interface Bindings (Section `0x0003`)

In KERN, **capabilities are typed authorization tokens**, identical to the kernel's internal zero-sized types in [`src/kern/src/caps.rs`](file:///home/drew/Repositories/kern/src/kern/src/caps.rs). There are no artificial security prompts or arbitrary restrictions: **requesting a capability token IS requesting the corresponding subsystem interface vtable**.

### 9.1. Unified Capability Binding Entry (6 Bytes)

Section `0x0003` consists of an array of compact 6-byte binding entries:

```rust
#[repr(C, packed)]
pub struct CapTokenBinding {
    pub cap_id: u16,            // Capability Token Identifier
    pub min_abi: u8,            // Minimum required interface ABI level
    pub flags: u8,              // Bit 0: MANDATORY (App cannot run without this token)
                                // Bit 1: OPTIONAL (GOT slot set to NULL if absent)
    pub got_slot_idx: u16,      // Index in app's GOT where resolved vtable pointer is written
}
```

### 9.2. Defined KERN Capability Tokens

| `cap_id` | Capability Token | Subsystem Authorized | Bound Interface VTable |
| :--- | :--- | :--- | :--- |
| **`0x0001`** | **`CapDisplay`** | Framebuffer, canvas drawing, fonts | `DisplayVTable` (`clear`, `pixel`, `text`, `blit`) |
| **`0x0002`** | **`CapRadio`** | BK4829 transceiver, tuning, RSSI | `RadioVTable` (`set_freq`, `get_rssi`, `set_power`) |
| **`0x0003`** | **`CapAudio`** | LN4898 audio amp, tone synth | `AudioVTable` (`play_tone`, `play_melody`, `volume`) |
| **`0x0004`** | **`CapStorage`** | KernFS SPI flash filesystem | `StorageVTable` (`read`, `write`, `delete`, `list`) |
| **`0x0005`** | **`CapKeypad`** | GPIO keypad matrix, key repeat | `InputVTable` (`poll_key`, `wait_key`, `set_repeater`) |
| **`0x0006`** | **`CapPower`** | Battery telemetry, ADC, sleep | `PowerVTable` (`voltage_mv`, `percentage`, `charging`) |
| **`0x0007`** | **`CapSerial`** | UART / USB CDC serial stream | `SerialVTable` (`read_byte`, `write_bytes`, `flush`) |
| **`0x0008`** | **`CapTimer`** | High-precision SysTick, RTC | `TimerVTable` (`millis`, `delay_ms`, `get_rtc`) |
| **`0x0009`** | **`CapUi`** | System compositor, modals | `UiVTable` (`show_modal`, `show_toast`, `show_sheet`) |

### 9.3. Late-Binding Resolution Mechanics
1. During loader initialization, the OS reads each `CapTokenBinding`.
2. The kernel verifies whether the requested capability token exists and satisfies `min_abi`.
3. If valid, the kernel writes the physical memory address of the service vtable directly into `app_ram.got[binding.got_slot_idx]`.
4. In application code, calls compile down to a single register load and branch:
   ```assembly
   ldr   r3, [got_ptr, #DISP_DRAW_PIXEL]
   blx   r3
   ```
   **Zero syscall overhead, zero context-switch penalty, identical latency to monolithic code.**

---

## 10. Embedded Asset Subsystem & Zero-RAM DMA Streaming (Section `0x0004`)

To avoid filesystem clutter and support rich graphics and audio on a 16 KB MCU, `.krx` packages bundle assets into an indexed directory within the package.

### 10.1. Asset Directory Header & Descriptors

Section `0x0004` begins with an `AssetHeader` followed by an array of 16-byte `AssetDescriptor`s:

```rust
#[repr(C, packed)]
pub struct AssetHeader {
    pub asset_count: u16,       // Total number of assets in directory
    pub reserved: u16,
}

#[repr(C, packed)]
pub struct AssetDescriptor {
    pub asset_id: u16,          // Unique ID within app (e.g. 0x0000 = ICON, 0x0001 = SPLASH)
    pub kind: u8,               // AssetKind (see below)
    pub flags: u8,              // Bit 0: STREAM_DMA (can stream direct to hardware without RAM)
                                // Bit 1: RLE_COMPRESSED (compressed via byte-run RLE)
    pub file_offset: u32,       // Byte offset from start of .krx file to asset payload
    pub length: u32,            // Byte length of asset payload
    pub meta: u32,              // Kind-specific metadata
}
```

### 10.2. Asset Kinds & Metadata Encoding

| `kind` | Asset Type | `meta` Field Encoding |
| :--- | :--- | :--- |
| **`0x01`** | **1-Bit Bitmap** | `(width: u16) | ((height: u16) << 16)` (e.g. 16×16 icon, 128×64 splash) |
| **`0x02`** | **Bitmap Font** | `(char_w: u8) | (char_h: u8 << 8) | (first_char: u8 << 16)` |
| **`0x03`** | **Melody / Tone** | Tone sequence format: `0=KRGR`, `1=KT`, `2=RTTTL` |
| **`0x04`** | **Raw Audio Stream** | `(sample_rate_hz: u16) | ((channels: u8) << 16) | ((bits: u8) << 24)` |
| **`0x05`** | **Binary Resource** | Unstructured binary data / calibration table / translation bundle |

### 10.3. Zero-RAM Hardware DMA Streaming
For assets marked with `STREAM_DMA`:
- **Display Streaming**: When an app calls `sys_display_blit_asset(asset_id, x, y)`, the kernel reads `file_offset` from the asset table and programs the SPI DMA controller to transfer pixels **directly from external SPI Flash to the ST7565 LCD controller**.
- **Audio Streaming**: When playing a chime via `sys_audio_play_asset(asset_id)`, the kernel streams audio data directly to the audio PWM/DAC hardware buffer.
- **The application allocates 0 bytes of execution SRAM for images or audio!**

---

## 11. Declarative State-Machine UI Engine (`Declare` Class)

For non-algorithmic utility applications—such as repeater directories, band plans, frequency guides, cheatsheets, and configuration forms—compiling native machine code is unnecessary overhead.

The **`Declare`** class (`app_type = 3`) serializes the user interface directly into a compact binary scene graph:

```rust
#[repr(C, packed)]
pub struct DeclareNode {
    pub node_id: u16,           // Unique screen node identifier
    pub layout: u8,             // 0=Menu/List, 1=Form, 2=ScrollText/Book, 3=InfoCard
    pub item_count: u8,         // Number of interactive child items
    pub title_offset: u16,      // Offset to title string in string pool
    pub reserved: u16,
}
```

### Direct Kernel UI Mapping
- The KERN microkernel already includes a zero-alloc GUI engine in [`src/lib/gfx/src/gui/modules/menu.rs`](file:///home/drew/Repositories/kern/src/lib/gfx/src/gui/modules/menu.rs) (`MenuView`, `MenuItem`, `MenuItemType`).
- When a `Declare` package is launched, the OS binds `MenuView` directly to the binary node tree streamed from SPI Flash.
- **Result**: Complex multi-page interactive reference manuals and tools run using **less than 128 bytes of RAM** for navigation state, with zero executable code compilation.

---

## 12. Dynamic RAM, Multitasking & The Symbian UI

With a constrained 3.5 KB application arena in the PY32F071 MCU, RAM must be dynamically tracked and visually exposed to the user.

```
0x2000_0000 ┌──────────────────────────────────────────────┐
            │ Kernel Static BSS / Data                     │
            ├──────────────────────────────────────────────┤
            │ Event Queue, IPC Queue, Peripheral States    │
            ├──────────────────────────────────────────────┤
            │ Display Framebuffer (1024 Bytes)             │
            ├──────────────────────────────────────────────┤
            │ [TRANSIENT APPLICATION ARENA: 3.5 KiB]       │
            │  - App 1 Context (64B) + Stack (256B)        │
            │  - App 2 Context (64B) + Stack (128B)        │
            │  - Active Paged Native Cache Block (1024B)   │
            │  - Shared Heap & GOT Table                   │
            ├──────────────────────────────────────────────┤
            │ System Call Stack (Down-growing from 0x4000) │
0x2000_3FFF └──────────────────────────────────────────────┘
```

### 1. 64-Byte Hardware Context Saving
A complete Cortex-M0+ thread execution context (`r0`–`r12`, `sp`, `lr`, `pc`, `xPSR`) requires exactly **64 bytes**. 
By preserving this 64-byte block alongside each app's declared stack (~128–256 bytes), KERN can keep **4 to 5 apps resident in the background simultaneously**.

### 2. Symbian-Style Task Manager
- Holding down the physical **MENU** key triggers a hardware-accelerated Task Manager overlay.
- Lists all active `.krx` processes, their live RAM footprint, and running/suspended states.
- If a user attempts to launch an application that exceeds the remaining RAM budget, the OS intercepts launch and presents a modal prompt to terminate or freeze idle background tasks.

---

## 13. KVM-16: Ultra-Fast Micro Virtual Machine Specification

For the **`VMachine`** class (`app_type = 2`), KERN provides **KVM-16**, an ultra-fast register-based micro-VM engineered strictly around ARMv6-M microarchitecture.

### 1. Machine Architecture
- **8 Virtual 32-bit Registers (`v0`–`v7`)**: Directly mapped to ARM low registers (`r4`–`r7`) in the inner dispatch loop.
- **Fixed 16-bit Instructions**: Aligned with halfword memory access (`LDRH`), eliminating decode branch penalties.

### 2. Theoretical Peak Dispatch Loop (ARMv6-M Assembly)
```assembly
// Direct Computed Jump Dispatch Loop (4 instructions, 6 CPU cycles)
kvm16_dispatch_next:
    ldrh    r2, [r0]            // Fetch 16-bit instruction [PC]
    adds    r0, r0, #2          // PC += 2
    uxtb    r3, r2              // Extract 8-bit opcode
    lsls    r3, r3, #2          // Offset = opcode * 4
    ldr     r3, [r1, r3]        // Load handler address from dispatch table
    bx      r3                  // Jump directly to handler
```

---

## 14. Developer Tooling & Toolchain Pipeline (`cargo-krx` / `krx-pack`)

To make `.krx` seamlessly buildable by developers and easily adoptable by future firmwares, a standard toolchain workflow is established:

```
  [ Rust App (no_std) ]          [ C / C++ App ]
           │                            │
     cargo build                   arm-none-eabi-gcc
   (-fPIC -msingle-pic-base)     (-fPIC -msingle-pic-base)
           │                            │
           └────────────┬───────────────┘
                        ▼
                 [ Raw ELF32 ]
                        │
                        ▼ (cargo-krx / krx-pack)
       ┌─────────────────────────────────┐
       │ 1. Strip ELF metadata & symbols │
       │ 2. Extract .text, .data, .rodata│
       │ 3. Compute Differential Relocs  │
       │ 4. Compress Bitmaps & Audio     │
       │ 5. Emit Preamble & Sections     │
       │ 6. Append CRC32 Checksum        │
       └─────────────────────────────────┘
                        │
                        ▼
                 [ Final .krx ]
```

### SDK Integration Crate (`krx-rt`)
Applications written in Rust import a lightweight `#![no_std]` runtime:
```rust
#![no_std]
#![no_main]

use krx_rt::prelude::*;

#[krx_entry]
fn main(ctx: &mut KrxContext) {
    ctx.display.clear(Color::Black);
    ctx.display.draw_str(10, 10, "Hello KERN!");
    ctx.display.blit();
}
```

---

## 15. Summary of Key Architectural Decisions

1. **Format Name & Extension**: **`.krx`** (KERN Realtime eXecutable).
2. **Magic Constant**: **`0x58524B7F`** (`b"\x7FKRX"`). Single canonical magic with high-bit corruption guard.
3. **Unified Capabilities & APIs**: Capabilities are typed authorization tokens (`CapRadio`, `CapDisplay`, etc.) that directly bind subsystem VTables into the app's GOT table. No artificial policy limits or permission modals.
4. **Position-Independent Relocations**: Single-PIC base (`-msingle-pic-base -mpic-register=r9`) with differential delta offsets, eliminating pinned VMAs completely.
5. **Zero-RAM Hardware Asset Streaming**: Assets in Section `0x0004` declare dimensions/sample rates and stream directly from SPI Flash via SPI DMA to the display controller or audio DAC without consuming execution SRAM.
6. **Ultra-Compact Container Layout**: 16-byte preamble + 4 compact numeric sections (`Identity`, `Memory`, `Capabilities`, `Assets`) fitting inside a single 128-byte read.
7. **Execution Spectrum**: Four focused topologies: **`Native`** (SRAM), **`Paged`** (Demand-paged SPI overlays), **`VMachine`** (KVM-16 bytecode), and **`Declare`** (Serialized state UI).
8. **Demand Paging**: LR-thunking and PLT interception allowing infinite application scaling on Cortex-M0+ with zero internal flash wear.
9. **Symbian Multitasking**: 64-byte context switching supporting 4–5 resident background applications with interactive task manager.
10. **Universal Toolchain**: `cargo-krx` packaging pipeline turning standard GCC/LLVM ELF binaries into production-grade `.krx` files.
