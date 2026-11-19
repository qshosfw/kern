# Kern Developer Tools

Host-side build drivers, binary analyzers, ABI code generators, KRX packers, serial provisioning tools, and QEMU emulator helpers.

Most workflows are exposed through the root `./kern` CLI (`./kern help`).

---

## Tool Catalog

### Build, ABI & Static Analysis
- `builder.py` — Core backend for `./kern build`, `./kern check`, `./kern size`, `./kern info`, and `./kern flash`. Resolves `kern.toml` feature gates and hardware target profiles (`UV-K1`, `UV-K5v3`, `UV-K10Pro`), enforces Flash/SRAM budgets, extracts raw/packed binaries, and optionally exports JSON memory telemetry (`--json`).
- `gen_abi.py` — Single-source-of-truth KRX ABI code generator (`./kern abi [--check]`). Parses the declarative `abi!` and `abi_sub!` tables in `src/kernel/src/abi.rs` and emits `packages/sdk/krx_ops.h`.
- `bloat.py` — ELF symbol, section, rodata, call-site, and monomorphization analyzer (`./kern bloat`). Supports baseline capture and delta diffing across commits.
- `archlint.py` — Heuristic static architectural smell scanner (`./kern lint`). Flags layer violations, division traps, unaligned packed fields, oversized functions, and duplicate logic.

### Packaging & Assets
- `kfw.py` — `.kfw` v1.0 firmware container packer, inspector, verifier, signer, and extractor (`./kern kfw`). Zero-dependency Python stdlib implementation using 100% Ascon cryptography (`Ascon-Hash256`, `Ascon-AEAD128`, `Ascon-128-MAC`, post-quantum `LMS-Ascon`) and Solid `ARMTHUMB+LZMA2` compression.
- `pack_krx.py` — KRX relocatable package packer and verifier used by `packages/*/build.sh`. Validates headers, capability bitmasks, P0/P1 paged sections, and CRC-32 checksums.
- `pack.py` — Stock Quansheng bootloader XOR obfuscator called by `builder.py` to emit `target/firmware_packed.bin`.
- `png_to_splash.py` — Converts PNG artwork to 128×64 1-bit monochrome splash images (`.img` or PackBits `.rle`).

### Serial, Flashing & Provisioning
- `rigwire.py` — Host CLI and Python client library for the RigWire COBS-framed binary protocol (`./kern wire` / `./rigwire`). Supports KernFS file operations, KRX sideloading, memory inspection, and live telemetry over USB-C CDC or UART.
- `kernfs_mount.py` — Universal multi-platform KernFS filesystem mounter (`./kern mount`). Exposes the radio's LittleFS over RigWire as a hardened localhost-only WebDAV Class 2 drive (native Linux GIO/Nautilus/Dolphin, macOS Finder/`mount_webdav`, Windows Explorer/`net use` — zero kernel drivers) with USB-style volume naming (`Quansheng UV-K1 XXXX`), DNS-rebinding/CSRF guards, RFC 4331 SPI flash quota reporting, ETag caching, OS junk filtering, `/sys/*` & recovery-module protection, pre-write KRX & ORDS-01 validation, and zero-wear idempotent writes.
- `kern_dock.py` — Modern GTK4 + Libadwaita graphical radio companion (`./kern dock`). Renders authentic switchable Quansheng **UV-K1** (gunmetal faceplate, cobalt-blue accents, `MENU < > EXIT`, Ice Blue backlight) and **UV-K5 v3** (matte black chassis, speaker grille, orange accents, `M ▲ ▼ EXIT`, Amber backlight) hardware skins with a crisp Cairo-backed 128×64 ST7565 dot-matrix LCD, exact 4×4 physical keypad + protruding PTT/SK1/SK2 flank (press/hold/release + PC keyboard shortcuts), clickable top LED flashlight, pure-stdlib PNG screenshots, KRX sideloading, and integrated KernFS WebDAV mounting.
- `flasher.py` — Serial bootloader flashing utility invoked by `./kern flash`.

### Hardware Emulation & Integration Tests
- `emulator_helper.py` — Launcher for the external QEMU `uv-k5-v3` hardware emulator and Web UI (`./kern emu`). Respects `KERN_EMU_DIR` and `KERN_QEMU_BIN` environment overrides.
- `test_kern_emulator.py` — Headless QEMU integration suite verifying boot sequences, keypad navigation, app lifecycle, and display rendering.
- `test_wire_emu.py` — Headless QEMU integration test for RigWire protocol commands over virtualized serial.
