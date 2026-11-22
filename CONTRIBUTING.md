# Contributing to Kern (OpenTaiko)

Welcome to **Kern**, part of the [OpenTaiko (`qshosfw`)](https://github.com/qshosfw) project building a memory-safe, zero-heap `#![no_std]` Rust microkernel and package ecosystem for handheld amateur radio transceivers.

Support ongoing hardware and firmware development at [ko-fi.com/qshosfw](https://ko-fi.com/qshosfw).

To maintain code quality, memory safety invariants, and architectural decoupling, all contributions must adhere to the engineering guidelines below.

---

## 1. Core Engineering Invariants

1. **Strict Zero Dynamic Heap Allocation**:
   - `alloc` is completely disabled. Never introduce `Box`, dynamic `Vec`, `String`, or heap allocations.
   - All buffers must be statically allocated, bump-allocated in the static arena, or bounded with `heapless`.
2. **Strict Layer Decoupling**:
   - Application and userland code must never interact directly with peripheral hardware registers.
   - All radio operations must be mediated through high-level microkernel traits (`RfChannel`, `RfTuning`, `RfModulation`, `RfSquelch`).
   - Graphics operations target the `Canvas` abstraction.
3. **Division-Free Power-of-Two Geometries**:
   - Cortex-M0+ has no hardware divider. All database records (headers 16B, contacts/bands 32B, channels 64B) must use power-of-two lengths ($2^N$) so record seeking uses single bit-shifts (`16 + (index << N)`).
4. **Minimize `unsafe`**:
   - `unsafe` is strictly limited to low-level hardware register access in the PAC/HAL.
   - Every `unsafe` block must include a `// SAFETY:` comment justifying why the invariant holds.
5. **No Blocking Interrupts**:
   - Interrupt Service Routines (ISRs) must be short (under 10 microseconds). ISRs signal event flags; all processing occurs in the main thread event loop.
6. **Zero Warnings**:
   - We enforce a strict zero-tolerance policy for compiler warnings. Every commit must pass `./kern check` with 0 warnings.

---

## 2. Developer Toolchain

Always use the `./kern` CLI for common developer operations:

```bash
# 1. Format code across the entire workspace
./kern fmt

# 2. Fast syntax and type verification (must pass with 0 warnings)
./kern check --deny-warnings

# 3. Verify Rust ABI and C SDK header (sdk/c/krx_ops.h) match
./kern abi --check

# 4. Execute host unit tests
./kern test

# 5. Build and verify KRX Rust & C packages under packages/
./kern pkg all

# 6. Compile release firmware and check Flash/SRAM budgets
./kern build --deny-warnings

# 7. Inspect Flash and SRAM memory section allocation
./kern size

# 8. Provision core + system packages to a radio over RigWire
./kern provision --port /dev/ttyACM0 --bundle sys
```

---

## 3. Commit Message Conventions

We follow Conventional Commits:

- `feat(subsystem)`: New feature or capability
- `fix(subsystem)`: Bug fix
- `refactor(subsystem)`: Code change that neither fixes a bug nor adds a feature
- `perf(subsystem)`: Performance or flash footprint optimization
- `chore`: Toolchain, CI, or dependency maintenance
