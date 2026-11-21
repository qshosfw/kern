# Kern Example Packages

Minimal starter templates for building relocatable `.krx` applications with [`sdk/`](../../sdk/).

| Example | Language | Execution Profile | Purpose |
|---|---|---|---|
| [`hello_rust/`](hello_rust/README.md) | Rust (`no_std`) | `n1` (Flat PIC) | Starter Rust app using `kern-sdk`, `rel_strings!`, and custom `8x8`/`16x16` icons |
| [`hello_c/`](hello_c/README.md) | C | `n1` (Flat PIC) | Starter C app using `sdk/c/krx.h` and custom `8x8`/`16x16` icons |
| [`leaf/`](leaf/README.md) | C | `p0` (Structured Paged) | Multi-leaf overlay trampoline chain (`root -> leaf1 -> leaf2`) |
| [`paged/`](paged/README.md) | C | `p1` (Transparent Paged) | Demand-paged fixed-window block execution (`Block 0 -> Block 1`) |

## Build Any Example

```bash
./kern pkg hello_rust
./kern pkg hello_c
./kern pkg leaf
./kern pkg paged
```
