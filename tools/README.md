# Kern Developer Tools

Host-side build drivers, binary analyzers, ABI code generators, KRX packers, serial provisioning tools, and QEMU emulator helpers.

Most workflows are exposed through the root `./kern` CLI (`./kern help`).

---

## Tool Catalog

### Core CLI & Cross-Platform Foundation
- `cli_ui.py` — Zero-dependency cross-platform CLI foundation shared across all Kern tools (Linux, macOS, Windows). Enforces `NO_COLOR`/`FORCE_COLOR`/TTY detection, Windows VT100 ANSI & UTF-8 console setup, aligned tables, progress bars, `stderr` diagnostics, `os.pathsep` `cargo_env()`, and clean POSIX exit codes (`0`, `1`, `2`, `130`).
- `kern_cli.py` — Unified cross-platform `./kern` (Linux/macOS) and `kern.cmd` (Windows) command dispatcher with grouped help output and `--version`.

### Build, ABI & Static Analysis
- `builder.py` — Core backend for `./kern build`, `./kern check`, `./kern size`, `./kern info`, and `./kern flash`. Resolves `kern.toml` feature gates and hardware target profiles (`UV-K1`, `UV-K5v3`, `UV-K10Pro`), enforces Flash/SRAM budgets, extracts raw/packed binaries, and optionally exports JSON memory telemetry (`--json`).
- `./kern abi [--check]` — KRX ABI header generator. Not a script: it runs the `abigen` host binary of the `src/abi` crate, which prints `packages/sdk/krx_ops.h` from the compiled `abi!` and `abi_sub!` tables.
- `bloat.py` — ELF symbol, section, rodata, call-site, and monomorphization analyzer (`./kern bloat`). Supports baseline capture and delta diffing across commits.
- `archlint.py` — Heuristic static architectural smell scanner (`./kern lint`). Flags layer violations, division traps, unaligned packed fields, oversized functions, and duplicate logic.

### Packaging & Assets
- `kfw.py` — `.kfw` v1.0 firmware container packer, inspector, verifier, signer, and extractor (`./kern kfw`). Zero-dependency Python stdlib implementation using 100% Ascon cryptography (`Ascon-Hash256`, `Ascon-AEAD128`, `Ascon-128-MAC`, post-quantum `LMS-Ascon`) and Solid `ARMTHUMB+LZMA2` compression.
- `pkg.py` — `./kern pkg` backend: builds every package from its `packages/<name>/krx.toml` manifest with one compiler invocation, the shared `packages/sdk/krx.ld`, and one set of position-independence and size guards.
- `pack_krx.py` — KRX relocatable package packer and verifier used by `pkg.py`. Capability, icon and scope names resolve against the generated `packages/sdk/krx_ops.h`. Validates headers, capability bitmasks, P0/P1 paged sections, and CRC-32 checksums.
- `pack.py` — Stock Quansheng bootloader XOR obfuscator called by `builder.py` to emit `target/firmware_packed.bin`.
- `png_to_splash.py` — Converts PNG artwork to 128×64 1-bit monochrome splash images (`.img` or PackBits `.rle`).

### Serial, Flashing & Provisioning
- `rigwire.py` — Host CLI and Python client library for the RigWire COBS-framed binary protocol (`./kern wire` / `./rigwire`). Supports KernFS file operations, KRX sideloading, memory inspection, and live telemetry over USB-C CDC or UART.
- `kernfs_mount.py` — Universal multi-platform KernFS filesystem mounter (`./kern mount`). Exposes the radio's LittleFS over RigWire as a hardened localhost-only WebDAV Class 2 drive (native Linux GIO/Nautilus/Dolphin, macOS Finder/`mount_webdav`, Windows Explorer/`net use` — zero kernel drivers) with USB-style volume naming (`Quansheng UV-K1 XXXX`), DNS-rebinding/CSRF guards, RFC 4331 SPI flash quota reporting, ETag caching, OS junk filtering, `/sys/*` & recovery-module protection, pre-write KRX & ORDS-01 validation, and zero-wear idempotent writes.
- `dock/` (standalone package, `./kern dock`) — Native GTK4 + Libadwaita 1.8 hardware companion with 12 distinct Quansheng handset & developer bench models (`k1`, `k1-polymer`, `k1-clear`, `k5v3`, `k5-orange`, `k5-black`, `k5-99`, `k5-99-red`, `uv5r-plus`, `k10-pro`, `k1-naked`, `pcb-r51`), BK4829 DDS-accurate headless audio synth, rotary-encoder scroll wheel control, flush-left tabbed workspace (Home, Radio, Files with async GVFS/WebDAV mount, Flasher, Apps with authentic 1-bit pixel-art icons & bounded header reads, Backups with `.kfw`/`.zip`/calib inspector, Colored Hex Inspector, embedded `Vte-3.91` Terminal + RigWire console, Capture), OS System Tray, 0600 local IPC (`./kern dock ctl`), and declarative plugins. See [dock/README.md](../dock/README.md).
- `flasher.py` — Serial bootloader flashing utility invoked by `./kern flash`.

### Hardware Emulation & Integration Tests
- `emulator_helper.py` — Launcher for the external QEMU `uv-k5-v3` hardware emulator and Web UI (`./kern emu`). Respects `KERN_EMU_DIR` and `KERN_QEMU_BIN` environment overrides.
- `test_kern_emulator.py` — Headless QEMU integration suite verifying boot sequences, keypad navigation, app lifecycle, and display rendering.
- `test_wire_emu.py` — Headless QEMU integration test for RigWire protocol commands over virtualized serial.
