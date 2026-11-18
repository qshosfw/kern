# kern

Kern is an experimental `no_std` Rust microkernel for the Quansheng UV-K1 and UV-K5 v3 handheld transceivers, powered by the Puya PY32F071xB ARM Cortex-M0+ microcontroller.

The project explores whether a memory-safe, modular microkernel architecture with relocatable userland applications can run effectively within the tight constraints of low-cost amateur radio hardware (128 KB Flash, 16 KB SRAM).

> **Project Status**: Under active development. Core kernel abstractions, hardware drivers, filesystem storage, and development tooling are functional. Application layers (Dual VFO, settings, and userland apps) are currently prototypes undergoing active bring-up.

---

## Hardware Targets

- **Microcontroller**: Puya PY32F071xB (ARM Cortex-M0+, 48 MHz, optional 72 MHz overclock)
- **Memory**: 128 KB Internal Flash, 16 KB SRAM
- **External Storage**: 2 MB SPI Flash (PY25Q16)
- **RF Transceiver**: Beken BK4829 (BK4819 compatible)
- **Display**: ST7565 monochrome LCD (128×64)

---

## Technical Constraints & Design Principles

1. **Zero Dynamic Allocation**: `alloc` is completely disabled. All buffers are statically allocated or bounded at compile time using `heapless`. The firmware maintains a hard invariant of 0 bytes heap usage.
2. **Layered Decoupling**: Application code never interacts directly with peripheral registers. Radio, display, audio, and storage operations are mediated through high-level microkernel traits (`RfChannel`, `RfTuning`, `DisplayBackend`, `StorageBackend`).
3. **Power-of-Two Geometries (ORDS-01)**: The Cortex-M0+ core lacks a hardware division instruction. Database records (16-byte headers, 32-byte contacts/zones, 64-byte channels) use power-of-two lengths so record offsets can be computed with a single bit-shift (`16 + (index << N)`).
4. **Relocatable Userland Runtime (KRX)**: Userland programs are compiled as separate `.krx` binaries that execute via a capability-checked system call interface.
5. **Framed Serial Protocol (RigWire)**: A binary serial protocol using Consistent Overhead Byte Stuffing (COBS) and CRC32 verification replaces ad-hoc ASCII/hex dumps for telemetry, control, and filesystem transfers.

---

## Repository Layout

The repository is structured into four primary directories:

- `src/` — Firmware source code and microkernel workspace crates:
  - `src/kernel/`: Core microkernel, scheduler, capability broker, and memory monitoring.
  - `src/platform/`: Hardware traits, PAC, HAL, and board support packages (BSP).
  - `src/driver/`: Peripheral drivers (BK4829 transceiver, ST7565 LCD, PY25Q16 flash, keypad).
  - `src/lib/`: Modular libraries (KernFS filesystem, graphics engine, crypto, calibration, byte utilities).
  - `src/firmware/`: Main application runtime, system services, and boot orchestration.
- `packages/` — Userland application packages (`bench`, `leaf_app`, `paged_app`, `beacon`).
- `tools/` — Developer toolchain, QEMU emulator wrapper, and analysis scripts.
- `media/` — Project media and presentation assets.

---

## Quick Start

The repository includes a developer CLI script (`./kern`) that wraps cargo and the project toolchain:

```bash
# Check syntax and types across the workspace (0 warnings policy)
./kern check

# Run host unit tests
./kern test

# Build the release firmware binary
./kern build

# Inspect Flash and SRAM memory section breakdown
./kern size

# Launch the interactive QEMU emulator on localhost:8080 (no hardware required)
./kern emu

# Run the native desktop UI simulator (Linux / macOS via SDL2)
cargo run --bin sim

# Flash release binary to a connected radio via bootloader
./kern flash --port /dev/ttyACM0
```

---

## Memory Budget

Target bounds for the release firmware:

| Section | Allocated Budget | Notes |
|:---|:---|:---|
| **Flash** | 120,832 bytes (118 KB) | 10 KB reserved for permanent bootloader |
| **SRAM** | 16,384 bytes (16 KB) | Static arena bump-allocation; `.ramfunc` execution |
| **Heap** | 0 bytes | Hard compile-time boundary |

Check current usage with `./kern size`.

---

## Safety & Legal Notices

- **Calibration Data**: Before flashing custom firmware, back up your radio's factory calibration block (`/sys/calib.dat`). Calibration data contains device-specific crystal oscillator offsets and RF gain tables.
- **Amateur Radio Licensing**: A valid amateur radio license is required to transmit on amateur frequencies. Ensure your transmissions comply with local spectrum regulations (FCC, CE, IARU).

---

## Lineage & Prior Art

Kern builds on the reverse-engineering work and firmware ecosystem established by the Quansheng community:
- [DualTachyon](https://github.com/DualTachyon) — Original open-source UV-K5 firmware.
- [Egzumer](https://github.com/egzumer) — Spectrum analyzer integration and custom features.
- [Armel (F4HWN)](https://github.com/armel) & [@muzkr](https://github.com/muzkr) — Porting to the Puya PY32F071 MCU.
- [DeltaFW](https://github.com/qshosfw/deltafw-k1-k5v3) — UI design and ergonomics reference.
- [GoguFW](https://github.com/Gogu-Qs/GOGUFW-UV-K1-Messenger) — Messenger and range-check features.

---

## License

GNU General Public License v3.0 ([GPL-3.0-or-later](LICENSE)), with a narrow [KRX syscall exception](COPYING) for standalone third-party userland applications that interact with the kernel exclusively through the published KRX-ABI system call table.

The kernel, drivers, platform layers, libraries, firmware, and any modifications or forks thereof are fully GPL v3. See [COPYING](COPYING) for the complete licensing terms.
