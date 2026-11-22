## Description

<!-- Describe your changes and the motivation/context behind them. -->

## Subsystem Impact
- [ ] Microkernel Core & ABI (`src/kernel`, `src/abi`)
- [ ] MCU, HAL, Traits & Board Support (`src/mcu/`, `src/board/`, `src/hal-traits/`)
- [ ] Device Drivers (`src/drivers/`)
- [ ] Filesystem, Graphics, Crypto & Utilities (`src/fs`, `src/gfx`, `src/crypto`, `src/calib`, `src/util`)
- [ ] Firmware Runtime & Services (`src/firmware/`)
- [ ] Userland KRX Packages & SDK (`packages/`, `sdk/`)
- [ ] Kern Dock Companion (`dock/`)
- [ ] Toolchain, CI & Scripts (`tools/`, `.github/`)

## Verification Checklist

- [ ] **Strict Zero Heap Allocation**: No `alloc`, `Box`, or dynamic heap allocations introduced.
- [ ] **Zero Warnings**: Passes `./kern check --deny-warnings` (and `--target UV-K10Pro`) with **0 warnings**.
- [ ] **ABI Header Sync**: Passes `./kern abi --check` (`sdk/c/krx_ops.h` matches `src/abi/`).
- [ ] **Host Unit Tests**: Passes `./kern test`.
- [ ] **KRX Packages**: Passes `./kern pkg` (if ABI or packages were touched).
- [ ] **Firmware Build & Memory Budget**: Passes `./kern build` within Flash (118 KB / 120 KB) and SRAM (16 KB) budgets.
- [ ] **Code Formatting**: Formatted via `./kern fmt`.
- [ ] **No Informal Magic Terminology / Redundant Type Aliases**: Uses standard systems identifiers (`FILE_ID_*`, `TABLE_ID_*`, `tag`, `ident`).
