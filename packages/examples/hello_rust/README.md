# Hello Rust (`hello_rust.krx`)

Minimal `#![no_std]` Rust `N1` position-independent `.krx` starter app built with `kern-sdk`.

- Manifest: [`krx.toml`](krx.toml)
- Source: [`src/main.rs`](src/main.rs)
- Demonstrates: `krx_entry!`, position-independent `rel_strings!`, reading OS properties (`prop::CONTRAST`), rendering to the compositor (`clear`, `draw_rect`, `draw_text`, `commit`), and embedding custom `8x8`/`16x16` launcher icons in `SECTION_ASSETS`.

```bash
./kern pkg hello_rust
```
