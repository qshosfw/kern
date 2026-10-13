//! International Morse Code (CW) Generator and Keyer.
//!
//! Provides text-to-Morse translation, standard Paris WPM timing calculation,
//! and `ToneNote` generation for CW transmissions and automated beacons.

use super::af::ToneNote;

/// Standard Morse timing based on Paris standard (50 dot lengths per word).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MorseTiming {
    pub dot_ms: u16,
    pub dash_ms: u16,
    pub symbol_space_ms: u16,
    pub char_space_ms: u16,
    pub word_space_ms: u16,
    pub pitch_hz: u16,
}

impl MorseTiming {
    /// Calculate timing parameters for a target Words-Per-Minute (WPM) speed and audio pitch.
    pub const fn from_wpm(wpm: u8, pitch_hz: u16) -> Self {
        let effective_wpm = if wpm == 0 { 1 } else { wpm };
        // Paris standard: dot length = 1200 / WPM ms
        let dot_ms = 1200 / effective_wpm as u16;
        Self {
            dot_ms,
            dash_ms: dot_ms * 3,
            symbol_space_ms: dot_ms,
            char_space_ms: dot_ms * 3,
            word_space_ms: dot_ms * 7,
            pitch_hz,
        }
    }
}

impl Default for MorseTiming {
    fn default() -> Self {
        Self::from_wpm(18, 700)
    }
}

/// Lookup Morse representation (dots '.' and dashes '-') for standard ASCII character.
pub const fn char_to_morse(c: char) -> Option<&'static str> {
    match c {
        'A' | 'a' => Some(".-"),
        'B' | 'b' => Some("-..."),
        'C' | 'c' => Some("-.-."),
        'D' | 'd' => Some("-.."),
        'E' | 'e' => Some("."),
        'F' | 'f' => Some("..-."),
        'G' | 'g' => Some("--."),
        'H' | 'h' => Some("...."),
        'I' | 'i' => Some(".."),
        'J' | 'j' => Some(".---"),
        'K' | 'k' => Some("-.-"),
        'L' | 'l' => Some(".-.."),
        'M' | 'm' => Some("--"),
        'N' | 'n' => Some("-."),
        'O' | 'o' => Some("---"),
        'P' | 'p' => Some(".--."),
        'Q' | 'q' => Some("--.-"),
        'R' | 'r' => Some(".-."),
        'S' | 's' => Some("..."),
        'T' | 't' => Some("-"),
        'U' | 'u' => Some("..-"),
        'V' | 'v' => Some("...-"),
        'W' | 'w' => Some(".--"),
        'X' | 'x' => Some("-..-"),
        'Y' | 'y' => Some("-.--"),
        'Z' | 'z' => Some("--.."),

        '0' => Some("-----"),
        '1' => Some(".----"),
        '2' => Some("..---"),
        '3' => Some("...--"),
        '4' => Some("....-"),
        '5' => Some("....."),
        '6' => Some("-...."),
        '7' => Some("--..."),
        '8' => Some("---.."),
        '9' => Some("----."),

        '/' => Some("-..-."),
        '?' => Some("..--.."),
        '.' => Some(".-.-.-"),
        ',' => Some("--..--"),
        '=' => Some("-...-"),
        '-' => Some("-....-"),
        _ => None,
    }
}

/// Maximum number of ToneNotes generated in a single batch.
pub const MORSE_BUFFER_CAPACITY: usize = 64;

/// Generates a stream or buffer of `ToneNote` elements from an ASCII text string.
pub struct MorseEncoder {
    pub timing: MorseTiming,
}

impl MorseEncoder {
    pub const fn new(timing: MorseTiming) -> Self {
        Self { timing }
    }

    /// Encode an ASCII string into an existing buffer of `ToneNote`s.
    /// Returns the number of notes written.
    pub fn encode_text(&self, text: &str, out: &mut [ToneNote]) -> usize {
        let mut idx = 0;
        let mut first_char = true;

        for ch in text.chars() {
            if ch == ' ' {
                if idx < out.len() {
                    out[idx] = ToneNote::new(0, self.timing.word_space_ms);
                    idx += 1;
                }
                first_char = true;
                continue;
            }

            if let Some(morse) = char_to_morse(ch) {
                // Inter-character space if not the first character of word
                if !first_char && idx < out.len() {
                    out[idx] = ToneNote::new(0, self.timing.char_space_ms);
                    idx += 1;
                }
                first_char = false;

                let mut first_symbol = true;
                for sym in morse.chars() {
                    if !first_symbol && idx < out.len() {
                        // Intra-character space between symbols
                        out[idx] = ToneNote::new(0, self.timing.symbol_space_ms);
                        idx += 1;
                    }
                    first_symbol = false;

                    let dur = if sym == '-' {
                        self.timing.dash_ms
                    } else {
                        self.timing.dot_ms
                    };

                    if idx < out.len() {
                        out[idx] = ToneNote::new(self.timing.pitch_hz, dur);
                        idx += 1;
                    } else {
                        return idx;
                    }
                }
            }
        }
        idx
    }
}
