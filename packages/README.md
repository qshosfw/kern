# Kern Userland Packages & KRX SDK

Userland applications and loadable system modules built against the Kern Relocatable Executable (KRX) single-entry syscall ABI (`kern_call`).

Keeping non-essential system UI (DFU graphics, crash forensics, system info) in external Flash-paged `.mod` / `.krx` modules keeps the core microkernel firmware compact inside internal MCU Flash.

---

## Layout

### C SDK
- `sdk/` — Canonical KRX C SDK:
  - `krx.h`: Core ABI types, `kern_call` trampoline, and helper inline wrappers.
  - `krx_ops.h`: Auto-generated opcodes and sub-opcodes synced from `src/abi` via `./kern abi`.
  - `krx.ld`: Shared Cortex-M0+ Thumb-1 PIC linker script.

### System Modules
- [`dfu/`](dfu/README.md) (`dfu.mod`, P0 Structured Paged) — Visual USB-C / UART DFU bootloader UI, hardware quiescing, and 30 Hz SRAM SysTick telemetry hook.
- `panic/` (`panic.mod`, P0 Structured Paged) — On-device crash inspector, register/stack viewer, Thumb-1 disassembler, QR report encoder, and safe-boot recovery menu.
- [`sysinfo/`](sysinfo/README.md) (`sysinfo.krx`, P0 Structured Paged) — Firmware/hardware identity, live power/thermal telemetry, flash-paged `QS-1` AES-128-CBC verification, and regulatory marks.

### Reference & Test Applications
- `beacon/` (`beacon.krx`, N1 Native PIC) — Minimal radio/display beacon reference app.
- `bench/` (`bench.krx`, N1 Native PIC) — KRX syscall and capability benchmark harness.
- `leaf_app/` (`leaf.krx`, P0 Structured Paged) — Reference multi-leaf paged application.
- `paged_app/` (`paged.krx`, P1 Transparent Paged) — Reference fixed-window transparent-paged application.

---

## Building & Sideloading

Requires `arm-none-eabi-gcc` and Python 3.

```bash
# Build all packages and verify ABI/relocation/size invariants
./kern pkg

# Build a specific package
./kern pkg dfu
./kern pkg sysinfo

# Verify C SDK opcodes match src/abi
./kern abi --check

# Sideload a built package onto a connected radio over RigWire
./kern wire sideload packages/dfu/dfu.mod --force
./kern wire sideload packages/sysinfo/sysinfo.krx --force
```
