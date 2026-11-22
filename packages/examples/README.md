# Kern Example Packages

Minimal starter templates for building relocatable `.krx` applications with [`sdk/`](../../sdk/).

| Example | Language | Execution Profile | Purpose |
|---|---|---|---|
| [`hello_rust/`](hello_rust/README.md) | Rust (`no_std`) | `n1` (Flat PIC) | Starter Rust app using `kern-sdk`, `rel_strings!`, and custom `8x8`/`16x16` icons |
| [`hello_c/`](hello_c/README.md) | C | `n1` (Flat PIC) | Starter C app using `sdk/c/krx.h` and custom `8x8`/`16x16` icons |

## Build Any Example

```bash
./kern pkg hello_rust
./kern pkg hello_c
```
