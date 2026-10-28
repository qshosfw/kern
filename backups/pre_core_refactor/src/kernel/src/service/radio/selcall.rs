//! Selective Calling (Selcall / 5-Tone) Signaling System.
//!
//! Implements European and international emergency and selective calling standards:
//! ZVEI-1, ZVEI-2, CCIR, and EEA, including repeat-tone handling and Swiss REGA emergency protocols.

/// Standard selective calling tone system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelcallStandard {
    /// Zentralverband der Elektrotechnik- und Elektronikindustrie 1 (Germany / Central EU)
    Zvei1,
    /// ZVEI-2 (Variation used by Austrian/Swiss services and REGA)
    Zvei2,
    /// Comité Consultatif International des Radiocommunications
    Ccir,
    /// European Electronic Association
    Eea,
}

impl SelcallStandard {
    /// Return tone duration in milliseconds for this standard.
    pub const fn tone_duration_ms(&self) -> u16 {
        match self {
            Self::Zvei1 | Self::Zvei2 => 70,
            Self::Ccir | Self::Eea => 100,
        }
    }

    /// Lookup frequency in Hz for a digit (0..=15, where 14 is Repeat 'E', 10..=13 are A..D, 15 is F).
    pub const fn digit_freq_hz(&self, digit: u8) -> Option<u16> {
        if digit > 15 {
            return None;
        }
        let freq = match self {
            Self::Zvei1 => match digit {
                0 => 2400,
                1 => 1060,
                2 => 1160,
                3 => 1270,
                4 => 1400,
                5 => 1530,
                6 => 1670,
                7 => 1830,
                8 => 2000,
                9 => 2200,
                10 => 2800, // A
                11 => 810,  // B
                12 => 970,  // C
                13 => 886,  // D
                14 => 2600, // E (Repeat)
                15 => 2600, // F
                _ => return None,
            },
            Self::Zvei2 => match digit {
                0 => 2400,
                1 => 1060,
                2 => 1160,
                3 => 1270,
                4 => 1400,
                5 => 1530,
                6 => 1670,
                7 => 1830,
                8 => 2000,
                9 => 2200,
                10 => 885,  // A
                11 => 810,  // B
                12 => 740,  // C
                13 => 680,  // D
                14 => 970,  // E (Repeat)
                15 => 2600, // F
                _ => return None,
            },
            Self::Ccir => match digit {
                0 => 1981,
                1 => 1124,
                2 => 1197,
                3 => 1275,
                4 => 1358,
                5 => 1446,
                6 => 1540,
                7 => 1640,
                8 => 1747,
                9 => 1860,
                10 => 2400, // A
                11 => 930,  // B
                12 => 2247, // C
                13 => 991,  // D
                14 => 2110, // E (Repeat)
                15 => 1055, // F
                _ => return None,
            },
            Self::Eea => match digit {
                0 => 1981,
                1 => 1124,
                2 => 1197,
                3 => 1275,
                4 => 1358,
                5 => 1446,
                6 => 1540,
                7 => 1640,
                8 => 1747,
                9 => 1860,
                10 => 1055, // A
                11 => 930,  // B
                12 => 2247, // C
                13 => 991,  // D
                14 => 2110, // E (Repeat)
                15 => 2400, // F
                _ => return None,
            },
        };
        Some(freq)
    }

    /// Resolve a char ('0'..='9', 'A'..='F') to its numeric digit.
    pub const fn char_to_digit(c: char) -> Option<u8> {
        match c {
            '0'..='9' => Some(c as u8 - b'0'),
            'A' | 'a' => Some(10),
            'B' | 'b' => Some(11),
            'C' | 'c' => Some(12),
            'D' | 'd' => Some(13),
            'E' | 'e' => Some(14), // Repeat
            'F' | 'f' => Some(15),
            _ => None,
        }
    }
}

/// A pre-computed tone element in a Selcall burst sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelcallTone {
    pub freq_hz: u16,
    pub duration_ms: u16,
}

/// Fixed-capacity sequence of Selcall tones (up to 8 tones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelcallSequence {
    pub tones: [SelcallTone; 8],
    pub count: u8,
}

impl SelcallSequence {
    pub const fn empty() -> Self {
        Self {
            tones: [SelcallTone {
                freq_hz: 0,
                duration_ms: 0,
            }; 8],
            count: 0,
        }
    }

    /// Encodes a sequence of ASCII chars into tones, automatically inserting Repeat ('E')
    /// tones for consecutive duplicate digits.
    pub fn from_digits(standard: SelcallStandard, digits: &[u8]) -> Self {
        let mut seq = Self::empty();
        let tone_dur = standard.tone_duration_ms();
        let mut prev_digit: Option<u8> = None;

        for &raw_b in digits {
            if seq.count as usize >= seq.tones.len() {
                break;
            }
            let c = raw_b as char;
            if let Some(digit) = SelcallStandard::char_to_digit(c) {
                // Check if this digit is identical to the previous one
                let effective_digit = if Some(digit) == prev_digit {
                    14 // 'E' Repeat tone
                } else {
                    digit
                };

                if let Some(freq) = standard.digit_freq_hz(effective_digit) {
                    seq.tones[seq.count as usize] = SelcallTone {
                        freq_hz: freq,
                        duration_ms: tone_dur,
                    };
                    seq.count += 1;
                    prev_digit = Some(digit);
                }
            }
        }
        seq
    }

    /// Generate Swiss REGA Emergency 5-tone call sequence (ZVEI-2 "21414").
    pub fn rega_emergency() -> Self {
        Self::from_digits(SelcallStandard::Zvei2, b"21414")
    }
}
