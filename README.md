# kern — Rust Microkernel Firmware for Quansheng UV-K1 / UV-K5 v3

[![License: MPL 2.0](https://img.shields.io/badge/License-MPL_2.0-blue.svg)](https://opensource.org/licenses/MPL-2.0)
[![Language: Rust no_std](https://img.shields.io/badge/Rust-no__std-orange.svg)](https://www.rust-lang.org/)
[![Target: PY32F071 Cortex-M0+](https://img.shields.io/badge/Target-PY32F071_Cortex--M0+-green.svg)]()

`kern` is a high-performance, memory-safe, zero-heap `no_std` Rust microkernel engineered from scratch for the **Quansheng UV-K1** and **UV-K5 v3** radios powered by the **PY32F071** ARM Cortex-M0+ MCU.

---

## Key Highlights

- ⚡ **SRAM Hot-Path Execution**: Critical RF polling and DMA display pipelines execute out of 16KB SRAM (`.ramfunc`) with 0 wait-states @ 72MHz.
- 🖼️ **Visual DFU Bootloader**: 10KB graphical bootloader featuring animated progress bars, boot logos, and USB/UART DFU recovery on the ST7565 LCD display.
- 🛡️ **Memory Tagging & Guarding**: Stack painting (`0xDEADBEEF`), struct magic tags (`0x4B45524E`), stack watermarking, and persistent SRAM flight recorder (`.flight_recorder`).
- 📂 **Advanced Filesystem Metadata**: LittleFS implementation on 2MB SPI Flash utilizing `lfs_attr` for transparent file compression, encryption, and app manifests.
- 📻 **Universal Open Protocol Suite**: Rigwire COBS binary protocol, OM-SA sub-audible telemetry standard, DVS (Dynamic Voice Scrambling), and KLink coprocessor bridge.
- 💻 **Native Desktop Emulator**: Built-in x86_64 SDL2 desktop simulator (`cargo run --bin sim`) to test UI, graphics, and radio logic on Linux without hardware.
- 🔒 **RDP & Anti-Tamper Security**: Option byte management (RDP Level 1 protection), Ed25519 firmware signature verification, and HMAC calibration integrity.
- 📜 **MPL-2.0 License**: File-level copyleft license encouraging upstream contributions while enabling vendor and commercial collaboration.

---

## Architecture Overview

```
┌────────────────────────────────────────────────────────────────────────┐
│                      Layer 4: Applications (app)                        │
│         Dual VFO  │  Spectrum Analyzer  │  Channel Scanner  │  Settings │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                    Layer 3: System Services & Framework                │
│    Radio Engine (radio) │ Graphics (gfx) │ Filesystem (fs) │ Proto/KWP   │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                    Layer 2: Device Drivers (dev)                       │
│    BK4819 RF  │  ST7565 LCD  │  PY25Q16 Flash  │  Keyboard Matrix       │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                  Layer 1: Core Microkernel (kern)                      │
│   Event Loop │ Task Scheduler │ Static Arena │ Stack Monitor │ Flight  │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│             Layer 0.5: Platform Abstraction Traits (platform)          │
│   DisplayBackend │ RadioBackend │ StorageBackend │ InputBackend │ Serial │
└─────────────────┬──────────────────────────────────┬───────────────────┘
                  │                                  │
      (Target: thumbv6m)                             │ (Target: x86_64)
                  ▼                                  ▼
┌───────────────────────────────────┐    ┌───────────────────────────────┐
│ Real Physical Hardware (arch/dev) │    │ Desktop Simulator (sim)       │
└───────────────────────────────────┘    └───────────────────────────────┘
```

---

## Technical Documentation Suite

Comprehensive technical documentation is available in the [`docs/`](docs/) directory:

- 📐 [`docs/architecture.md`](docs/architecture.md) — 5-Layer Workspace, Static Arena, SRAM Execution
- 🖥️ [`docs/bootloader.md`](docs/bootloader.md) — Visual ST7565 DFU Bootloader Specification
- 🛡️ [`docs/memory-tagging.md`](docs/memory-tagging.md) — Stack Painting, Struct Magic Tags, Flight Recorder
- 💻 [`docs/console.md`](docs/console.md) — Interactive VT100 Text Serial Console & Shell REPL
- 📡 [`docs/rigwire.md`](docs/rigwire.md) — Rigwire COBS Binary Framing Protocol Specification
- 🌐 [`docs/protocols.md`](docs/protocols.md) — Unified Bus Protocol & Node Discovery Overview
- 🆔 [`docs/node-identity.md`](docs/node-identity.md) — Hermes Node Identity & Memory-Aware Contact Architecture
- 🔊 [`docs/audio.md`](docs/audio.md) — PHY Drivers (FskPhy/SubAudiblePhy/ScramblerPhy), Sub-Audible Telemetry, DVS (Dynamic Voice Scrambling)
- 🔐 [`docs/security.md`](docs/security.md) — Threat Model, PY32 RDP Option Bytes, Ed25519 Signatures
- 📂 [`docs/filesystem.md`](docs/filesystem.md) — Zero-Overhead Flat Binary Schema & `lfs_attr` Tags
- 🔌 [`docs/expansion.md`](docs/expansion.md) — Si4732/Si4735 HF Mod Driver & Remote Compute Bridge
- 🎮 [`docs/emulator.md`](docs/emulator.md) — x86_64 SDL2 Desktop Radio Simulator
- ⚡ [`docs/flashing.md`](docs/flashing.md) — Calibration Protection, Backup & Unbrick Recovery Manual
- 🤝 [`docs/contributing.md`](docs/contributing.md) — Coding Standards, Safety Invariants, Commit Rules
- 📊 [`docs/gogufw-analysis.md`](docs/gogufw-analysis.md) — Feature Analysis: Porting GoguFW Messenger & Mesh Features to Rust
- 🔌 [`docs/uart-analysis.md`](docs/uart-analysis.md) — Architectural comparison of GoguFW's legacy UART vs `kern`'s Rigwire
- 🔬 [`docs/deltafw-analysis.md`](docs/deltafw-analysis.md) — Architectural analysis of the advanced `k1-fw` DeltaFW implementation
- 🏗️ [`docs/apps-architecture.md`](docs/apps-architecture.md) — Novel App Paradigms: Virtual Framebuffers, Plan 9 VFS, Zero-Copy
- 📻 [`docs/radio-services.md`](docs/radio-services.md) — Modular, Event-Driven Architecture for Traditional Radio Features

---

## Quick Start

### Build Firmware for Quansheng Radio Target
```bash
cargo build --release --target thumbv6m-none-eabi
```

### Launch Desktop Radio Simulator (x86_64 Linux)
```bash
cargo run --bin sim
```

---

## License

This project is licensed under the **Mozilla Public License 2.0 (MPL-2.0)**. See [`LICENSE`](LICENSE) for details.
