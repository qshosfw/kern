//! Compile-time decoding of ASCII pixel art into packed 1-bit bitmaps.
//!
//! Every icon in this crate is authored as a readable grid (`#` = ink, `.` =
//! paper), one row per line, so diffs and reviews show the artwork itself.
//! Decoding runs entirely in `const` context: the firmware image contains only
//! the packed MSB-first rows, and malformed art (wrong row width or row count,
//! stray characters, or an entry filed under the wrong identifier) is a build
//! error instead of a corrupted glyph.

/// Decode `H` rows of `W` pixels (`W <= 8`) into one byte per row.
const fn rows<const H: usize>(art: &str, w: usize) -> [u8; H] {
    let b = art.as_bytes();
    let mut out = [0u8; H];
    let (mut r, mut c, mut i) = (0usize, 0usize, 0usize);
    while i < b.len() {
        match b[i] {
            b'#' => {
                assert!(r < H && c < w, "icon art overflows its cell");
                out[r] |= 0x80 >> c;
                c += 1;
            }
            b'.' => {
                assert!(r < H && c < w, "icon art overflows its cell");
                c += 1;
            }
            b'\n' => {
                if c != 0 {
                    assert!(c == w, "icon art row has the wrong width");
                    r += 1;
                    c = 0;
                }
            }
            b' ' => {}
            _ => panic!("icon art may only contain '#', '.' and whitespace"),
        }
        i += 1;
    }
    assert!(c == 0 && r == H, "icon art has the wrong number of rows");
    out
}

/// Decode a 16x16 grid into two bytes per row.
const fn rows16(art: &str) -> [u8; 32] {
    let b = art.as_bytes();
    let mut out = [0u8; 32];
    let (mut r, mut c, mut i) = (0usize, 0usize, 0usize);
    while i < b.len() {
        match b[i] {
            b'#' | b'.' => {
                assert!(r < 16 && c < 16, "icon art overflows its cell");
                if b[i] == b'#' {
                    out[r * 2 + c / 8] |= 0x80 >> (c % 8);
                }
                c += 1;
            }
            b'\n' => {
                if c != 0 {
                    assert!(c == 16, "icon art row has the wrong width");
                    r += 1;
                    c = 0;
                }
            }
            b' ' => {}
            _ => panic!("icon art may only contain '#', '.' and whitespace"),
        }
        i += 1;
    }
    assert!(c == 0 && r == 16, "icon art has the wrong number of rows");
    out
}

/// Bake an 8x8 table whose `n`-th entry must carry identifier `base + n`.
pub(super) const fn bake8<const N: usize>(base: u8, art: &[(u8, &str); N]) -> [[u8; 8]; N] {
    let mut out = [[0u8; 8]; N];
    let mut n = 0;
    while n < N {
        assert!(
            art[n].0 as usize == base as usize + n,
            "icon filed out of order"
        );
        out[n] = rows::<8>(art[n].1, 8);
        n += 1;
    }
    out
}

/// Bake the 5 px status-bar table; each row width must equal [`Icon::width`].
pub(super) const fn bake5<const N: usize>(art: &[(crate::Icon, &str); N]) -> [[u8; 5]; N] {
    let mut out = [[0u8; 5]; N];
    let mut n = 0;
    while n < N {
        assert!(art[n].0 as usize == n, "status icon filed out of order");
        out[n] = rows::<5>(art[n].1, art[n].0.width() as usize);
        n += 1;
    }
    out
}

/// Bake a 16x16 table whose `n`-th entry must carry slot `n`.
pub(super) const fn bake16<const N: usize>(art: &[(u8, &str); N]) -> [[u8; 32]; N] {
    let mut out = [[0u8; 32]; N];
    let mut n = 0;
    while n < N {
        assert!(art[n].0 as usize == n, "launcher icon filed out of order");
        out[n] = rows16(art[n].1);
        n += 1;
    }
    out
}
