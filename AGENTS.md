# Kern Microkernel & Firmware Project Guidelines

These guidelines are always active for all agents working within the `kern` repository.

---

## 1. Core Engineering Philosophy

Every component, interface, driver, and application in Kern must strictly embody these seven architectural principles:

1. **Abstracted**:
   - Strict layered decoupling: microkernel contracts (`platform/traits`, `kern`), peripheral drivers (`driver`), board support packages (`bsp`), and userland applications (`apps`).
   - Applications must never interact directly with peripheral hardware registers (e.g. BK4829, Si4732). All radio operations must be mediated through high-level microkernel traits (`RfChannel`, `RfTuning`, `RfModulation`, `RfSquelch`, `RfSubaudible`, `RfFskPhy`, `RadioService`, `RadioCoordinator`).
   - Graphics operations target the `Canvas` abstraction, allowing seamless support for monochrome LCDs and multi-color RGB panels.
   - Storage operations interact through the high-level KernFS filesystem without exposing block-level flash mechanics to applications.

2. **Lightweight**:
   - **Strict Zero Heap Allocation**: `#![no_std]` execution across the entire workspace. Strictly zero dynamic heap allocations (`alloc`, `Box`, heap `Vec`). All buffers must be stack-allocated or bounded with `heapless`.
   - **Resource Footprint**: The primary MCU target is Cortex-M0+ (PY32F071xB) with 128 KB Flash and 16 KB SRAM. Keep the release firmware under 80 KB Flash (< 65% utilization) to leave ample headroom for installable apps and storage.
   - **Zero Flash Bloat**: Unused features must cost 0 bytes of Flash when disabled via compile-time Cargo feature flags. Avoid virtual method tables (`dyn Trait`) in hot runtime paths; use static dispatch in the BSP.
   - **Crisp Naming**: Prefer clean, minimalist names (`Cap`, `CapsCheck`, `TableFileHeader`, `Shortcut`) over redundant enterprise abstractions (`CapabilityKind`, `CapabilityBroker`, `DbHeader`, `ShortcutAction`).

3. **Efficient**:
   - **$O(1)$ Power-of-Two Geometries ($2^N$ Bytes)**: All database records and table structures must use lengths that are exact powers of two (16B headers/FM, 32B contacts/zones/bands, 64B channels/caps). Record seeking must use a single bit-shift (`offset = 16 + (index << N)`) without division or modulo instructions (Cortex-M0+ has no hardware divider).
   - **Sector Boundary Alignment**: Record sizes must divide cleanly into LittleFS 4 KB flash erase sectors (64 channel records per sector, 128 contact records per sector) to prevent sector-straddling I/O operations.
   - **Two-Tier VFO Resilience**: Frequency and tuning changes update volatile SRAM state instantaneously. Cold-boot persistence to `/user/vfo_a.vfo` and `/user/vfo_b.vfo` occurs on idle or power-down, achieving sub-1.8 ms boot times while preventing flash wear.
   - **Zero-Copy Deserialization**: All fixed-stride headers and records use packed layouts (`#[repr(C, packed)]`) for direct byte-slice mapping and zero-copy flash reads.

4. **Modular**:
   - Fully decoupled subsystems: RF modem, audio amplification, display, USB CDC serial, battery power management, and storage.
   - **Modular Configuration**: Settings are partitioned into independent CRC32-protected files (`/pref/display.cfg`, `/pref/audio.cfg`, `/pref/keys.cfg`, `/pref/battery.cfg`, `/pref/system.cfg`) rather than a single monolithic binary blob.
   - Subsystems must be independently feature-gated via Cargo features.

5. **Flexible**:
   - **Target-Agnostic Design**: While initially targeting UV-K1 and UV-K5 (PY32F071xB @ 48 MHz), code must anticipate future hardware variations (10W PA stages, secondary broad-band HF receivers like the Si4732 covering 150 kHz - 30 MHz, GPS/GNSS telemetry, BLE serial programming, and color RGB displays) without structural refactoring.
   - **Dynamic Modulation Profiles**: Microkernel accepts high-level `ModulationProfile` definitions (RF bandwidth, AF filter shaping, AGC response time, squelch, and audio routing) to dynamically arbitrate FM, AM, SSB, FSK, CW, or digital packet modes.
   - **Multi-Bus Support**: Support `BusType::Internal`, `BusType::I2c { address }`, `BusType::Spi { cs_pin }`, `BusType::Uart { port }`, and `BusType::Usb` natively.

6. **Extensible**:
   - **Opaque App Extensions**: Core record formats must reserve 16-byte `user_tag` slots (`[u8; 4]` tag + `[u8; 12]` payload) for userland apps to store custom metadata without requiring schema migrations in core table formats.
   - **Declarative Driver Registry**: Peripheral expansion must use typed `PeripheralDescriptor` arrays rather than hardcoded bitfields.
   - **Executable App Bundles**: Installable `.app` / `.krx` (KERN Relocatable Executable) bundles loaded from KernFS are capability-checked against hardware tokens before execution.

7. **Elegant**:
   - **NO Type Aliases**: Strictly avoid using type aliases as backward-compatibility crutches, compatibility shims, or cosmetic rename layers. When refactoring types, refactor all definition and call sites cleanly across the codebase.
   - **NO Informal "Magic" Terminology**: Never use informal terms like "magic numbers" or "magic constants". Use standard systems terminology: "format identifier", "FourCC", "ident", "tag", "type_id", or `TABLE_ID_*` / `FILE_ID_*`.
   - **No Proprietary Quirks**: Do not introduce archive containers (`.cplg`, `.tar`, `.zip`, `.qsh`), proprietary `.db` extensions, or `.k` prefixes. VFO files are strictly `.vfo`.
   - **Zero Warnings**: Zero-tolerance policy for compiler warnings. Every change must pass `./kern check` with 0 warnings.

8. **KISS (Keep It Simple, Sensible)**:
   - Embody simplicity and sensible restraint across designs, interfaces, data structures, and UI layouts.
   - Do not take minimalism to an extreme that harms modularity, robustness, or architectural clarity.
   - Eliminate unnecessary layers of indirection, over-generalized abstractions, and convoluted mechanisms whenever a clean, direct solution exists.

---

## 2. Storage & Filesystem Architecture (ORDS-01)

- **Direct LittleFS Storage**: Flat, uncompressed files directly on SPI Flash. No archive containers.
- **Filesystem Hierarchy**:
  - `/user/`: User-modifiable operational data:
    - `/user/channels.ch` (64-byte records, 16-byte header `CHAN`)
    - `/user/contacts.ct` (32-byte records, 16-byte header `CONT`)
    - `/user/bandplans.bnd` (32-byte records, 16-byte header `BNDP`)
    - `/user/vfo_a.vfo`, `/user/vfo_b.vfo` (160-byte composite VFO state: 32B header + 2x 64B channel records)
    - `/user/scanner.hit` (32-byte records, 16-byte header `HITS`)
    - `/user/zones.zn` (32-byte records, 16-byte header `ZONE`)
    - `/user/fm_radio.fm` (16-byte records, 16-byte header `FMBC`)
  - `/pref/`: Domain-separated configuration files (`display.cfg`, `audio.cfg`, `keys.cfg`, `battery.cfg`, `system.cfg`).
  - `/sys/`: Factory and hardware calibration:
    - `/sys/calib.dat` (512-byte raw calibration payload)
    - `/sys/calib.sig` (32-byte Poly1305 hardware authentication MAC)
    - `/sys/hw.cfg` (Hardware revision manifest)
- **Geometry Rules**:
  - Headers: 16 bytes.
  - Contacts / Zones / Hits / Bands: 32 bytes ($2^5$).
  - Channels / Capabilities: 64 bytes ($2^6$).
  - Calibration: 512 bytes ($2^9$).
  - Offset calculation: `16 + (index << N)`.

---

## 3. Hardware & Driver Contracts

- **Driver Trait**: Every peripheral driver implements the zero-cost `Driver` trait:
  ```rust
  pub trait Driver {
      fn descriptor(&self) -> PeripheralDescriptor;
      fn bus(&self) -> BusType;
      fn init(&mut self) -> Result<(), ()>;
  }
  ```
- **Capability Checks**: Applications check capabilities using the $O(1)$ bitwise token API:
  ```rust
  if ctx.has_cap(Cap::BroadBandRx) { ... }
  ```
- **Decoupled RF Services**: All radio operations route through the microkernel `RadioCoordinator` or `RadioService` trait facets (`RfChannel`, `RfTuning`, `RfModulation`, `RfSquelch`, `RfSubaudible`, `RfFskPhy`, `RfToneGenerator`).

---

## 4. Developer CLI Toolchain

Always use the `./kern` CLI for project tasks:

- `./kern check`: Fast type and syntax verification (must pass with **0 warnings**).
- `./kern build`: Compile release firmware and pack into bootloader payload.
- `./kern size`: Display detailed Flash and SRAM memory section breakdown.
- `./kern fmt`: Format all Rust code across the workspace using `rustfmt`.
- `./kern disasm`: Disassemble compiled Cortex-M0+ release binary.
- `./kern flash`: Flash firmware image to connected radio via serial bootloader.
- `./kern clean`: Clean build artifacts.
