# Contributing & Engineering Standards

Welcome to `kern`! We are building a high-performance, mathematically rigorous, memory-safe Rust microkernel for ham radio devices.

To maintain code elegance, zero-heap safety, and structural integrity across the codebase, all contributors must follow these strict engineering standards.

---

## 1. Safety & Architecture Rules

1. **Strict 5-Layer Dependency Hierarchy**:
   - `arch` -> `platform` -> `kern` -> `dev` -> `radio`/`gfx`/`fs`/`proto`/`security` -> `app` -> `firmware`
   - Cyclic imports between crates are prohibited.
2. **Zero Heap Allocation (`no_std`)**:
   - Do not use `alloc`, `Vec`, `Box`, or `String`.
   - Use fixed-capacity stack arrays, static arena allocations, or stack-backed vectors (e.g. `ArrayVec`).
3. **Minimize `unsafe` Blocks**:
   - `unsafe` is strictly restricted to low-level hardware register access in `arch/pac.rs`.
   - Every `unsafe` block must include a `// SAFETY:` comment explaining why the invariant holds.
4. **No Blocking Interrupts**:
   - Interrupt Service Routines (ISRs) must be ultra-short (under 10 microseconds).
   - ISRs write event bitmasks to the atomic ring buffer; processing occurs in the main thread event loop.

---

## 2. Code Formatting & Quality Checks

Before submitting a pull request or committing changes:

```bash
# 1. Format code according to Rust standard style
cargo fmt --all

# 2. Run strict clippy linting (warnings are treated as errors)
cargo clippy --all-targets -- -D warnings

# 3. Execute unit test suite (runs on host x86_64)
cargo test --all

# 4. Verify cross-compilation for ARM target
cargo build --release --target thumbv6m-none-eabi

# 5. Check memory budget bounds
cargo size --release --target thumbv6m-none-eabi
```

---

## 3. Commit Message Conventions

We follow Conventional Commits format:

- `feat(dev)`: Add Si473x SSB bandwidth selection logic
- `fix(arch)`: Correct SPI1 DMA channel clear flag bitmask
- `docs(kwp)`: Add sequence number anti-replay section to KWP spec
- `refactor(gfx)`: Optimize ST7565 dirty page rendering loop
- `perf(kern)`: Move RSSI polling loop into `.ramfunc` SRAM section
