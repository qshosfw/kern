# Hello C (`hello_c.krx`)

Minimal C `N1` position-independent `.krx` starter app built with `sdk/c/krx.h`.

- Manifest: [`krx.toml`](krx.toml)
- Source: [`src/hello_c.c`](src/hello_c.c)
- Demonstrates: `.text.entry`, reading OS properties via `KRX_OP_PROP_GET`, keypad handling, LCD drawing, and embedding custom `8x8`/`16x16` icons in `SECTION_ASSETS`.

```bash
./kern pkg hello_c
```
