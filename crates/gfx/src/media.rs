//! Dynamic Filesystem Media Formats: External Icon Packs (`.icn`), Font Packs (`.fnt`),
//! and Localization Packs (`.lng`).
//!
//! Keeps non-essential icon kits, language fonts (Latin, Cyrillic, Greek, GSM-7, Japanese,
//! Chinese), and translation tables on external SPI Flash (`/user/icons/*.icn`, `/user/fonts/*.fnt`,
//! `/user/lang/*.lng`) instead of internal MCU Flash, while guaranteeing zero-heap $O(1)$ /
//! $O(\log N)$ streaming lookups and crisp built-in fallback glyphs (`□` or `?`).

use crate::draw;
use crate::icons::{FALLBACK_BOX_5, FALLBACK_BOX_8, FALLBACK_QMARK_5, FALLBACK_QMARK_8};
use crate::{Canvas, Color};

/// FourCC format identifier for binary icon packs (`"ICN1"`).
pub const FILE_ID_ICON_PACK: u32 = u32::from_be_bytes(*b"ICN1");
/// FourCC format identifier for binary external font packs (`"FNT1"`).
pub const FILE_ID_FONT_PACK: u32 = u32::from_be_bytes(*b"FNT1");
/// FourCC format identifier for binary localization packs (`"LNG1"`).
pub const FILE_ID_LANG_PACK: u32 = u32::from_be_bytes(*b"LNG1");

/// Flag bit in [`IconPackHeader::flags`]: byte 0 of each record stores variable pixel width `1..=8`,
/// followed by MSB-first row bytes (`1..=cell_h`).
pub const ICON_PACK_FLAG_VAR_WIDTH: u8 = 1 << 0;

/// Built-in visual fallback drawn when an external pack is missing or a glyph/icon ID is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FallbackStyle {
    /// Hollow tofu box `□` sized to the target cell.
    EmptyBox = 0,
    /// Boxed or standalone `?` indicator.
    QuestionMark = 1,
}

/// Declarative reference to a dynamically loaded filesystem icon (e.g. from `/user/icons/kern_kit.icn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalIconRef<'a> {
    pub pack_path: &'a str,
    pub glyph_id: u16,
    pub fallback: FallbackStyle,
}

/// Declarative reference to a dynamically loaded filesystem font (e.g. `/user/fonts/tiny5.fnt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalFontRef<'a> {
    pub font_path: &'a str,
    pub fallback: FallbackStyle,
}

// ── Binary Icon Pack (`.icn`, FourCC `ICN1`) ──────────────────────────────────

/// 16-byte naturally-aligned header for flat binary icon packs (`.icn`).
///
/// Record payloads use exact power-of-two strides (`8`, `16`, or `32` bytes) so seeking is
/// `16 + ((id - base_id) << shift)` with zero integer division and clean LittleFS sector alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct IconPackHeader {
    pub ident: u32,
    pub version: u16,
    pub count: u16,
    pub cell_w: u8,
    pub cell_h: u8,
    pub record_stride: u8,
    pub flags: u8,
    pub base_id: u16,
    pub crc16: u16,
}

const _: () = {
    assert!(core::mem::size_of::<IconPackHeader>() == 16);
    assert!(core::mem::align_of::<IconPackHeader>() == 4);
};

impl IconPackHeader {
    pub const SIZE: usize = 16;

    #[inline(always)]
    pub const fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            ident: u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            version: u16::from_le_bytes([b[4], b[5]]),
            count: u16::from_le_bytes([b[6], b[7]]),
            cell_w: b[8],
            cell_h: b[9],
            record_stride: b[10],
            flags: b[11],
            base_id: u16::from_le_bytes([b[12], b[13]]),
            crc16: u16::from_le_bytes([b[14], b[15]]),
        }
    }

    /// Power-of-two shift `N` where `record_stride == 1 << N` (`8 -> 3`, `16 -> 4`, `32 -> 5`).
    #[inline(always)]
    pub const fn stride_shift(&self) -> Option<u32> {
        match self.record_stride {
            8 => Some(3),
            16 => Some(4),
            32 => Some(5),
            _ => None,
        }
    }

    #[inline(always)]
    pub const fn is_valid(&self) -> bool {
        self.ident == FILE_ID_ICON_PACK
            && self.cell_w > 0
            && self.cell_w <= 16
            && self.cell_h > 0
            && self.cell_h <= 16
            && self.stride_shift().is_some()
    }

    #[inline(always)]
    pub const fn is_var_width(&self) -> bool {
        (self.flags & ICON_PACK_FLAG_VAR_WIDTH) != 0
    }

    /// Compute $O(1)$ byte offset `16 + ((id - base_id) << N)` without division or multiplication.
    #[inline(always)]
    pub const fn record_offset(&self, id: u16) -> Option<usize> {
        if !self.is_valid() || id < self.base_id {
            return None;
        }
        let idx = id - self.base_id;
        if idx >= self.count {
            return None;
        }
        match self.stride_shift() {
            Some(shift) => Some(Self::SIZE + ((idx as usize) << shift)),
            None => None,
        }
    }
}

/// Draw a built-in fallback glyph (empty tofu box `□` or `?`) sized to `(w, h)`.
#[inline(never)]
pub fn draw_fallback_glyph(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    style: FallbackStyle,
    color: Color,
) {
    if w <= 5 && h <= 5 {
        let bmp = match style {
            FallbackStyle::EmptyBox => &FALLBACK_BOX_5,
            FallbackStyle::QuestionMark => &FALLBACK_QMARK_5,
        };
        draw::draw_bitmap(canvas, x, y, 5, 5, bmp, color);
    } else if w <= 8 && h <= 8 {
        let bmp = match style {
            FallbackStyle::EmptyBox => &FALLBACK_BOX_8,
            FallbackStyle::QuestionMark => FALLBACK_QMARK_8,
        };
        draw::draw_bitmap(canvas, x, y, 8, 8, bmp, color);
    } else {
        // 16x16 (or arbitrary >8px) crisp hollow box, with centered '?' when requested.
        let bw = w.max(8);
        let bh = h.max(8);
        canvas.draw_rect_i(x, y, bw as i16, bh as i16, color);
        if style == FallbackStyle::QuestionMark {
            let qx = x + ((bw as i16 - 5) >> 1);
            let qy = y + ((bh as i16 - 5) >> 1);
            draw::draw_bitmap(canvas, qx, qy, 5, 5, &FALLBACK_QMARK_5, color);
        }
    }
}

/// Draw an icon from an in-memory `.icn` pack slice; draws `fallback` if invalid or out of range.
/// Returns `true` if the icon was resolved from the pack, `false` if fallback was rendered.
#[inline(never)]
pub fn draw_pack_icon(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    pack: &[u8],
    id: u16,
    fallback_w: u8,
    fallback_h: u8,
    fallback: FallbackStyle,
    color: Color,
) -> bool {
    draw_pack_icon_streaming(
        canvas,
        x,
        y,
        |off, dst| {
            if off >= pack.len() {
                return 0;
            }
            let n = dst.len().min(pack.len() - off);
            dst[..n].copy_from_slice(&pack[off..off + n]);
            n
        },
        id,
        fallback_w,
        fallback_h,
        fallback,
        color,
    )
}

/// Zero-heap streaming `.icn` icon drawer using a pre-parsed [`IconPackHeader`].
/// Performs exactly 1 storage read (`16 + ((id - base_id) << N)`) per icon.
#[inline(never)]
pub fn draw_pack_icon_with_header<F>(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    hdr: &IconPackHeader,
    mut read_at: F,
    id: u16,
    fallback_w: u8,
    fallback_h: u8,
    fallback: FallbackStyle,
    color: Color,
) -> bool
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let Some(off) = hdr.record_offset(id) else {
        let fw = if hdr.is_valid() {
            hdr.cell_w
        } else {
            fallback_w
        };
        let fh = if hdr.is_valid() {
            hdr.cell_h
        } else {
            fallback_h
        };
        draw_fallback_glyph(canvas, x, y, fw, fh, fallback, color);
        return false;
    };

    let stride = hdr.record_stride as usize;
    let mut rec = [0u8; 32];
    if read_at(off, &mut rec[..stride]) != stride {
        draw_fallback_glyph(canvas, x, y, hdr.cell_w, hdr.cell_h, fallback, color);
        return false;
    }

    if hdr.is_var_width() {
        let w = rec[0].clamp(1, hdr.cell_w);
        draw::draw_bitmap(canvas, x, y, w, hdr.cell_h, &rec[1..stride], color);
    } else {
        draw::draw_bitmap(canvas, x, y, hdr.cell_w, hdr.cell_h, &rec[..stride], color);
    }
    true
}

/// Zero-heap streaming `.icn` icon drawer using a caller-provided `read_at(offset, buf) -> n`
/// reader (e.g. KernFS `read_file_at`). Uses only 32 bytes of stack storage.
#[inline(never)]
pub fn draw_pack_icon_streaming<F>(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    mut read_at: F,
    id: u16,
    fallback_w: u8,
    fallback_h: u8,
    fallback: FallbackStyle,
    color: Color,
) -> bool
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let mut hdr_buf = [0u8; IconPackHeader::SIZE];
    if read_at(0, &mut hdr_buf) != IconPackHeader::SIZE {
        draw_fallback_glyph(canvas, x, y, fallback_w, fallback_h, fallback, color);
        return false;
    }
    let hdr = IconPackHeader::from_bytes(&hdr_buf);
    draw_pack_icon_with_header(
        canvas, x, y, &hdr, read_at, id, fallback_w, fallback_h, fallback, color,
    )
}

// ── External Font Pack (`.fnt`, FourCC `FNT1`) ────────────────────────────────

/// Flag bit in [`FontGlyphEntry::bmp_off_hi`]: glyph has no bitmap payload (whitespace).
pub const FONT_GLYPH_FLAG_BLANK: u8 = 1 << 7;

/// 16-byte header for external filesystem font packs (`.fnt`, FourCC `"FNT1"`).
///
/// Supports ASCII, Latin-1/Extended, Cyrillic, Greek, GSM-7, Japanese (Kana + Kanji), Chinese
/// (Simplified + Traditional Hanzi), and any Unicode BMP script via a sorted 8-byte (`2^3`) glyph
/// index table (`16 + (idx << 3)`) followed immediately by the packed 1-bit bitmap pool (up to
/// 8 MB addressable via 23-bit per-glyph offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct FontPackHeader {
    pub ident: u32,
    pub version: u16,
    pub glyph_count: u16,
    pub line_height: u8,
    pub baseline: u8,
    pub encoding: u8,
    pub flags: u8,
    pub crc16: u16,
    pub reserved: u16,
}

/// Fixed 8-byte (`2^3`) glyph index entry sorted ascending by `codepoint`.
///
/// - `bmp_off_lo` + lower 7 bits of `bmp_off_hi` = 23-bit byte offset (up to 8 MB bitmap pool,
///   more than enough for full 8px–16px Japanese/Chinese CJK packs).
/// - Bit 7 of `bmp_off_hi` = `1` for blank/whitespace glyphs (0 bitmap bytes).
/// - `dim_wh`: for non-blank glyphs, high nibble = `width - 1` (`1..=16`), low nibble = `height - 1` (`1..=16`).
/// - `xy_offset`: high nibble = signed 4-bit `x_offset` (`-8..=7`), low nibble = signed 4-bit BDF `y_offset` (`-8..=7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct FontGlyphEntry {
    pub codepoint: u16,
    pub bmp_off_lo: u16,
    pub bmp_off_hi: u8,
    pub dim_wh: u8,
    pub x_advance: u8,
    pub xy_offset: u8,
}

const _: () = {
    assert!(core::mem::size_of::<FontPackHeader>() == 16);
    assert!(core::mem::align_of::<FontPackHeader>() == 4);
    assert!(core::mem::size_of::<FontGlyphEntry>() == 8);
    assert!(core::mem::align_of::<FontGlyphEntry>() == 2);
};

#[inline(always)]
const fn sext4(nibble: u8) -> i8 {
    ((nibble << 4) as i8) >> 4
}

impl FontPackHeader {
    pub const SIZE: usize = 16;

    #[inline(always)]
    pub const fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            ident: u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            version: u16::from_le_bytes([b[4], b[5]]),
            glyph_count: u16::from_le_bytes([b[6], b[7]]),
            line_height: b[8],
            baseline: b[9],
            encoding: b[10],
            flags: b[11],
            crc16: u16::from_le_bytes([b[12], b[13]]),
            reserved: u16::from_le_bytes([b[14], b[15]]),
        }
    }

    /// Deterministic byte offset where the packed 1-bit bitmap pool begins (`16 + (glyph_count << 3)`).
    #[inline(always)]
    pub const fn bitmap_pool_offset(&self) -> usize {
        Self::SIZE + ((self.glyph_count as usize) << 3)
    }

    #[inline(always)]
    pub const fn is_valid(&self) -> bool {
        self.ident == FILE_ID_FONT_PACK && self.line_height > 0
    }
}

impl FontGlyphEntry {
    pub const SIZE: usize = 8;

    #[inline(always)]
    pub const fn from_bytes(b: &[u8; 8]) -> Self {
        Self {
            codepoint: u16::from_le_bytes([b[0], b[1]]),
            bmp_off_lo: u16::from_le_bytes([b[2], b[3]]),
            bmp_off_hi: b[4],
            dim_wh: b[5],
            x_advance: b[6],
            xy_offset: b[7],
        }
    }

    #[inline(always)]
    pub const fn is_blank(&self) -> bool {
        (self.bmp_off_hi & FONT_GLYPH_FLAG_BLANK) != 0
    }

    /// 23-bit byte offset into the font's bitmap pool (supports up to 8 MB of CJK bitmaps).
    #[inline(always)]
    pub const fn bitmap_offset(&self) -> usize {
        (self.bmp_off_lo as usize) | (((self.bmp_off_hi & 0x7F) as usize) << 16)
    }

    #[inline(always)]
    pub const fn width(&self) -> u8 {
        if self.is_blank() {
            0
        } else {
            (self.dim_wh >> 4) + 1
        }
    }

    #[inline(always)]
    pub const fn height(&self) -> u8 {
        if self.is_blank() {
            0
        } else {
            (self.dim_wh & 0x0F) + 1
        }
    }

    #[inline(always)]
    pub const fn x_offset(&self) -> i8 {
        sext4(self.xy_offset >> 4)
    }

    #[inline(always)]
    pub const fn y_offset(&self) -> i8 {
        sext4(self.xy_offset & 0x0F)
    }

    /// Packed byte length of this glyph's MSB-first row bitmap (`((width + 7) >> 3) * height`).
    #[inline(always)]
    pub const fn bitmap_bytes(&self) -> usize {
        if self.is_blank() {
            0
        } else {
            (((self.width() as usize) + 7) >> 3) * (self.height() as usize)
        }
    }
}

/// Binary-search a `.fnt` pack for `codepoint` using a streaming `read_at` reader.
#[inline(never)]
pub fn lookup_font_glyph_streaming<F>(
    hdr: &FontPackHeader,
    mut read_at: F,
    codepoint: u16,
) -> Option<FontGlyphEntry>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    if !hdr.is_valid() || hdr.glyph_count == 0 {
        return None;
    }
    let mut lo = 0usize;
    let mut hi = hdr.glyph_count as usize;
    let mut buf = [0u8; 8];
    while lo < hi {
        let mid = (lo + hi) >> 1;
        let off = FontPackHeader::SIZE + (mid << 3);
        if read_at(off, &mut buf) != 8 {
            return None;
        }
        let entry = FontGlyphEntry::from_bytes(&buf);
        if entry.codepoint == codepoint {
            return Some(entry);
        } else if entry.codepoint < codepoint {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    None
}

/// Draw a 1-bit MSB-first row bitmap clipped horizontally to `[min_x, max_x)`.
#[inline(never)]
pub fn draw_bitmap_clipped(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    bitmap: &[u8],
    min_x: i16,
    max_x: i16,
    color: Color,
) {
    if x + (w as i16) <= min_x || x >= max_x {
        return;
    }
    let bytes_per_row = ((w as usize) + 7) >> 3;
    for gy in 0..h as usize {
        let py = y + gy as i16;
        if (py as u16) >= 64 {
            continue;
        }
        let row_off = gy * bytes_per_row;
        for gx in 0..w as usize {
            let px = x + gx as i16;
            if px < min_x || px >= max_x || (px as u16) >= 128 {
                continue;
            }
            let b_idx = row_off + (gx >> 3);
            if b_idx < bitmap.len() && ((bitmap[b_idx] >> (7 - (gx & 7))) & 1) != 0 {
                canvas.draw_pixel(px as u8, py as u8, color);
            }
        }
    }
}

/// Zero-heap direct-mapped SRAM glyph cache (`N` must be a power of two, e.g. `16` or `32`).
/// Eliminates repeated SPI Flash seeks when rendering UI labels across frames.
#[derive(Clone, Copy)]
pub struct CachedGlyphSlot {
    pub tag: u32, // (pack_id << 16) | codepoint
    pub entry: FontGlyphEntry,
    pub bmp: [u8; 32],
    pub valid: u8,
}

impl CachedGlyphSlot {
    pub const EMPTY: Self = Self {
        tag: 0,
        entry: FontGlyphEntry {
            codepoint: 0,
            bmp_off_lo: 0,
            bmp_off_hi: 0,
            dim_wh: 0,
            x_advance: 0,
            xy_offset: 0,
        },
        bmp: [0; 32],
        valid: 0,
    };
}

pub struct FontGlyphCache<const N: usize> {
    slots: [CachedGlyphSlot; N],
}

impl<const N: usize> FontGlyphCache<N> {
    pub const fn new() -> Self {
        assert!(
            N > 0 && (N & (N - 1)) == 0,
            "cache size must be a power of two"
        );
        Self {
            slots: [CachedGlyphSlot::EMPTY; N],
        }
    }

    pub fn clear(&mut self) {
        for s in &mut self.slots {
            s.valid = 0;
        }
    }

    #[inline(always)]
    fn slot_idx(pack_id: u8, cp: u16) -> usize {
        ((cp as usize).wrapping_add((pack_id as usize) * 31)) & (N - 1)
    }

    /// Look up or fetch a glyph from a streaming `.fnt` pack using a pre-parsed header.
    #[inline(never)]
    pub fn fetch<F>(
        &mut self,
        pack_id: u8,
        hdr: &FontPackHeader,
        mut read_at: F,
        codepoint: u16,
    ) -> Option<&CachedGlyphSlot>
    where
        F: FnMut(usize, &mut [u8]) -> usize,
    {
        let tag = ((pack_id as u32) << 16) | (codepoint as u32);
        let idx = Self::slot_idx(pack_id, codepoint);
        if self.slots[idx].valid != 0 && self.slots[idx].tag == tag {
            return Some(&self.slots[idx]);
        }
        let g = lookup_font_glyph_streaming(hdr, &mut read_at, codepoint)?;
        let nbytes = g.bitmap_bytes();
        if nbytes > 32 {
            return None;
        }
        let slot = &mut self.slots[idx];
        if nbytes > 0 {
            let off = hdr.bitmap_pool_offset() + g.bitmap_offset();
            if read_at(off, &mut slot.bmp[..nbytes]) != nbytes {
                slot.valid = 0;
                return None;
            }
        }
        slot.tag = tag;
        slot.entry = g;
        slot.valid = 1;
        Some(slot)
    }
}

/// Try rendering a BMP codepoint using a pre-parsed [`FontPackHeader`], clipped to `[min_x, max_x)`.
#[inline(never)]
pub fn try_draw_font_cp_with_header<F>(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    codepoint: u16,
    hdr: &FontPackHeader,
    mut read_at: F,
    min_x: i16,
    max_x: i16,
    color: Color,
) -> Option<i16>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let g = lookup_font_glyph_streaming(hdr, &mut read_at, codepoint)?;
    if g.is_blank() {
        return Some(g.x_advance as i16);
    }
    let nbytes = g.bitmap_bytes();
    if nbytes > 32 {
        return None;
    }
    let mut bmp = [0u8; 32];
    let off = hdr.bitmap_pool_offset() + g.bitmap_offset();
    if read_at(off, &mut bmp[..nbytes]) != nbytes {
        return None;
    }
    draw_bitmap_clipped(
        canvas,
        x + g.x_offset() as i16,
        y - g.height() as i16 - g.y_offset() as i16,
        g.width(),
        g.height(),
        &bmp[..nbytes],
        min_x,
        max_x,
        color,
    );
    Some(g.x_advance as i16)
}

/// Try rendering a BMP codepoint through a [`FontGlyphCache`] (0 Flash seeks on cache hit).
#[inline(never)]
pub fn try_draw_font_cp_cached<const N: usize, F>(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    codepoint: u16,
    pack_id: u8,
    hdr: &FontPackHeader,
    read_at: F,
    cache: &mut FontGlyphCache<N>,
    min_x: i16,
    max_x: i16,
    color: Color,
) -> Option<i16>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let slot = cache.fetch(pack_id, hdr, read_at, codepoint)?;
    let g = slot.entry;
    if !g.is_blank() {
        let nbytes = g.bitmap_bytes();
        draw_bitmap_clipped(
            canvas,
            x + g.x_offset() as i16,
            y - g.height() as i16 - g.y_offset() as i16,
            g.width(),
            g.height(),
            &slot.bmp[..nbytes],
            min_x,
            max_x,
            color,
        );
    }
    Some(g.x_advance as i16)
}

/// Try rendering a BMP codepoint from a single streaming `.fnt` pack; returns `Some(x_advance)` on
/// hit or `None` if the pack is absent/invalid or does not contain `codepoint`.
#[inline(never)]
pub fn try_draw_font_cp_streaming<F>(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    codepoint: u16,
    mut read_at: F,
    color: Color,
) -> Option<i16>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let mut hdr_buf = [0u8; FontPackHeader::SIZE];
    if read_at(0, &mut hdr_buf) != FontPackHeader::SIZE {
        return None;
    }
    let hdr = FontPackHeader::from_bytes(&hdr_buf);
    try_draw_font_cp_with_header(canvas, x, y, codepoint, &hdr, read_at, 0, 128, color)
}

/// Draw a single Unicode BMP character from a streaming `.fnt` font pack.
/// When the pack is missing/invalid or the codepoint is absent, draws the built-in tofu `□` or `?`
/// fallback and returns its horizontal advance.
#[inline(never)]
pub fn draw_font_char_streaming<F>(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    ch: char,
    mut read_at: F,
    fallback: FallbackStyle,
    color: Color,
) -> i16
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let cp_u32 = ch as u32;
    if cp_u32 <= u16::MAX as u32 {
        if let Some(adv) =
            try_draw_font_cp_streaming(canvas, x, y, cp_u32 as u16, &mut read_at, color)
        {
            return adv;
        }
    }
    draw_fallback_glyph(canvas, x, y - 5, 5, 5, fallback, color);
    6
}

#[inline(always)]
fn slice_reader(slice: &[u8]) -> impl FnMut(usize, &mut [u8]) -> usize + '_ {
    move |off, dst| {
        if off >= slice.len() {
            return 0;
        }
        let n = dst.len().min(slice.len() - off);
        dst[..n].copy_from_slice(&slice[off..off + n]);
        n
    }
}

/// Measure the horizontal advance of a single Unicode character across a `.fnt` fallback chain.
#[inline(never)]
pub fn font_char_width_chain(ch: char, font_packs: &[&[u8]]) -> i16 {
    let cp_u32 = ch as u32;
    if cp_u32 <= u16::MAX as u32 {
        let cp = cp_u32 as u16;
        for &pack in font_packs {
            if pack.len() < FontPackHeader::SIZE {
                continue;
            }
            let hdr = FontPackHeader::from_bytes(pack[..16].try_into().unwrap());
            if let Some(g) = lookup_font_glyph_streaming(&hdr, slice_reader(pack), cp) {
                return g.x_advance as i16;
            }
        }
    }
    6
}

/// Measure total pixel width of a UTF-8 string across a `.fnt` fallback chain.
#[inline(never)]
pub fn font_str_width_chain(text: &str, font_packs: &[&[u8]]) -> i16 {
    let mut w = 0i16;
    for ch in text.chars() {
        w += font_char_width_chain(ch, font_packs);
    }
    w
}

/// Draw a UTF-8 string scrolled horizontally by `scroll_x` and strictly clipped to `[min_x, max_x)`
/// across an ordered `.fnt` pack chain (supports live marquee scrolling for Latin/Cyrillic/CJK).
#[inline(never)]
pub fn draw_font_str_clipped_chain(
    canvas: &mut Canvas,
    min_x: i16,
    max_x: i16,
    scroll_x: i16,
    y: i16,
    text: &str,
    font_packs: &[&[u8]],
    fallback: FallbackStyle,
    color: Color,
) -> i16 {
    let mut x = min_x - scroll_x;
    for ch in text.chars() {
        let cp_u32 = ch as u32;
        let mut drawn = None;
        if cp_u32 <= u16::MAX as u32 {
            let cp = cp_u32 as u16;
            for &pack in font_packs {
                if pack.len() < FontPackHeader::SIZE {
                    continue;
                }
                let hdr = FontPackHeader::from_bytes(pack[..16].try_into().unwrap());
                if let Some(adv) = try_draw_font_cp_with_header(
                    canvas,
                    x,
                    y,
                    cp,
                    &hdr,
                    slice_reader(pack),
                    min_x,
                    max_x,
                    color,
                ) {
                    drawn = Some(adv);
                    break;
                }
            }
        }
        if let Some(adv) = drawn {
            x += adv;
        } else {
            let bmp = match fallback {
                FallbackStyle::EmptyBox => &FALLBACK_BOX_5,
                FallbackStyle::QuestionMark => &FALLBACK_QMARK_5,
            };
            draw_bitmap_clipped(canvas, x, y - 5, 5, 5, bmp, min_x, max_x, color);
            x += 6;
        }
        if x >= max_x {
            break;
        }
    }
    x
}

/// Draw a UTF-8 string clipped to `max_x` with `"…"` (`U+2026`, or `".."` fallback) when truncated.
#[inline(never)]
pub fn draw_font_str_ellipsis_chain(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    max_x: i16,
    text: &str,
    font_packs: &[&[u8]],
    fallback: FallbackStyle,
    color: Color,
) -> i16 {
    let avail = max_x - x;
    if avail <= 0 {
        return x;
    }
    if font_str_width_chain(text, font_packs) <= avail {
        return draw_font_str_clipped_chain(
            canvas, x, max_x, 0, y, text, font_packs, fallback, color,
        );
    }
    let ell_w = font_str_width_chain("..", font_packs);
    let body_max = (max_x - ell_w).max(x);
    let mut cur_x = x;
    let mut end_byte = 0usize;
    for (idx, ch) in text.char_indices() {
        let cw = font_char_width_chain(ch, font_packs);
        if cur_x + cw > body_max {
            break;
        }
        cur_x += cw;
        end_byte = idx + ch.len_utf8();
    }
    let body_end = draw_font_str_clipped_chain(
        canvas,
        x,
        body_max,
        0,
        y,
        &text[..end_byte],
        font_packs,
        fallback,
        color,
    );
    draw_font_str_clipped_chain(
        canvas, body_end, max_x, 0, y, "..", font_packs, fallback, color,
    )
}

/// Draw an aligned (`Left`/`Center`/`Right`) UTF-8 string across a `.fnt` pack chain.
#[inline(never)]
pub fn draw_font_str_align_chain(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    font_packs: &[&[u8]],
    align: crate::text::Align,
    fallback: FallbackStyle,
    color: Color,
) -> i16 {
    let w = font_str_width_chain(text, font_packs);
    let start_x = match align {
        crate::text::Align::Left => x,
        crate::text::Align::Center => x - (w >> 1),
        crate::text::Align::Right => x - w,
    };
    draw_font_str_clipped_chain(
        canvas, 0, 128, -start_x, y, text, font_packs, fallback, color,
    )
}

#[inline(always)]
const fn is_cjk_breakable(ch: char) -> bool {
    let cp = ch as u32;
    cp >= 0x2E80 && cp <= 0xFFFF
}

#[inline(always)]
const fn is_cjk_no_line_start(ch: char) -> bool {
    matches!(
        ch,
        '。' | '、' | '！' | '？' | '，' | '．' | '：' | '；' | '）' | '】' | '》' | '」' | '』'
    )
}

/// CJK-aware zero-allocation line wrapper returning `(line, remaining_rest)`.
/// Splits on ASCII whitespace for Latin/Cyrillic and at CJK glyph boundaries without needing spaces.
#[inline(never)]
pub fn next_wrapped_line_chain<'a>(
    text: &'a str,
    max_w: i16,
    font_packs: &[&[u8]],
) -> (&'a str, &'a str) {
    if text.is_empty() {
        return ("", "");
    }
    let mut w = 0i16;
    let mut last_break = None;
    let mut prev_cjk = false;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            return (&text[..idx], &text[idx + 1..]);
        }
        let cw = font_char_width_chain(ch, font_packs);
        if idx > 0 && w + cw > max_w {
            if let Some((brk_end, rest_start)) = last_break {
                return (&text[..brk_end], &text[rest_start..]);
            }
            return (&text[..idx], &text[idx..]);
        }
        w += cw;
        let next_idx = idx + ch.len_utf8();
        if ch == ' ' {
            last_break = Some((idx, next_idx));
            prev_cjk = false;
        } else if is_cjk_breakable(ch) {
            if prev_cjk && !is_cjk_no_line_start(ch) {
                last_break = Some((idx, idx));
            }
            prev_cjk = true;
            if next_idx < text.len() {
                let next_ch = text[next_idx..].chars().next().unwrap_or(' ');
                if !is_cjk_no_line_start(next_ch) {
                    last_break = Some((next_idx, next_idx));
                }
            }
        } else {
            prev_cjk = false;
        }
    }
    (text, "")
}

/// Draw a UTF-8 string across an ordered fallback chain of `.fnt` packs (e.g. `[tiny5, cjk_ja]`).
/// Each codepoint resolves from the first pack containing it (Latin/Cyrillic from base pack,
/// Kanji/Kana/Hanzi from optional CJK pack), falling back to built-in tofu `□` or `?` if absent.
#[inline(always)]
pub fn draw_font_str_chain(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    font_packs: &[&[u8]],
    fallback: FallbackStyle,
    color: Color,
) -> i16 {
    draw_font_str_clipped_chain(canvas, x, 128, 0, y, text, font_packs, fallback, color)
}

/// Draw a raw big-endian UCS-2 / UTF-16BE byte slice (standard DMR / SMS CJK & Cyrillic payload)
/// across an ordered `.fnt` pack chain with zero heap allocation and automatic tofu fallback.
#[inline(never)]
pub fn draw_font_ucs2_be_chain(
    canvas: &mut Canvas,
    mut x: i16,
    y: i16,
    ucs2_be: &[u8],
    font_packs: &[&[u8]],
    fallback: FallbackStyle,
    color: Color,
) -> i16 {
    let mut i = 0;
    while i + 1 < ucs2_be.len() {
        let cp = u16::from_be_bytes([ucs2_be[i], ucs2_be[i + 1]]);
        i += 2;
        let mut drawn = None;
        for &pack in font_packs {
            if let Some(adv) =
                try_draw_font_cp_streaming(canvas, x, y, cp, slice_reader(pack), color)
            {
                drawn = Some(adv);
                break;
            }
        }
        if let Some(adv) = drawn {
            x += adv;
        } else {
            draw_fallback_glyph(canvas, x, y - 5, 5, 5, fallback, color);
            x += 6;
        }
        if x >= 128 {
            break;
        }
    }
    x
}

/// Draw a UTF-8 string from an in-memory `.fnt` slice with automatic built-in tofu/question-mark
/// fallback for missing codepoints or missing/invalid font data.
#[inline(always)]
pub fn draw_font_str(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    font_pack: &[u8],
    fallback: FallbackStyle,
    color: Color,
) -> i16 {
    draw_font_str_chain(canvas, x, y, text, &[font_pack], fallback, color)
}

// ── Binary Language Pack (`.lng`, FourCC `LNG1`) ──────────────────────────────

/// Special metadata key storing the native display name (e.g. `"日本語"`, `"Русский"`).
pub const LANG_KEY_NATIVE_NAME: &str = "__LANG_NAME__";
/// Special metadata key storing an ASCII-safe picker label (e.g. `"Japanese (JA)"`, `"Russian (RU)"`).
pub const LANG_KEY_ASCII_NAME: &str = "__LANG_ASCII__";

/// Companion font hint bitflags in [`LangPackHeader::font_hint`].
pub const LANG_FONT_TINY5: u8 = 1 << 0;
pub const LANG_FONT_CJK_JA: u8 = 1 << 1;
pub const LANG_FONT_CJK_ZH: u8 = 1 << 2;

/// 16-byte naturally-aligned header for external binary language packs (`/user/lang/*.lng`).
///
/// Layout: `16 B` header + `entry_count * 8 B` sorted FNV-1a index (`offset = 16 + (mid << 3)`)
/// + packed NFC-normalized UTF-8 string pool. Zero MCU Flash overhead when languages are on SPI Flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LangPackHeader {
    pub ident: u32,
    pub version: u16,
    pub entry_count: u16,
    pub lang_code: [u8; 4],
    pub font_hint: u8,
    pub flags: u8,
    pub crc16: u16,
}

/// Fixed 8-byte (`2^3`) sorted translation index entry keyed by 32-bit FNV-1a hash of the English string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LangEntry {
    pub key_hash: u32,
    pub str_off_lo: u16,
    pub str_off_hi: u8,
    pub str_len: u8,
}

const _: () = {
    assert!(core::mem::size_of::<LangPackHeader>() == 16);
    assert!(core::mem::align_of::<LangPackHeader>() == 4);
    assert!(core::mem::size_of::<LangEntry>() == 8);
    assert!(core::mem::align_of::<LangEntry>() == 4);
};

/// Compact 32-bit FNV-1a hash for `LNG1` English source keys (zero lookup tables).
#[inline(always)]
pub const fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 32-bit FNV-1a hash for the natural compact/short variant of an English key (`key + 0x1F`).
#[inline(always)]
pub const fn fnv1a_32_short(bytes: &[u8]) -> u32 {
    (fnv1a_32(bytes) ^ 0x1F).wrapping_mul(0x0100_0193)
}

impl LangPackHeader {
    pub const SIZE: usize = 16;

    #[inline(always)]
    pub const fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            ident: u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            version: u16::from_le_bytes([b[4], b[5]]),
            entry_count: u16::from_le_bytes([b[6], b[7]]),
            lang_code: [b[8], b[9], b[10], b[11]],
            font_hint: b[12],
            flags: b[13],
            crc16: u16::from_le_bytes([b[14], b[15]]),
        }
    }

    #[inline(always)]
    pub const fn is_valid(&self) -> bool {
        self.ident == FILE_ID_LANG_PACK && self.version >= 0x0100
    }

    /// Deterministic byte offset where the packed UTF-8 string pool begins (`16 + (entry_count << 3)`).
    #[inline(always)]
    pub const fn string_pool_offset(&self) -> usize {
        Self::SIZE + ((self.entry_count as usize) << 3)
    }
}

impl LangEntry {
    pub const SIZE: usize = 8;

    #[inline(always)]
    pub const fn from_bytes(b: &[u8; 8]) -> Self {
        Self {
            key_hash: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            str_off_lo: u16::from_le_bytes([b[4], b[5]]),
            str_off_hi: b[6],
            str_len: b[7],
        }
    }

    #[inline(always)]
    pub const fn str_offset(&self) -> usize {
        (self.str_off_lo as usize) | ((self.str_off_hi as usize) << 16)
    }
}

/// Binary-search a `.lng` pack for `key_hash` using a streaming `read_at` reader.
#[inline(never)]
pub fn lookup_lang_entry_streaming<F>(
    hdr: &LangPackHeader,
    mut read_at: F,
    key_hash: u32,
) -> Option<LangEntry>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    if !hdr.is_valid() || hdr.entry_count == 0 {
        return None;
    }
    let mut lo = 0usize;
    let mut hi = hdr.entry_count as usize;
    let mut buf = [0u8; 8];
    while lo < hi {
        let mid = (lo + hi) >> 1;
        let off = LangPackHeader::SIZE + (mid << 3);
        if read_at(off, &mut buf) != 8 {
            return None;
        }
        let e = LangEntry::from_bytes(&buf);
        if e.key_hash == key_hash {
            return Some(e);
        } else if e.key_hash < key_hash {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    None
}

/// Minimal `#![no_std]` UTF-8 validator avoiding `core::str::from_utf8` table bloat on Cortex-M0+.
#[inline(never)]
fn utf8_str(bytes: &[u8]) -> Option<&str> {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let need = if b < 0x80 {
            1
        } else if (b & 0xE0) == 0xC0 && b >= 0xC2 {
            2
        } else if (b & 0xF0) == 0xE0 {
            3
        } else if (b & 0xF8) == 0xF0 && b <= 0xF4 {
            4
        } else {
            return None;
        };
        if i + need > bytes.len() {
            return None;
        }
        let mut k = 1;
        while k < need {
            if (bytes[i + k] & 0xC0) != 0x80 {
                return None;
            }
            k += 1;
        }
        i += need;
    }
    Some(unsafe { core::str::from_utf8_unchecked(bytes) })
}

/// Look up a translated UTF-8 string from a streaming `.lng` pack by precomputed `key_hash`.
#[inline(never)]
pub fn lookup_lang_hash_streaming<'a, F>(
    hdr: &LangPackHeader,
    mut read_at: F,
    key_hash: u32,
    out: &'a mut [u8],
) -> Option<&'a str>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let entry = lookup_lang_entry_streaming(hdr, &mut read_at, key_hash)?;
    let len = entry.str_len as usize;
    if len == 0 || len > out.len() {
        return None;
    }
    let off = hdr.string_pool_offset() + entry.str_offset();
    if read_at(off, &mut out[..len]) != len {
        return None;
    }
    utf8_str(&out[..len])
}

/// Look up a translated UTF-8 string from a streaming `.lng` pack into caller-owned `out`.
#[inline(always)]
pub fn lookup_lang_str_streaming<'a, F>(
    hdr: &LangPackHeader,
    read_at: F,
    english_key: &str,
    out: &'a mut [u8],
) -> Option<&'a str>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    lookup_lang_hash_streaming(hdr, read_at, fnv1a_32(english_key.as_bytes()), out)
}

/// Look up the natural compact/short variant of a label (e.g. `"初期化"` or `"Сброс"` for tight
/// UI slots), falling back automatically to the standard translation when no separate short form exists.
#[inline(never)]
pub fn lookup_lang_short_streaming<'a, F>(
    hdr: &LangPackHeader,
    mut read_at: F,
    english_key: &str,
    out: &'a mut [u8],
) -> Option<&'a str>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let short_h = fnv1a_32_short(english_key.as_bytes());
    let full_h = fnv1a_32(english_key.as_bytes());
    let entry = lookup_lang_entry_streaming(hdr, &mut read_at, short_h)
        .or_else(|| lookup_lang_entry_streaming(hdr, &mut read_at, full_h))?;
    let len = entry.str_len as usize;
    if len == 0 || len > out.len() {
        return None;
    }
    let off = hdr.string_pool_offset() + entry.str_offset();
    if read_at(off, &mut out[..len]) != len {
        return None;
    }
    utf8_str(&out[..len])
}

/// Look up a natural compact/short translation from an in-memory `.lng` pack slice.
#[inline(always)]
pub fn lookup_lang_short<'a>(pack: &[u8], english_key: &str, out: &'a mut [u8]) -> Option<&'a str> {
    if pack.len() < LangPackHeader::SIZE {
        return None;
    }
    let hdr = LangPackHeader::from_bytes(pack[..16].try_into().unwrap());
    lookup_lang_short_streaming(&hdr, slice_reader(pack), english_key, out)
}

/// Zero-overhead translator helper: returns `english_key` immediately when `hdr` is `None`
/// or the key is absent in the pack.
#[inline(always)]
pub fn tr_streaming<'a, F>(
    hdr: Option<&LangPackHeader>,
    read_at: F,
    english_key: &'a str,
    out: &'a mut [u8],
) -> &'a str
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    if let Some(h) = hdr {
        if let Some(s) = lookup_lang_str_streaming(h, read_at, english_key, out) {
            return s;
        }
    }
    english_key
}

/// Look up a translated UTF-8 string from an in-memory `.lng` pack slice.
#[inline(always)]
pub fn lookup_lang_str<'a>(pack: &[u8], english_key: &str, out: &'a mut [u8]) -> Option<&'a str> {
    if pack.len() < LangPackHeader::SIZE {
        return None;
    }
    let hdr = LangPackHeader::from_bytes(pack[..16].try_into().unwrap());
    lookup_lang_str_streaming(&hdr, slice_reader(pack), english_key, out)
}

/// Read the self-describing ASCII picker label (`__LANG_ASCII__`, falling back to `__LANG_NAME__`)
/// directly from a streaming `.lng` pack header+reader.
#[inline(never)]
pub fn read_lang_label_streaming<'a, F>(
    mut read_at: F,
    prefer_native: bool,
    out: &'a mut [u8],
) -> Option<&'a str>
where
    F: FnMut(usize, &mut [u8]) -> usize,
{
    let mut hdr_buf = [0u8; LangPackHeader::SIZE];
    if read_at(0, &mut hdr_buf) != LangPackHeader::SIZE {
        return None;
    }
    let hdr = LangPackHeader::from_bytes(&hdr_buf);
    if !hdr.is_valid() {
        return None;
    }
    let keys = if prefer_native {
        [LANG_KEY_NATIVE_NAME, LANG_KEY_ASCII_NAME]
    } else {
        [LANG_KEY_ASCII_NAME, LANG_KEY_NATIVE_NAME]
    };
    for &key in &keys {
        if let Some(entry) =
            lookup_lang_entry_streaming(&hdr, &mut read_at, fnv1a_32(key.as_bytes()))
        {
            let len = entry.str_len as usize;
            if len > 0 && len <= out.len() {
                let off = hdr.string_pool_offset() + entry.str_offset();
                if read_at(off, &mut out[..len]) == len {
                    return utf8_str(&out[..len]);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(c: &Canvas, x: u8, y: u8) -> bool {
        (c.buffer[(y >> 3) as usize][x as usize] & (1 << (y & 7))) != 0
    }

    fn make_icon_pack_8x8(base_id: u16, glyphs: &[[u8; 8]]) -> [u8; 48] {
        let mut buf = [0u8; 48];
        buf[0..4].copy_from_slice(b"ICN1");
        buf[4..6].copy_from_slice(&0x0100u16.to_le_bytes());
        buf[6..8].copy_from_slice(&(glyphs.len() as u16).to_le_bytes());
        buf[8] = 8;
        buf[9] = 8;
        buf[10] = 8;
        buf[11] = 0;
        buf[12..14].copy_from_slice(&base_id.to_le_bytes());
        for (i, g) in glyphs.iter().enumerate() {
            let off = 16 + (i << 3);
            buf[off..off + 8].copy_from_slice(g);
        }
        buf
    }

    #[test]
    fn icon_pack_lookup_and_missing_fallbacks() {
        let solid = [0xFFu8; 8];
        let cross = [0x81, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x81];
        let pack = make_icon_pack_8x8(200, &[solid, cross]);

        let hdr = IconPackHeader::from_bytes(pack[..16].try_into().unwrap());
        assert!(hdr.is_valid());
        assert_eq!(hdr.record_offset(200), Some(16));
        assert_eq!(hdr.record_offset(201), Some(24));
        assert_eq!(hdr.record_offset(199), None);
        assert_eq!(hdr.record_offset(202), None);

        let mut canvas = Canvas::new();
        assert!(draw_pack_icon(
            &mut canvas,
            0,
            0,
            &pack,
            201,
            8,
            8,
            FallbackStyle::EmptyBox,
            Color::Fill
        ));
        // Top-left and bottom-right corners of cross are lit
        assert!(px(&canvas, 0, 0));
        assert!(px(&canvas, 7, 7));
        assert!(!px(&canvas, 3, 0));

        // Missing ID renders built-in EmptyBox tofu instead of leaving blank space
        canvas.clear();
        assert!(!draw_pack_icon(
            &mut canvas,
            10,
            0,
            &pack,
            999,
            8,
            8,
            FallbackStyle::EmptyBox,
            Color::Fill
        ));
        assert!(px(&canvas, 10, 0));
        assert!(px(&canvas, 17, 6));
        assert!(!px(&canvas, 13, 3));

        // Missing pack (empty slice) renders built-in QuestionMark fallback
        canvas.clear();
        assert!(!draw_pack_icon(
            &mut canvas,
            20,
            0,
            &[],
            200,
            8,
            8,
            FallbackStyle::QuestionMark,
            Color::Fill
        ));
        assert!(px(&canvas, 20, 0));
        assert!(px(&canvas, 24, 5)); // '?' dot inside box
    }

    #[test]
    fn font_pack_binary_search_cjk_chain_ucs2_and_tofu_fallback() {
        // Build a tiny CJK add-on pack (8x8 'あ' U+3042 and '中' U+4E2D) to test multi-font chaining
        let mut cjk = [0u8; 64];
        cjk[0..4].copy_from_slice(b"FNT1");
        cjk[4..6].copy_from_slice(&0x0101u16.to_le_bytes());
        cjk[6..8].copy_from_slice(&2u16.to_le_bytes());
        cjk[8] = 9; // line_height
        cjk[9] = 7; // baseline
        cjk[10] = 3; // Unicode BMP
        cjk[11] = 1;

        // Entry 0: 'あ' (U+3042), 8x8 at bmp_off 0, x_adv=9
        cjk[16..18].copy_from_slice(&0x3042u16.to_le_bytes());
        cjk[18..20].copy_from_slice(&0u16.to_le_bytes());
        cjk[20] = 0; // bmp_off_hi = 0 (non-blank)
        cjk[21] = (7 << 4) | 7; // 8x8
        cjk[22] = 9;
        cjk[23] = 0;

        // Entry 1: '中' (U+4E2D), 8x8 at bmp_off 8, x_adv=9
        cjk[24..26].copy_from_slice(&0x4E2Du16.to_le_bytes());
        cjk[26..28].copy_from_slice(&8u16.to_le_bytes());
        cjk[28] = 0;
        cjk[29] = (7 << 4) | 7; // 8x8
        cjk[30] = 9;
        cjk[31] = 0;

        // 8x8 'あ' and '中' bitmaps
        cjk[32..40].copy_from_slice(&[0x10, 0x7E, 0x10, 0x3C, 0x52, 0x52, 0x2C, 0x00]);
        cjk[40..48].copy_from_slice(&[0x10, 0x10, 0xFE, 0x92, 0x92, 0xFE, 0x10, 0x10]);

        let tiny5 = include_bytes!("../../../assets/media/tiny5.fnt");
        let mut canvas = Canvas::new();

        // Mixed Latin + Cyrillic (from tiny5.fnt) + Japanese 'あ' + Chinese '中' (from cjk pack) + missing '漢' (tofu □)
        let end_x = draw_font_str_chain(
            &mut canvas,
            0,
            8,
            "AЖあ中漢",
            &[tiny5, &cjk],
            FallbackStyle::EmptyBox,
            Color::Fill,
        );
        assert!(end_x > 30);

        // UCS-2 BE payload (DMR / SMS): "Ж中" = [0x04, 0x16, 0x4E, 0x2D]
        canvas.clear();
        let ucs2_end = draw_font_ucs2_be_chain(
            &mut canvas,
            0,
            8,
            &[0x04, 0x16, 0x4E, 0x2D],
            &[tiny5, &cjk],
            FallbackStyle::EmptyBox,
            Color::Fill,
        );
        assert!(ucs2_end >= 14);

        // Missing font file (empty slice) gracefully draws tofu boxes without crashing
        canvas.clear();
        let end_missing = draw_font_str(
            &mut canvas,
            0,
            6,
            "HI",
            &[],
            FallbackStyle::EmptyBox,
            Color::Fill,
        );
        assert_eq!(end_missing, 12);
        assert!(px(&canvas, 0, 1));
        assert!(px(&canvas, 6, 1));
    }

    #[test]
    fn bundled_kern_kit_and_tiny5_packs_decode_cleanly() {
        let kit = include_bytes!("../../../assets/media/kern_kit.icn");
        let hdr = IconPackHeader::from_bytes(kit[..16].try_into().unwrap());
        assert!(hdr.is_valid());
        assert_eq!(hdr.count, 128);
        assert_eq!(hdr.record_stride, 8);
        assert_eq!(kit.len(), 16 + 128 * 8);

        let mut canvas = Canvas::new();
        assert!(draw_pack_icon(
            &mut canvas,
            0,
            0,
            kit,
            127, // HEART
            8,
            8,
            FallbackStyle::EmptyBox,
            Color::Fill
        ));
        assert!(px(&canvas, 3, 1)); // Heart body lit

        let apps16 = include_bytes!("../../../assets/media/kern_apps_16.icn");
        let ahdr = IconPackHeader::from_bytes(apps16[..16].try_into().unwrap());
        assert!(ahdr.is_valid());
        assert_eq!(ahdr.count, 16);
        assert_eq!(ahdr.cell_w, 16);
        assert_eq!(ahdr.cell_h, 16);
        assert_eq!(ahdr.record_stride, 32);
        assert_eq!(apps16.len(), 16 + 16 * 32);
        canvas.clear();
        assert!(draw_pack_icon(
            &mut canvas,
            0,
            0,
            apps16,
            0, // SPECTRUM
            16,
            16,
            FallbackStyle::EmptyBox,
            Color::Fill
        ));
        assert!(px(&canvas, 7, 1)); // Spectrum center peak

        let font = include_bytes!("../../../assets/media/tiny5.fnt");
        let fhdr = FontPackHeader::from_bytes(font[..16].try_into().unwrap());
        assert!(fhdr.is_valid());
        assert!(fhdr.glyph_count >= 3600);

        // Verify full Unicode BMP coverage: ASCII, accented Latin, Greek, Cyrillic, Braille, box-drawing
        for ch in ['A', 'é', 'Ñ', 'Δ', 'Ω', 'Ж', '€', '⣿', '┌', '★', '✓'] {
            let entry = lookup_font_glyph_streaming(
                &fhdr,
                |off, dst| {
                    let n = dst.len().min(font.len().saturating_sub(off));
                    dst[..n].copy_from_slice(&font[off..off + n]);
                    n
                },
                ch as u16,
            );
            assert!(entry.is_some(), "missing glyph for {ch:?}");
        }

        canvas.clear();
        let end_x = draw_font_str(
            &mut canvas,
            0,
            8,
            "┌KERN €★✓Ж┐",
            font,
            FallbackStyle::EmptyBox,
            Color::Fill,
        );
        assert!(end_x > 36);
    }

    #[test]
    fn glyph_cache_eliminates_flash_reads_and_cjk_wraps_cleanly() {
        let font = include_bytes!("../../../assets/media/tiny5.fnt");
        let hdr = FontPackHeader::from_bytes(font[..16].try_into().unwrap());
        let mut cache = FontGlyphCache::<16>::new();
        let reads = core::cell::Cell::new(0usize);
        let mut reader = |off: usize, dst: &mut [u8]| {
            reads.set(reads.get() + 1);
            let n = dst.len().min(font.len().saturating_sub(off));
            dst[..n].copy_from_slice(&font[off..off + n]);
            n
        };

        let mut canvas = Canvas::new();
        let a1 = try_draw_font_cp_cached(
            &mut canvas,
            0,
            8,
            'Ж' as u16,
            0,
            &hdr,
            &mut reader,
            &mut cache,
            0,
            128,
            Color::Fill,
        );
        assert!(a1.is_some());
        let after_first = reads.get();
        assert!(after_first > 0);

        // Second lookup hits SRAM cache: strictly 0 storage reads
        let a2 = try_draw_font_cp_cached(
            &mut canvas,
            10,
            8,
            'Ж' as u16,
            0,
            &hdr,
            &mut reader,
            &mut cache,
            0,
            128,
            Color::Fill,
        );
        assert_eq!(a1, a2);
        assert_eq!(reads.get(), after_first);

        // Clipped marquee scrolling stays strictly within [min_x, max_x)
        canvas.clear();
        let full_w = font_str_width_chain("Настройки радио", &[font]);
        assert!(full_w > 40);
        draw_font_str_clipped_chain(
            &mut canvas,
            10,
            40,
            8,
            8,
            "Настройки радио",
            &[font],
            FallbackStyle::EmptyBox,
            Color::Fill,
        );
        for x in 0..10u8 {
            for y in 0..16u8 {
                assert!(
                    !px(&canvas, x, y),
                    "marquee bled left of min_x at ({x},{y})"
                );
            }
        }
        for x in 40..128u8 {
            for y in 0..16u8 {
                assert!(
                    !px(&canvas, x, y),
                    "marquee bled right of max_x at ({x},{y})"
                );
            }
        }

        // CJK line wrapping splits without spaces and keeps punctuation attached
        let (l1, rest) = next_wrapped_line_chain("設定画面テスト。", 24, &[font]);
        assert!(!l1.is_empty());
        assert!(!rest.is_empty());
        assert!(!rest.starts_with('。'));
    }

    #[test]
    fn bundled_language_packs_lookup_labels_and_fallback() {
        let ru = include_bytes!("../../../assets/media/lang/ru.lng");
        let ja = include_bytes!("../../../assets/media/lang/ja.lng");
        let zh = include_bytes!("../../../assets/media/lang/zh.lng");
        let es = include_bytes!("../../../assets/media/lang/es.lng");

        let mut buf = [0u8; 48];
        assert_eq!(lookup_lang_str(ru, "Settings", &mut buf), Some("Настройки"));
        assert_eq!(lookup_lang_str(ja, "Language", &mut buf), Some("言語"));
        assert_eq!(lookup_lang_str(zh, "Contrast", &mut buf), Some("对比度"));
        assert_eq!(lookup_lang_str(es, "Battery", &mut buf), Some("Batería"));

        // Natural compact/short variants for tight UI slots + automatic fallback to full form
        assert_eq!(lookup_lang_short(ru, "Restart", &mut buf), Some("Рестарт"));
        assert_eq!(
            lookup_lang_short(ja, "Reset Settings", &mut buf),
            Some("初期化")
        );
        assert_eq!(
            lookup_lang_short(zh, "Backlight Time", &mut buf),
            Some("背光")
        );
        assert_eq!(
            lookup_lang_short(es, "Max Brightness", &mut buf),
            Some("Brillo +")
        );
        assert_eq!(lookup_lang_short(ja, "Language", &mut buf), Some("言語"));

        // Self-describing ASCII & native labels
        assert_eq!(
            read_lang_label_streaming(slice_reader(ja), false, &mut buf),
            Some("Japanese (JA)")
        );
        assert_eq!(
            read_lang_label_streaming(slice_reader(ja), true, &mut buf),
            Some("日本語")
        );

        // Zero-cost fallback to English when pack is absent or key is missing
        let missing = tr_streaming(None, slice_reader(&[]), "Contrast", &mut buf);
        assert_eq!(missing, "Contrast");
        let ru_hdr = LangPackHeader::from_bytes(ru[..16].try_into().unwrap());
        assert_eq!(
            tr_streaming(
                Some(&ru_hdr),
                slice_reader(ru),
                "Untranslated Key",
                &mut buf
            ),
            "Untranslated Key"
        );
    }
}
