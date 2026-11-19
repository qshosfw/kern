# Contributing to Kern

Welcome to Kern. We are building a memory-safe, zero-heap `no_std` Rust microkernel for handheld amateur radio transceivers.

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

# 3. Verify Rust ABI and C SDK header (packages/sdk/krx_ops.h) match
./kern abi --check

# 4. Execute host unit tests
./kern test

# 5. Build and verify KRX C packages under packages/
./kern pkg

# 6. Compile release firmware and check Flash/SRAM budgets
./kern build --deny-warnings

# 7. Inspect Flash and SRAM memory section allocation
./kern size
```

---

## 3. Commit Message Conventions

We follow Conventional Commits:

- `feat(subsystem)`: New feature or capability
- `fix(subsystem)`: Bug fix
- `refactor(subsystem)`: Code change that neither fixes a bug nor adds a feature
- `perf(subsystem)`: Performance or flash footprint optimization
- `chore`: Toolchain, CI, or dependency maintenance
