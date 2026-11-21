# Kern OS — OpenTaiko Amateur Radio Microkernel

[![Organization: OpenTaiko](https://img.shields.io/badge/OpenTaiko-qshosfw-1f6feb?style=flat-square&logo=github)](https://github.com/qshosfw)
[![Support on Ko-fi](https://img.shields.io/badge/Support%20on-Ko--fi-FF5E5B?style=flat-square&logo=ko-fi&logoColor=white)](https://ko-fi.com/qshosfw)
[![Rust: no_std](https://img.shields.io/badge/Rust-no__std%20%7C%20zero%20heap-DEA584?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Target: Cortex-M0+](https://img.shields.io/badge/Target-PY32F071xB%20Cortex--M0%2B-0091BD?style=flat-square)](https://github.com/qshosfw)
[![License](https://img.shields.io/badge/License-GPLv3-green?style=flat-square)](LICENSE)

**Kern** is an open-source, zero-heap `#![no_std]` microkernel, RF coordinator, and relocatable application platform developed by **[OpenTaiko (`qshosfw`)](https://github.com/qshosfw)** for modern handheld amateur radio transceivers (Quansheng **UV-K1**, **UV-K5 v3**, and **K10 Pro**).

Built from the ground up in Rust for constrained ARM Cortex-M0+ silicon (`128 KB` Flash / `16 KB` SRAM + `2 MB` SPI Flash), Kern decouples hardware drivers, RF arbitration, power-loss-safe storage, and graphics from installable userland applications (`.krx` / `.mod`) provisioned cleanly over USB-C / UART via **RigWire**.

---

## Highlights

- **Strict Zero-Heap Microkernel (`#![no_std]`):** Zero `alloc`, zero `Box`, zero heap `Vec`; deterministic stack/SRAM budgets (`8 KB` relocatable app execution arena + generous call/IRQ stack headroom).
- **Dynamic RF Arbitration:** Microkernel-mediated `RadioCoordinator`, `RfResourceMask(u16)`, `ScanPlan` scanner state machine, multi-VFO & broad-band receiver contracts, and hardware-safe TX/PA guards.
- **External Userland Packages (`.krx` / `.mod`):** Core apps (`vfo.krx`, `settings.krx`) and system modules (`calib.mod`, `dfu.mod`, `recovery.mod`, `multiboot.mod`, `panic.mod`, `sysinfo.krx`) run outside MCU ROM from SPI Flash with self-contained `SECTION_ASSETS` icons, capability checks, and fault containment.
- **Unified Single-Entry Syscall ABI (`kern_call`):** Shared Rust (`sdk/rust` — `kern-sdk`) and C (`sdk/c/krx.h` + `sdk/c/krx_ops.h`) SDKs generated from a single source of truth (`src/abi`).
- **Power-Loss-Safe Storage (`KernFS`):** Copy-on-write 256-byte page-granular SPI Flash filesystem with CRC32-protected modular config files (`/pref/*.cfg`), power-of-two record geometries, and Ascon-128 hardware-bound calibration (`/sys/calib.dat`).
- **RigWire Serial & USB-C Protocol:** Fast COBS-framed telemetry, live framebuffer streaming, remote keypad/encoder control, WebDAV mounting, and CRC-verified package provisioning (`tools/provision.py` / `./kern wire`).

---

## Hardware Support Matrix

| Radio | MCU | Flash / SRAM | SPI Flash | Status |
|---|---|---|---|---|
| **Quansheng UV-K1** | PY32F071xB (Cortex-M0+ @ 48/72 MHz) | 128 KB / 16 KB | PY25Q16 (2 MB) | Primary Target |
| **Quansheng UV-K5 v3** | PY32F071xB (Cortex-M0+ @ 48/72 MHz) | 128 KB / 16 KB | PY25Q16 (2 MB) | Supported (QEMU + HW) |
| **K10 Pro** | PY32F071xB + GNSS / RGB | 128 KB / 16 KB | PY25Q16 (2 MB) | Experimental (`target_k10pro`) |

---

## Repository Structure

```text
kern/
├── kern                   # Unified CLI toolchain (build, check, test, size, pkg, abi, wire, flash)
├── kern.toml              # Declarative hardware target & feature configuration
├── src/                   # Layered Rust microkernel & firmware workspace
│   ├── abi/               # Single-source-of-truth syscall opcodes, properties, capabilities, C-ABI types
│   ├── kernel/            # Microkernel contracts (proc, ipc/intents, caps, radio, store, power, runtime)
│   ├── fs/                # KernFS copy-on-write SPI Flash filesystem
│   ├── drivers/           # Peripheral drivers (BK4829, BK1080, ST7565, PY25Q16, USB CDC, Wire, Audio)
│   ├── gfx/               # Monochrome/grayscale 2D canvas, fonts, icons, and native UI widgets
│   ├── crypto/            # CRC32, Ascon-128 AEAD/MAC, COBS framing, TRNG
│   ├── calib/             # Factory calibration block parsing & curves
│   ├── util/              # Divisionless Cortex-M0+ math, DSP, formatting, path helpers
│   ├── hal-traits/        # Hardware-agnostic trait contracts
│   ├── board/uv-k1/       # Board support package (pinmux, clocks, UID)
│   ├── mcu/py32f071/      # PY32F071 PAC & HAL
│   └── firmware/          # Resident OS orchestrator, compositor, KRX loader/runtime, RigWire host
├── sdk/                   # Package author SDKs
│   ├── rust/              # kern-sdk (Rust #![no_std] PIC package SDK)
│   ├── c/                 # krx.h & generated krx_ops.h (C N1/P0/P1 package SDK)
│   └── krx.ld             # Position-independent ARM Thumb-1 linker script
├── packages/              # External installable userland packages
│   ├── core/              # vfo.krx (Radio Transceiver), settings.krx (System Settings)
│   ├── sys/               # calib.mod, dfu.mod, recovery.mod, multiboot.mod, panic.mod, sysinfo.krx
│   ├── apps/              # beacon.krx, bench.krx
│   └── examples/          # hello_rust, hello_c, leaf (P0), paged (P1) starter templates
└── tools/                 # RigWire CLI, package builder, provisioner, and bloat/lint analyzers
```

---

## Quick Start

### Prerequisites
- Rust nightly (`rust-toolchain.toml` selects the pinned toolchain with `thumbv6m-none-eabi` + `rust-src`)
- `arm-none-eabi-gcc` / `binutils` (for linking `.krx` packages and firmware ELF/bin)
- Python 3.11+ (`pyserial` for flashing and RigWire)

### Build, Test & Flash
```bash
# Verify ABI headers, type-check with zero warnings, and run all workspace unit tests
./kern abi --check
./kern check --deny-warnings
./kern test

# Build release firmware (.bin, packed bootloader image, .kfw, and QEMU .elf)
./kern build
./kern size

# Build all external userland packages (.krx / .mod)
./kern pkg all

# Flash firmware over serial bootloader
./kern flash
```

### Provisioning Packages Over RigWire
Kern does not bloat MCU Flash by embedding apps into ROM. Provision core apps and system modules directly onto the radio's SPI Flash over USB-C / UART:

```bash
# Provision core apps (vfo.krx + settings.krx)
python3 tools/provision.py --port /dev/ttyACM0 --bundle core

# Provision core apps + system modules (calib, dfu, recovery, panic, sysinfo)
python3 tools/provision.py --port /dev/ttyACM0 --bundle sys

# Or sideload a single package directly via RigWire
./kern wire sideload packages/core/settings/settings.krx --force
./kern wire sideload packages/sys/calib/calib.mod --force
```

### Authoring Your Own Package
Start from any template in [`packages/examples/`](packages/examples/README.md) (`hello_rust`, `hello_c`, `leaf`, or `paged`):

```bash
./kern pkg examples/hello_rust
./kern wire sideload packages/examples/hello_rust/hello_rust.krx --force
```

---

## Contributing & Community

Kern is part of the **[OpenTaiko](https://github.com/qshosfw)** open-source radio firmware ecosystem.
- Read [CONTRIBUTING.md](CONTRIBUTING.md), [SAFETY.md](SAFETY.md), and [SECURITY.md](SECURITY.md) before opening a PR.
- Follow the zero-heap, zero-warning (`./kern check --deny-warnings`), and clean architectural guidelines in [AGENTS.md](AGENTS.md).

### Support OpenTaiko
If Kern or OpenTaiko tools are useful to you and you want to support ongoing hardware research, reverse-engineering, and open-source amateur radio firmware development:

**[Support OpenTaiko on Ko-fi — https://ko-fi.com/qshosfw](https://ko-fi.com/qshosfw)**

---

## License

Licensed under the **GNU General Public License v3.0** — see [LICENSE](LICENSE) and [COPYING](COPYING) for details.
