# Kern Microkernel & Firmware Project Guidelines

These guidelines are always active for all agents working within the `kern` repository.

---

## 1. Core Engineering Philosophy

Every component, interface, driver, and application in Kern must strictly embody these seven architectural principles:

1. **Abstracted**:
   - Strict layered decoupling: microkernel contracts (`src/abi`, `src/hal-traits`, `src/kernel`), peripheral drivers (`src/drivers`), board support packages (`src/board/*`), and userland applications (`packages/*`).
   - Applications must never interact directly with peripheral hardware registers (e.g. BK4829, Si4732). All radio operations must be mediated through high-level microkernel traits (`RfChannel`, `RfTuning`, `RfModulation`, `RfSquelch`, `RfSubaudible`, `RfFskPhy`, `RadioService`, `RadioCoordinator`).
   - Graphics operations target the 4-layer `gfx` crate (`canvas`, `assets`, `elements`, `views`), allowing seamless support for monochrome LCDs (`dirty_x0/x1` DMA page-span blits) and future multi-color RGB panels.
   - Storage operations interact through the high-level KernFS v2.1 filesystem without exposing block-level flash mechanics to applications.

2. **Lightweight**:
   - **Strict Zero Heap Allocation**: `#![no_std]` execution across the entire workspace. Strictly zero dynamic heap allocations (`alloc`, `Box`, heap `Vec`). All buffers must be stack-allocated, arena-backed (`6 KiB` `TRANSIENT_ARENA` + disjoint `512 B` `KERNEL_IO_SCRATCH`), or bounded with `heapless`.
   - **Resource Footprint**: The primary MCU target is Cortex-M0+ (`PY32F071xB`) with 128 KB Flash and 16 KB SRAM. Keep the release firmware under 80 KB Flash (< 65% utilization) to leave ample headroom for installable apps and storage.
   - **Zero Flash Bloat**: Unused features must cost 0 bytes of Flash when disabled via compile-time Cargo feature flags. Avoid virtual method tables (`dyn Trait`) in hot runtime paths; use static dispatch (`BoardSpec` ZSTs) in the BSP (`&mut dyn FnMut` is acceptable only on cold flash directory walks to avoid generic closure monomorphization bloat).
   - **Crisp Naming**: Prefer clean, minimalist names (`Cap`, `CapsCheck`, `TableHeader`, `Shortcut`) over redundant enterprise abstractions (`CapabilityKind`, `CapabilityBroker`, `DbHeader`, `ShortcutAction`).

3. **Efficient**:
   - **$O(1)$ Power-of-Two Geometries ($2^N$ Bytes)**: All database records and table structures must use lengths that are exact powers of two (`16 B` headers/FM, `32 B` contacts/zones/bands/hits/`Pcb`, `64 B` channels/caps/`WireSession`/`Inode`). Record seeking must use a single bit-shift (`offset = 16 + (index << N)`); general integer division/modulo uses the `PY32F071xB` hardware 32÷32 divider (`DIV` @ `0x4002_3800`, `py32_hal::div` / `Op::SysDiv`).
   - **Silicon DMA & Injected ADC Offload**: Use `PY32F071xB` 7-channel `DMA1` (`SPI1_TX` LCD blits, `SPI2_RX/TX` PY25Q16 flash bursts, `USART1_RX` `256 B` circular ring, `ADC1`/`DAC1` `TIM6`/`TIM7` baseband audio) and `ADC1` injected sequence (`JSQR -> JDR1..3`) for zero-stall battery/thermal/`VREFINT` telemetry.
   - **Sector Boundary Alignment**: Record sizes must divide cleanly into KernFS v2.1 4 KB flash erase sectors (63 channel records + 16 B header, 127 contact records + 16 B header) to prevent sector-straddling I/O operations.
   - **Two-Tier VFO Resilience**: Frequency and tuning changes update volatile SRAM state instantaneously. Cold-boot persistence to `/user/vfo_a.vfo` and `/user/vfo_b.vfo` occurs on idle or power-down, achieving sub-1.8 ms boot times while preventing flash wear.
   - **Zero-Copy Deserialization**: All fixed-stride headers and records use packed layouts (`#[repr(C, packed)]`) for direct byte-slice mapping and zero-copy flash reads.

4. **Modular**:
   - Fully decoupled subsystems: RF modem, audio amplification (`ToneSequencer` non-blocking cues), display, USB CDC / UART / BLE serial, battery power management, and storage.
   - **Modular Configuration**: Settings are partitioned into 6 independent CRC32-protected files (`/pref/display.cfg`, `/pref/audio.cfg`, `/pref/keys.cfg`, `/pref/system.cfg`, `/pref/battery.cfg`, `/pref/apps.cfg`) addressed through a unified `u8` property & live telemetry registry (`abi::prop`, `0x00..=0xFE`).
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

## 2. Storage, ABI & Wire Architecture

- **Direct KernFS v2.1 Storage**: Flat, uncompressed log-structured files directly on SPI Flash (`64 B` aligned 12-root `Inode` supporting up to `384 KiB` / `393,216 B` per file, `24 B` inline payload, atomic batched page commits, wear-leveled GC, optional Ascon-128 AEAD when `secure_fs` is enabled). No archive containers.
- **Canonical ABI v5 (`abi::VERSION = 5`)**: Centralized `kern_call` gate enforcing 22 orthogonal operations (`OP_TABLE` across 9 domains), pre-flight Cortex-M0+ stack guard, `Cap` permission checks, packed `Sig` pointer-signature validation (`In`, `Out`, `Str`), unified scalar/live telemetry registry (`abi::prop`, `0x00..=0xBF` durable `/pref/*.cfg` + `0xC0..=0xFE` live scalars), and typed `abi::Status` returns across Rust, C89/C99, Zig, and Luau `.krx` SDKs (`sdk/`).
- **Universal RigWire v2**: Firmware-agnostic binary serial protocol (`8 B` naturally-aligned `WireHeader`, terminator-only in-place COBS, `abi::Status` error replies, `Ascon-128` session auth/AEAD, `CORE::CAPS` `SemanticCap` & `CORE::TABLES` FourCC table discovery, 256 B page CRC-32 delta sync, multi-port `SerialRouter` write-fencing).
- **Lean Resident Failsafe Launcher**: Resident `APP_ID_LAUNCHER` (`Home`, default startup app `startup_app = 0`) uses a minimal ROM list launcher (`ListRowSpec` + `icon_8`) with optional `/apps/home.krx` override and automatic failsafe fallback.
- **Filesystem Hierarchy**:
  - `/user/`: User-modifiable operational data:
    - `/user/channels.ch` (64-byte records, 16-byte header `CHAN`)
    - `/user/contacts.ct` (32-byte records, 16-byte header `CONT`)
    - `/user/bandplans.bnd` (32-byte records, 16-byte header `BNDP`)
    - `/user/vfo_a.vfo`, `/user/vfo_b.vfo` (160-byte composite VFO state: 32B header + 2x 64B channel records)
    - `/user/scanner.hit` (32-byte records, 16-byte header `HITS`)
    - `/user/zones.zn` (32-byte records, 16-byte header `ZONE`)
    - `/user/fm_radio.fm` (16-byte records, 16-byte header `FMBC`)
  - `/pref/`: Domain-separated configuration files (`display.cfg`, `audio.cfg`, `keys.cfg`, `system.cfg`, `battery.cfg`, `apps.cfg`).
  - `/sys/`: Factory and hardware calibration:
    - `/sys/calib.dat` (512-byte raw calibration payload)
    - `/sys/calib.sig` (32-byte Ascon-128 hardware authentication MAC)
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
- `./kern build`: Compile release firmware, `.kfw` v1.0 container, and bootloader payload.
- `./kern size`: Display detailed Flash and SRAM memory section breakdown (enforces `flash_ceiling` ratchet in `kern.toml`).
- `./kern test`: Run native host unit tests across all workspace crates (`cargo test --manifest-path emu/Cargo.toml --target x86_64-unknown-linux-gnu` runs the `kern-emu` ARMv6-M + PY32F071 silicon twin tests).
- `./kern abi --check`: Verify generated SDK headers (`sdk/c/krx_ops.h`) match `src/abi`.
- `./kern pkg`: Build `.krx` / `.mod` userland packages (`packages/*`).
- `./kern fmt`: Format all Rust code across the workspace using `rustfmt`.
- `./kern disasm`: Disassemble compiled Cortex-M0+ release binary.
- `./kern flash`: Flash firmware image to connected radio via serial bootloader.
- `./kern clean`: Clean build artifacts.
- `python3 tools/check_firmware_invariants.py`: Verify chip-isolation (`R-NO-CHIP-IN-KERNEL`), zero-heap, and architectural invariants.
- `python3 tools/test_rigwire_sdk.py`: Verify cross-language RigWire v2 & `.krx` SDK toolchains (Python, C89/C99/C++98, ES5 JS, Zig, Luau AOT).

---

## 5. Agent Operational Directives & Invariants

1. **Plan & Architectural Preservation**:
   - Never truncate, simplify, or overwrite master implementation plans or architecture specifications with less detail. Always preserve historical design rationale, silicon-level Cortex-M0+ details, and system roadmaps.
2. **State & Lifecycle Test Coverage**:
   - Any modifications affecting app switching, navigation transitions (e.g. VFO to Launcher, Settings to Launcher), or view context preservation MUST be guarded by automated unit tests in `src/firmware` or `src/kernel`. Tests must explicitly verify that state and focus are not discarded or cleared unintentionally on exit.
3. **Zero UI/Navigation Lag & Non-Blocking Audio Invariant**:
   - Navigation must feel instantaneous (sub-30ms response). Peripheral operations (e.g. audio amplifier warm-up, boot jingles, key pip / roger tone synthesis via `ToneSequencer`, disk metadata reads) must never introduce blocking delays into the interactive event dispatch loop. File catalogs and package scans must be cached in memory rather than re-queried during UI transitions.
4. **Dependency Hygiene**:
   - Strictly zero redundant, duplicate, or unused crates in any `Cargo.toml`. Standardize workspace editions to Rust 2024.
5. **Multi-Agent File Isolation**:
   - When another agent is actively modifying a designated subsystem (such as boot splash orchestration in `splash.rs` / `orchestration.rs`), strictly avoid touching those files to prevent merge collisions.
6. **Structural Refactoring Only — Zero Micro-Optimization Token Waste**:
   - Never waste reasoning or turns on small micro-optimizations (`#[inline]` tweaking, byte-golfing, or superficial patches). All size, speed, and reliability improvements must come from **deep structural consolidation**: eliminating duplicate layers/schemas, moving generic OS services into `src/kernel`, unifying runtime execution (`P1 v2` `ExtentStreamer`), and ratcheting `flash_ceiling` downward in `kern.toml`.

---

## 6. Architecture Specifications & Next-Gen Execution Blueprint

New agents continuing architectural work in `kern` **must** read [`docs/kern-nextgen-execution-blueprint.md`](docs/kern-nextgen-execution-blueprint.md) first, alongside its companion specifications:

- **[`docs/kern-nextgen-execution-blueprint.md`](docs/kern-nextgen-execution-blueprint.md)** — **Primary Agent Handoff & Execution Roadmap**: Verified post-ABI-v5 baseline, module-by-module diagnosis of remaining resident ROM in `src/firmware`, and ordered atomic execution phases (**Phase A → Phase E**: storage/calib flattening, `P1 v2` `ExtentStreamer` + `.ramfunc` veneer + single-crate `settings.krx`, single-owner `RadioCoordinator<Rf>` + `ChannelRecord` unification + table-driven `kernel::wire` + `DMA1` audio, `UvK10ProBoard` ZST, and `.kfw` v1.0 / `LMS-Ascon` security + Next-Gen `.krx` suite).
- **[`docs/firmware-architecture-overhaul-plan.md`](docs/firmware-architecture-overhaul-plan.md)** — Master end-to-end architectural overhaul plan and Flash/SRAM scorecard.
- **[`docs/kern-nextgen-architecture-and-silicon.md`](docs/kern-nextgen-architecture-and-silicon.md)** — `PY32F071xB` silicon acceleration (`DIV`, `DMA1`, `ADC1 JSQR`, `LPTIM1` STOP wake), memory map, and microkernel boundary design.
- **[`docs/silicon-dma-audio-and-k10pro.md`](docs/silicon-dma-audio-and-k10pro.md)** — Bidirectional `TIM6`/`TIM7` + `DMA1` `ADC1`/`DAC1` audio streaming and `Quansheng UV-K10 Pro` (`Si4732` SSB, RGB PWM backlight, `PHY6235SC` BLE RigWire, `BL1551B` GNSS mux) hardware map.
- **[`docs/kern-open-standards.md`](docs/kern-open-standards.md)** & **[`docs/kfw-spec.md`](docs/kfw-spec.md)** — Normative `RigWire v2`, `KRX` ABI v5, `KernFS v2.1`, FourCC record tables, and `.kfw` v1.0 specifications.
