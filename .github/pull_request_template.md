## Description

<!-- Describe your changes and the motivation/context behind them. -->

## Subsystem Impact
- [ ] Microkernel Core (`src/kernel`)
- [ ] Platform Traits, PAC, HAL & BSP (`src/platform/`)
- [ ] Device Drivers (`src/driver`)
- [ ] Shared Libraries (`src/lib/`)
- [ ] Firmware Runtime & Services (`src/firmware`)
- [ ] Userland KRX Packages & SDK (`packages/`)
- [ ] Toolchain, CI & Scripts (`tools/`, `.github/`)

## Verification Checklist

- [ ] **Strict Zero Heap Allocation**: No `alloc`, `Box`, or dynamic heap allocations introduced.
- [ ] **Zero Warnings**: Passes `./kern check --deny-warnings` (and `--target UV-K10Pro`) with **0 warnings**.
- [ ] **ABI Header Sync**: Passes `./kern abi --check` (`packages/sdk/krx_ops.h` matches `src/kernel/src/abi.rs`).
- [ ] **Host Unit Tests**: Passes `./kern test`.
- [ ] **KRX Packages**: Passes `./kern pkg` (if ABI or packages were touched).
- [ ] **Firmware Build & Memory Budget**: Passes `./kern build` within Flash (118 KB / 120 KB) and SRAM (16 KB) budgets.
- [ ] **Code Formatting**: Formatted via `./kern fmt`.
- [ ] **No Informal Magic Terminology / Redundant Type Aliases**: Uses standard systems identifiers (`FILE_ID_*`, `TABLE_ID_*`, `tag`, `ident`).
