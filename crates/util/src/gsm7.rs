//! GSM 7-bit Default & Extended Alphabet (3GPP TS 23.038).
//!
//! Provides ultra-compact 7-to-8 bit packing (saves 12.5% storage/payload space),
//! character mapping, and text formatting tag parsing with zero allocations.

pub const ESCAPE: u8 = 0x1B;

/// Text Formatting Tags (toggled by extended codes 0x00..0x03)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gsm7Format {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
}

impl Gsm7Format {
    pub const fn new() -> Self {
        Self {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
        }
    }

    /// Toggles a formatting tag if `ext_code` is 0x00..0x03. Returns true if handled.
    pub fn toggle(&mut self, ext_code: u8) -> bool {
        match ext_code {
            0x00 => {
                self.bold = !self.bold;
                true
            }
            0x01 => {
                self.italic = !self.italic;
                true
            }
            0x02 => {
                self.strikethrough = !self.strikethrough;
                true
            }
            0x03 => {
                self.underline = !self.underline;
                true
            }
            _ => false,
        }
    }

    /// Reset all formatting tags to normal
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Pack an array of 7-bit septets into 8-bit octets (7 bytes per 8 characters).
/// Returns the number of bytes written to `out`.
pub fn pack_septets(septets: &[u8], out: &mut [u8]) -> usize {
    let mut out_idx = 0;
    let mut bit_buf = 0u16;
    let mut bits_in_buf = 0;

    for &s in septets {
        bit_buf |= ((s as u16) & 0x7F) << bits_in_buf;
        bits_in_buf += 7;

        while bits_in_buf >= 8 {
            if out_idx < out.len() {
                out[out_idx] = (bit_buf & 0xFF) as u8;
                out_idx += 1;
            }
            bit_buf >>= 8;
            bits_in_buf -= 8;
        }
    }

    if bits_in_buf > 0 && out_idx < out.len() {
        out[out_idx] = (bit_buf & 0xFF) as u8;
        out_idx += 1;
    }

    out_idx
}

/// Unpack 8-bit octets back into 7-bit septets.
/// Unpacks up to `num_septets` characters into `out`.
/// Returns the number of septets unpacked.
pub fn unpack_septets(octets: &[u8], num_septets: usize, out: &mut [u8]) -> usize {
    let mut out_idx = 0;
    let mut bit_buf = 0u16;
    let mut bits_in_buf = 0;
    let mut in_idx = 0;

    while out_idx < num_septets && out_idx < out.len() {
        while bits_in_buf < 7 {
            if in_idx < octets.len() {
                bit_buf |= (octets[in_idx] as u16) << bits_in_buf;
                in_idx += 1;
                bits_in_buf += 8;
            } else {
                break;
            }
        }

        if bits_in_buf >= 7 {
            out[out_idx] = (bit_buf & 0x7F) as u8;
            out_idx += 1;
            bit_buf >>= 7;
            bits_in_buf -= 7;
        } else {
            break;
        }
    }

    out_idx
}

/// Decode a basic GSM 7-bit character (not escaped) into a Unicode char.
pub fn decode_basic(code: u8) -> Option<char> {
    if code > 0x7F {
        return None;
    }

    // Direct ASCII mapping for 0x20..0x7F with exceptions
    match code {
        // 0x00..0x0F
        0x00 => Some('@'),
        0x01 => Some('£'),
        0x02 => Some('$'),
        0x03 => Some('¥'),
        0x04 => Some('è'),
        0x05 => Some('é'),
        0x06 => Some('ù'),
        0x07 => Some('ì'),
        0x08 => Some('ò'),
        0x09 => Some('Ç'),
        0x0A => Some('\n'),
        0x0B => Some('Ø'),
        0x0C => Some('ø'),
        0x0D => Some('\r'),
        0x0E => Some('Å'),
        0x0F => Some('å'),

        // 0x10..0x1F (Greek & special)
        0x10 => Some('Δ'),
        0x11 => Some('_'),
        0x12 => Some('Φ'),
        0x13 => Some('Γ'),
        0x14 => Some('Λ'),
        0x15 => Some('Ω'),
        0x16 => Some('Π'),
        0x17 => Some('Ψ'),
        0x18 => Some('Σ'),
        0x19 => Some('Θ'),
        0x1A => Some('Ξ'),
        0x1B => None, // ESC prefix
        0x1C => Some('Æ'),
        0x1D => Some('æ'),
        0x1E => Some('ß'),
        0x1F => Some('É'),

        // 0x20..0x3F (Standard ASCII except 0x24)
        0x24 => Some('¤'),
        0x20..=0x3F => Some(code as char),

        // 0x40..0x5F (Standard ASCII uppercase except accents)
        0x40 => Some('¡'),
        0x41..=0x5A => Some(code as char),
        0x5B => Some('Ä'),
        0x5C => Some('Ö'),
        0x5D => Some('Ñ'),
        0x5E => Some('Ü'),
        0x5F => Some('§'),

        // 0x60..0x7F (Standard ASCII lowercase except accents)
        0x60 => Some('¿'),
        0x61..=0x7A => Some(code as char),
        0x7B => Some('ä'),
        0x7C => Some('ö'),
        0x7D => Some('ñ'),
        0x7E => Some('ü'),
        0x7F => Some('à'),

        _ => None,
    }
}

/// Decode an extended GSM 7-bit character (preceded by ESC 0x1B) into a Unicode char.
pub fn decode_extended(code: u8) -> Option<char> {
    match code {
        0x0A => Some('\x0C'), // FF
        0x14 => Some('^'),
        0x28 => Some('{'),
        0x29 => Some('}'),
        0x2C => Some('['),
        0x2F => Some('\\'),
        0x30 => Some('¶'),
        0x31 => Some('™'),
        0x32 => Some('®'),
        0x33 => Some('©'),
        0x38 => Some('«'),
        0x39 => Some('»'),
        0x3D => Some('~'),
        0x3E => Some(']'),
        0x40 => Some('│'),
        0x41 => Some('←'),
        0x42 => Some('→'),
        0x43 => Some('↑'),
        0x44 => Some('↓'),
        0x46 => Some('◀'),
        0x47 => Some('▶'),
        0x50 => Some('░'),
        0x51 => Some('▒'),
        0x52 => Some('█'),
        0x53 => Some('▄'),
        0x54 => Some('▌'),
        0x55 => Some('▐'),
        0x56 => Some('─'),
        0x57 => Some('┌'),
        0x58 => Some('┐'),
        0x59 => Some('└'),
        0x5A => Some('┘'),
        0x5B => Some('├'),
        0x5C => Some('┤'),
        0x5D => Some('┬'),
        0x5E => Some('┴'),
        0x5F => Some('┼'),
        0x60 => Some('■'),
        0x61 => Some('□'),
        0x62 => Some('●'),
        0x63 => Some('○'),
        0x64 => Some('▲'),
        0x65 => Some('△'),
        0x66 => Some('▼'),
        0x67 => Some('▽'),
        0x68 => Some('◆'),
        0x69 => Some('◇'),
        0x6A => Some('★'),
        0x6B => Some('☆'),
        0x6C => Some('☺'),
        0x6D => Some('☻'),
        0x6E => Some('♥'),
        0x6F => Some('🌡'),
        0x70 => Some('✓'),
        0x71 => Some('✗'),
        0x72 => Some('✉'),
        0x75 => Some('€'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gsm7_format_toggle() {
        let mut fmt = Gsm7Format::new();
        assert!(!fmt.bold);
        assert!(fmt.toggle(0x00));
        assert!(fmt.bold);
        assert!(fmt.toggle(0x00));
        assert!(!fmt.bold);

        assert!(fmt.toggle(0x01));
        assert!(fmt.italic);
        assert!(!fmt.toggle(0x05)); // Invalid formatting tag
    }

    #[test]
    fn test_pack_unpack_septets_roundtrip() {
        let original = [
            b'H' & 0x7F,
            b'e' & 0x7F,
            b'l' & 0x7F,
            b'l' & 0x7F,
            b'o' & 0x7F,
            0x00,
            0x10,
            0x75,
        ];
        let mut packed = [0u8; 7];
        let packed_len = pack_septets(&original, &mut packed);
        assert_eq!(packed_len, 7);

        let mut unpacked = [0u8; 8];
        let unpacked_len = unpack_septets(&packed, 8, &mut unpacked);
        assert_eq!(unpacked_len, 8);
        assert_eq!(unpacked, original);
    }

    #[test]
    fn test_decoding_basic_and_extended() {
        assert_eq!(decode_basic(0x00), Some('@'));
        assert_eq!(decode_basic(0x10), Some('Δ'));
        assert_eq!(decode_basic(0x24), Some('¤'));
        assert_eq!(decode_basic(b'A'), Some('A'));
        assert_eq!(decode_extended(0x75), Some('€'));
        assert_eq!(decode_extended(0x14), Some('^'));
        assert_eq!(decode_extended(0x41), Some('←'));
    }
}
