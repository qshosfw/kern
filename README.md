# kern

Kern is an experimental `no_std` Rust microkernel for the Quansheng UV-K1, UV-K5 v3, and UV-K10Pro handheld transceivers, built for the Puya PY32F071xB ARM Cortex-M0+ microcontroller.

The project explores running a memory-safe, modular microkernel with capability-checked relocatable userland executables inside tight MCU constraints (128 KB internal Flash, 16 KB SRAM).

> **Status**: Under active development. Core kernel abstractions, peripheral drivers, KernFS storage, RigWire protocol, KRX runtime, and system modules (`dfu.mod`, `panic.mod`, `sysinfo.krx`) are functional. Radio applications (Dual VFO, scanner, channel workflows) are prototypes undergoing active bring-up.

---

## Hardware Targets

- **Radios**: Quansheng UV-K1, UV-K5 v3 (shared base image), UV-K10Pro (`--target UV-K10Pro`)
- **MCU**: Puya PY32F071xB (ARM Cortex-M0+, 48 MHz, optional 72 MHz overclock)
- **Internal Memory**: 128 KB Flash (`0x08002800` application base after 10 KB stock bootloader), 16 KB SRAM
- **External Storage**: 2 MB SPI Flash (PY25Q16)
- **RF Transceiver**: Beken BK4829 (BK4819 register compatible)
- **Display**: ST7565 monochrome LCD (128×64)

---

## Design Principles

1. **Zero Heap Allocation**: `#![no_std]` across the entire workspace with `alloc` disabled. All buffers are statically arena-backed or compile-time bounded with `heapless`.
2. **Layered Decoupling**: Application and userland code never touch hardware registers directly. Radio, display, audio, and storage operations pass through microkernel traits (`RfChannel`, `RfTuning`, `DisplayBackend`, `StorageBackend`).
3. **Power-of-Two Geometries (ORDS-01)**: Cortex-M0+ lacks a hardware divider. Table records (16 B headers, 32 B contacts/zones/bands, 64 B channels) use power-of-two strides so record offsets resolve via single bit-shifts (`16 + (index << N)`).
4. **Relocatable Userland Packages (KRX)**: System modules and userland applications compile as independent position-independent `.krx` / `.mod` binaries (supporting N1 native PIC, P0 structured leaf paging, and P1 transparent paging) talking to the kernel over a single-entry `kern_call` syscall ABI.
5. **Framed Binary Serial Protocol (RigWire)**: COBS-framed packets with CRC-32 verification handle KernFS file transfers, package sideloading, telemetry, and device control over USB-C CDC or UART.

---

## Repository Layout

- `src/` — Rust workspace crates:
  - `src/kernel/` — Microkernel core, scheduler, KRX ABI (`abi.rs`), capability checks, boot records.
  - `src/platform/` — Hardware traits, PAC, HAL, and board support packages (BSP).
  - `src/driver/` — Peripheral drivers (BK4829, ST7565, PY25Q16, USB CDC, UART, keypad, audio, battery).
  - `src/lib/` — Shared libraries (`kernfs`, `gfx`, `crypto`, `calib`, `utils`).
  - `src/firmware/` — Runtime entrypoint, syscall dispatcher, UI compositor, RigWire service, built-in shell.
- [`packages/`](packages/README.md) — Canonical KRX C SDK (`packages/sdk/`) and relocatable packages (`dfu`, `panic`, `sysinfo`, `beacon`, `bench`, `leaf_app`, `paged_app`).
- [`tools/`](tools/README.md) — Build driver, ABI header generator, ELF bloat analyzer, static arch linter, KRX packer, RigWire client, flasher, and QEMU helper.
- [`media/`](media/README.md) — Project media and presentation assets.

---

## Quick Start

### Prerequisites
- Rust nightly toolchain (configured automatically via `rust-toolchain.toml`, with `thumbv6m-none-eabi` target and `rust-src`)
- Python 3.11+
- `arm-none-eabi-gcc` & `binutils-arm-none-eabi` (for building C KRX packages and extracting binaries)

### Common Commands

The `./kern` script wraps the workspace build, test, package, and provisioning tools:

```bash
# Type and syntax check (0 warnings policy)
./kern check

# Verify Rust ABI and C SDK header (packages/sdk/krx_ops.h) match
./kern abi --check

# Run host unit tests across the workspace
./kern test

# Build and verify all KRX C packages under packages/
./kern pkg

# Compile release firmware and check Flash/SRAM budgets
./kern build
./kern build --target UV-K10Pro

# Print detailed Flash/SRAM section breakdown or symbol bloat report
./kern size
./kern bloat report

# Flash packed firmware over serial bootloader or sideload modules via RigWire
./kern flash --port /dev/ttyACM0
./kern wire sideload packages/dfu/dfu.mod --force

# Launch external QEMU hardware emulator Web UI (requires KERN_EMU_DIR / KERN_QEMU_BIN)
./kern emu
```

---

## Memory Budgets

| Region | UV-K1 / UV-K5 v3 | UV-K10Pro | Notes |
|:---|:---|:---|:---|
| **Flash** | 120,832 B (118 KB) | 122,880 B (120 KB) | Application budget above 10 KB stock bootloader |
| **SRAM** | 16,384 B (16 KB) | 16,384 B (16 KB) | Static arena allocation; `.ramfunc` hot paths |
| **Heap** | 0 B | 0 B | Enforced at compile time (`no_std`, no `alloc`) |

---

## Safety & Legal Notices

- **Calibration Backup**: Back up your radio's factory calibration data before flashing any custom firmware. See [`SAFETY.md`](SAFETY.md) for memory, RF, and recovery details.
- **Amateur Radio Licensing**: Transmitting on amateur frequencies requires an appropriate amateur radio license and compliance with local spectrum regulations (FCC, CE, IARU).

---

## Prior Art

Kern draws hardware knowledge and inspiration from the Quansheng open-source firmware community:
- [DualTachyon](https://github.com/DualTachyon) — Original open-source UV-K5 firmware.
- [Egzumer](https://github.com/egzumer) — Spectrum analyzer and custom feature integration.
- [Armel (F4HWN)](https://github.com/armel) & [@muzkr](https://github.com/muzkr) — Puya PY32F071 port and bootloader tooling.
- [DeltaFW](https://github.com/qshosfw/deltafw-k1-k5v3) — UI layout and ergonomics reference.
- [GoguFW](https://github.com/Gogu-Qs/GOGUFW-UV-K1-Messenger) — Radio messaging and range-check concepts.

---

## License

Licensed under the **GNU General Public License v3.0 or later** ([`LICENSE`](LICENSE)), with the **KRX Syscall Exception** ([`COPYING`](COPYING)) permitting standalone third-party `.krx` applications that interact solely through the published KRX-ABI.
