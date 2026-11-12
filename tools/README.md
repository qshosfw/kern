# Kern Developer Tools

This directory contains host-side tooling, build drivers, emulators, and binary packaging utilities for the Kern project.

Most utilities are invoked automatically via the root `./kern` CLI script.

---

## Tool Catalog

### 1. Build & Analysis Engine
- **`builder.py`**: Core backend for `./kern`. Handles configuration loading from `kern.toml`, feature flag synthesis, cargo invocation, and bootloader payload generation.
- **`bloat.py`**: Flash memory and RAM symbol analyzer. Disassembles and measures symbol-level size contributions against the 118 KB flash budget.
- **`archlint.py`**: Static architectural compliance linter. Enforces zero-heap constraints, unsafe safety comments, non-circular crate layering, and naming invariants.

### 2. Simulation & Hardware Emulation
- **`emulator_helper.py`**: Driver for the QEMU-based UV-K1 hardware emulator. Starts the simulation and serves the interactive Web UI (LCD display mirror, keyboard injection, serial telemetry).
- **`test_kern_emulator.py`**: Headless automated integration test suite running against the QEMU emulator. Verifies boot sequences, app switching, and frame captures.

### 3. Packaging & Bootloader
- **`pack_krx.py`**: Relocatable application packer for userland KRX executables (`.krx`). Validates section headers and generates relocation tables.
- **`pack.py`**: Bootloader XOR obfuscation helper used by `builder.py` to prepare firmware payloads for stock bootloader flashing.
- **`qsh_packer.py`**: Container packaging utility for multi-app slot management.
- **`png_to_splash.py`**: Converts PNG images into ST7565 LCD splash screens (1-bit binary or PackBits RLE).

### 4. Serial & Provisioning
- **`rigwire.py`**: Host-side CLI and client library for the RigWire COBS binary serial protocol. Supports file transfers to KernFS, memory inspection, and live telemetry.
- **`flasher.py`**: Serial bootloader programming client for flashing firmware payloads over UART / USB-C.

### 5. Font & Asset Generation
- **`convert_fonts_modular.py`**: Build-time modular font converter and glyph subsetter.
- **`gen_tomthumb.py`**: Generator for the ultra-compact TomThumb 3×5 proportional font.
