## Description

<!-- Describe your changes in detail and include the motivation/context behind them. -->

## Subsystem Impact
- [ ] Microkernel Core (`src/kernel`)
- [ ] Platform Traits & BSP (`src/platform/`)
- [ ] Device Drivers (`src/driver`)
- [ ] Filesystem & Storage (`src/lib/kernfs`)
- [ ] Graphics & UI (`src/lib/gfx`)
- [ ] Protocols & Telemetry (`src/firmware`)
- [ ] Userland Packages (`packages/`)
- [ ] Toolchain & Scripts (`tools/`)
- [ ] Documentation (`docs/`)

## Verification Checklist

Please verify that your pull request satisfies all Kern architectural invariants:
- [ ] **Strict Zero Heap Allocation**: No `alloc`, `Box`, or dynamic heap structures introduced.
- [ ] **Zero Warnings**: Passes `./kern check` with **0 warnings**.
- [ ] **Compilation**: Compiles successfully via `./kern build`.
- [ ] **Memory Footprint**: Verified via `./kern size` to ensure Flash (< 120,832 B) and SRAM (< 16,384 B) bounds are respected.
- [ ] **Code Formatting**: Formatted via `./kern fmt` (`cargo fmt --all`).
- [ ] **Test Coverage**: Host unit tests pass via `./kern test` (`cargo test --all`).
- [ ] **No Subsystem Regressions**: Tested on physical hardware or QEMU emulator (`./kern emu`).
- [ ] **No Informal Magic Terminology**: Clean systems terminology used (`FILE_ID_*`, `TABLE_ID_*`, `tag`, `ident`).
- [ ] **No Redundant Type Aliases**: Types cleanly refactored across definition and call sites.
