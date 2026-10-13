//! Manchester Modulation Encoders, Decoders, and Keyfob Packet Generators.
//!
//! Implements bit-level Manchester phase encoding (`0` -> `01`, `1` -> `10`)
//! and keyfob packet frames inspired by `Nosen92/F4HWN-app-keyfob`.

/// Manchester encode a single byte into a 16-bit word (`0` -> `0b01`, `1` -> `0b10`, MSB first).
#[inline(always)]
pub const fn manchester_encode_byte(val: u8) -> u16 {
    let mut out: u16 = 0;
    let mut i = 0;
    while i < 8 {
        let bit = (val & (1 << (7 - i))) != 0;
        let pair: u16 = if bit { 0b10 } else { 0b01 };
        out = (out << 2) | pair;
        i += 1;
    }
    out
}

/// Manchester decode a 16-bit word back into a byte. Returns `None` on transition violations.
#[inline(always)]
pub const fn manchester_decode_word(val: u16) -> Option<u8> {
    let mut out: u8 = 0;
    let mut i = 0;
    while i < 8 {
        let shift = (7 - i) * 2;
        let pair = (val >> shift) & 0b11;
        match pair {
            0b01 => {}                   // 0 bit
            0b10 => out |= 1 << (7 - i), // 1 bit
            _ => return None,            // Inverted or transition violation
        }
        i += 1;
    }
    Some(out)
}

/// Manchester encodes a slice of bytes into an array of 16-bit words.
pub fn manchester_encode_slice(in_bytes: &[u8], out_words: &mut [u16]) -> usize {
    let count = in_bytes.len().min(out_words.len());
    for i in 0..count {
        out_words[i] = manchester_encode_byte(in_bytes[i]);
    }
    count
}

/// Manchester decodes a slice of 16-bit words into bytes.
pub fn manchester_decode_slice(in_words: &[u16], out_bytes: &mut [u8]) -> usize {
    let count = in_words.len().min(out_bytes.len());
    for i in 0..count {
        match manchester_decode_word(in_words[i]) {
            Some(b) => out_bytes[i] = b,
            None => return i,
        }
    }
    count
}

/// Manchester FSK Keyfob Encoder (from `F4HWN-app-keyfob`).
/// Encodes a 64-bit code and its bitwise complement into 256 bits (16 words of 16-bit).
pub struct ManchesterEncoder;

impl ManchesterEncoder {
    /// Encode 64-bit code into 16 words of 16-bit using standard Manchester encoding
    /// (`0` -> `01`, `1` -> `10`, MSB first). First 128 bits = code, next 128 bits = complement.
    pub fn encode_keyfob_packet(code: u64, out_words: &mut [u16; 16]) {
        let complement = !code;
        let mut bit_pos = 0;

        // Helper closure to push a bit pair
        let mut push_manchester = |bit: bool, pos: &mut usize| {
            let word_idx = *pos / 16;
            let bit_in_word = 14 - (*pos % 16); // Write MSB to LSB in pairs
            let pair: u16 = if bit { 0b10 } else { 0b01 };
            out_words[word_idx] |= pair << bit_in_word;
            *pos += 2;
        };

        // First 64 bits of code
        for i in (0..64).rev() {
            let bit = (code & (1 << i)) != 0;
            push_manchester(bit, &mut bit_pos);
        }

        // Next 64 bits of complement
        for i in (0..64).rev() {
            let bit = (complement & (1 << i)) != 0;
            push_manchester(bit, &mut bit_pos);
        }
    }
}
