# Kern Userland Packages

Installable applications and system modules built against the [`sdk/`](../sdk/) Rust (`kern-sdk`) and C (`sdk/c/krx.h` + `sdk/c/krx_ops.h`) package toolchain over the single-entry syscall ABI (`kern_call`).

---

## Layout

### `packages/core/` — Core Userland OS Apps
- [`core/vfo/`](core/vfo/krx.toml) (`vfo.krx`, Rust `N1` PIC) — External Dual-VFO Radio Transceiver HUD replacing resident ROM VFO.
- [`core/settings/`](core/settings/krx.toml) (`settings.krx`, Rust `P0` Structured Paged, 17 leaves) — Full System Settings with native `OptionSheet` pickers, `Device` identity (`Model`, `Owner`, `Tag`, `Radio ID`, `Language`, `Bandplans`, `Boot Splash`), `Audio` (`Roger Beep` & `Boot Jingle` choosers), `Buttons` (`Shortcuts` binder), `System Modules` launcher, and scrollable 16×16 `App Detail` manager.

### `packages/sys/` — System & Maintenance Modules
- [`sys/calib/`](sys/calib/krx.toml) (`calib.mod`, Rust `N1` PIC) — Factory RF & battery calibration inspector (`/sys/calib.dat`), automatically listed in `Settings → System → Modules`.
- [`sys/dfu/`](sys/dfu/README.md) (`dfu.mod`, `P0` Structured Paged) — Visual USB-C / UART DFU updater, hardware quiescing, and SRAM hook.
- [`sys/recovery/`](sys/recovery/README.md) (`recovery.mod`, `P0` Structured Paged) — On-device recovery shell and SRAM flasher.
- [`sys/multiboot/`](sys/multiboot/README.md) (`multiboot.mod`, `P0` Structured Paged) — Multi-slot bootloader manager.
- `sys/panic/` (`panic.mod`, `P0` Structured Paged) — Crash inspector, stack/register viewer, Thumb-1 disassembler, and QR report generator.
- [`sys/sysinfo/`](sys/sysinfo/README.md) (`sysinfo.krx`, `P0` Structured Paged) — Device hardware, OTP certificate, power/thermal telemetry, and regulatory info.

### `packages/apps/` — Optional Radio Apps
- `apps/beacon/` (`beacon.krx`, `N1` Native PIC) — Radio/display beacon app.
- `apps/bench/` (`bench.krx`, `N1` Native PIC) — Comprehensive C-ABI and capability validation bench.

### `packages/examples/` — Contributor Starter Templates
- [`examples/README.md`](examples/README.md) — `hello_rust` (Rust `N1`), `hello_c` (C `N1`), `leaf` (`P0` leaf chaining), and `paged` (`P1` demand paging).

---

## Building & Sideloading

```bash
# Build and verify all packages (intermediate artifacts -> target/packages/)
./kern pkg

# Build a single package by short or grouped name
./kern pkg settings
./kern pkg examples/hello_rust

# Verify C SDK opcodes match src/abi
./kern abi --check

# Provision core packages (vfo.krx + settings.krx) over RigWire
python3 tools/provision.py --port /dev/ttyACM0 --bundle core

# Or sideload a single package directly over RigWire
./kern wire sideload packages/core/settings/settings.krx --force
```
