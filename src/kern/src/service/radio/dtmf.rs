//! Dual-Tone Multi-Frequency (DTMF) Signaling System.
//!
//! Standard ITU-T Q.23 dual-tone matrix generator for selective calling,
//! repeater access, and telephone interconnect.

/// Standard DTMF Row frequencies (low group in Hz).
pub const DTMF_ROW_FREQS: [u16; 4] = [697, 770, 852, 941];

/// Standard DTMF Column frequencies (high group in Hz).
pub const DTMF_COL_FREQS: [u16; 4] = [1209, 1336, 1477, 1633];

/// A single dual-tone pair representing a DTMF character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DtmfTonePair {
    pub low_hz: u16,
    pub high_hz: u16,
}

impl DtmfTonePair {
    pub const fn new(low_hz: u16, high_hz: u16) -> Self {
        Self { low_hz, high_hz }
    }

    /// Resolve a character ('0'..='9', 'A'..='D', '*', '#') to its standard tone pair.
    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            '1' => Some(Self::new(DTMF_ROW_FREQS[0], DTMF_COL_FREQS[0])),
            '2' => Some(Self::new(DTMF_ROW_FREQS[0], DTMF_COL_FREQS[1])),
            '3' => Some(Self::new(DTMF_ROW_FREQS[0], DTMF_COL_FREQS[2])),
            'A' | 'a' => Some(Self::new(DTMF_ROW_FREQS[0], DTMF_COL_FREQS[3])),

            '4' => Some(Self::new(DTMF_ROW_FREQS[1], DTMF_COL_FREQS[0])),
            '5' => Some(Self::new(DTMF_ROW_FREQS[1], DTMF_COL_FREQS[1])),
            '6' => Some(Self::new(DTMF_ROW_FREQS[1], DTMF_COL_FREQS[2])),
            'B' | 'b' => Some(Self::new(DTMF_ROW_FREQS[1], DTMF_COL_FREQS[3])),

            '7' => Some(Self::new(DTMF_ROW_FREQS[2], DTMF_COL_FREQS[0])),
            '8' => Some(Self::new(DTMF_ROW_FREQS[2], DTMF_COL_FREQS[1])),
            '9' => Some(Self::new(DTMF_ROW_FREQS[2], DTMF_COL_FREQS[2])),
            'C' | 'c' => Some(Self::new(DTMF_ROW_FREQS[2], DTMF_COL_FREQS[3])),

            '*' => Some(Self::new(DTMF_ROW_FREQS[3], DTMF_COL_FREQS[0])),
            '0' => Some(Self::new(DTMF_ROW_FREQS[3], DTMF_COL_FREQS[1])),
            '#' => Some(Self::new(DTMF_ROW_FREQS[3], DTMF_COL_FREQS[2])),
            'D' | 'd' => Some(Self::new(DTMF_ROW_FREQS[3], DTMF_COL_FREQS[3])),

            _ => None,
        }
    }
}

/// Timing parameters for DTMF string burst transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DtmfTiming {
    pub tone_duration_ms: u16,
    pub pause_duration_ms: u16,
}

impl Default for DtmfTiming {
    fn default() -> Self {
        Self {
            tone_duration_ms: 60,
            pause_duration_ms: 40,
        }
    }
}
