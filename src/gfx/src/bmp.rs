//! Windows 1-Bit Monochrome Bitmap (BMP) Encoder & Decoder.
//!
//! Provides zero-RAM streaming screenshot serialization and rendering
//! for 128×64 LCD displays adhering to the 1086-byte Windows BMP specification.

use crate::Canvas;

/// Exact 62-byte Windows 1-bit BMP header for 128×64 monochrome displays.
///
/// Total file size: 62 (header) + 1024 (pixel array) = 1086 bytes.
pub const BMP_HEADER_128X64: [u8; 62] = [
    // BITMAPFILEHEADER (14 bytes)
    0x42, 0x4D, // bfType: 'BM'
    0x3E, 0x04, 0x00, 0x00, // bfSize: 1086 bytes
    0x00, 0x00, // bfReserved1: 0
    0x00, 0x00, // bfReserved2: 0
    0x3E, 0x00, 0x00, 0x00, // bfOffBits: 62 bytes (offset to pixel array)
    // BITMAPINFOHEADER (40 bytes)
    0x28, 0x00, 0x00, 0x00, // biSize: 40 bytes
    0x80, 0x00, 0x00, 0x00, // biWidth: 128 pixels
    0x40, 0x00, 0x00, 0x00, // biHeight: 64 pixels (positive = bottom-up)
    0x01, 0x00, // biPlanes: 1 plane
    0x01, 0x00, // biBitCount: 1 bit per pixel
    0x00, 0x00, 0x00, 0x00, // biCompression: BI_RGB (uncompressed)
    0x00, 0x04, 0x00, 0x00, // biSizeImage: 1024 bytes
    0x13, 0x0B, 0x00, 0x00, // biXPelsPerMeter: 2835 (~72 DPI)
    0x13, 0x0B, 0x00, 0x00, // biYPelsPerMeter: 2835 (~72 DPI)
    0x02, 0x00, 0x00, 0x00, // biClrUsed: 2 colors in palette
    0x02, 0x00, 0x00, 0x00, // biClrImportant: 2 colors
    // RGBQUAD Palette (8 bytes)
    0xFF, 0xFF, 0xFF, 0x00, // Palette[0]: White (RGB 255, 255, 255) -> bit 0 (clear pixel)
    0x00, 0x00, 0x00, 0x00, // Palette[1]: Black (RGB 0, 0, 0)       -> bit 1 (lit pixel)
];

pub const BMP_TOTAL_FILE_SIZE: usize = 1086;
pub const BMP_HEADER_SIZE: usize = 62;
pub const BMP_ROW_BYTES: usize = 16;
pub const BMP_TOTAL_ROWS: usize = 64;

/// Validate whether a byte slice begins with a valid 128×64 1-bit BMP header.
pub fn is_valid_128x64_bmp(header: &[u8]) -> bool {
    if header.len() < BMP_HEADER_SIZE {
        return false;
    }
    // Check 'BM' signature
    if header[0] != 0x42 || header[1] != 0x4D {
        return false;
    }
    // Check width = 128 (0x00000080)
    if header[18] != 0x80 || header[19] != 0x00 || header[20] != 0x00 || header[21] != 0x00 {
        return false;
    }
    // Check height = 64 (0x00000040)
    if header[22] != 0x40 || header[23] != 0x00 || header[24] != 0x00 || header[25] != 0x00 {
        return false;
    }
    // Check bit depth = 1 bpp (0x0001)
    if header[28] != 0x01 || header[29] != 0x00 {
        return false;
    }
    true
}

/// Encode a single horizontal scanline (0..63) from the canvas into 16 BMP bytes.
///
/// MSB (bit 7) is the leftmost pixel (x=0); LSB (bit 0) is the rightmost (x=7).
pub fn encode_canvas_row(canvas: &Canvas, y: u8) -> [u8; BMP_ROW_BYTES] {
    let mut row = [0u8; BMP_ROW_BYTES];
    if y >= 64 {
        return row;
    }

    let page = (y >> 3) as usize;
    let bit_mask = 1 << (y & 7);

    for byte_idx in 0..BMP_ROW_BYTES {
        let mut b = 0u8;
        let base_x = byte_idx * 8;
        for bit in 0..8 {
            let x = base_x + bit;
            if (canvas.buffer[page][x] & bit_mask) != 0 {
                b |= 1 << (7 - bit);
            }
        }
        row[byte_idx] = b;
    }

    row
}

/// Decode a 16-byte BMP row into a scanline (0..63) in the canvas.
pub fn decode_bmp_row(canvas: &mut Canvas, y: u8, row: &[u8; BMP_ROW_BYTES]) {
    if y >= 64 {
        return;
    }

    let page = (y >> 3) as usize;
    let bit_pos = y & 7;

    for byte_idx in 0..BMP_ROW_BYTES {
        let b = row[byte_idx];
        let base_x = byte_idx * 8;
        for bit in 0..8 {
            let x = base_x + bit;
            if (b & (1 << (7 - bit))) != 0 {
                canvas.buffer[page][x] |= 1 << bit_pos;
            } else {
                canvas.buffer[page][x] &= !(1 << bit_pos);
            }
        }
    }

    canvas.mark_span(page, 0, 127);
}

/// Zero-RAM streaming encoder: streams BMP header + 64 rows (bottom-up) to a sink callback.
///
/// Consumes only 16 bytes of stack space for the transient row buffer.
pub fn stream_canvas_to_bmp<F: FnMut(&[u8]) -> Result<(), E>, E>(
    canvas: &Canvas,
    mut write_chunk: F,
) -> Result<(), E> {
    write_chunk(&BMP_HEADER_128X64)?;

    // BMP format mandates bottom-to-top scanline ordering: y = 63 down to 0
    for y in (0..64).rev() {
        let row = encode_canvas_row(canvas, y as u8);
        write_chunk(&row)?;
    }

    Ok(())
}
