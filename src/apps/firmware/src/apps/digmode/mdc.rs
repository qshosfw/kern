//! MDC1200 Digital Signaling & Roger Beep Generator.
//!
//! Provides pre-calculated and dynamically framed MDC-1200 bursts
//! (1200 baud MSK/FFSK) used across amateur and commercial radios.

/// Standard Motorola MDC1200 post-transmission Roger Beep frame (14 bytes / 7 words).
pub const MDC1200_ROGER_BURST: [u16; 7] = [0xF1A2, 0x7446, 0x61A4, 0x6544, 0x4E8A, 0xE044, 0xEA84];

/// Helper for MDC1200 packet creation and transmission.
pub struct Mdc1200;

impl Mdc1200 {
    /// Copies standard MDC1200 roger beep frame into a destination word buffer.
    pub fn get_roger_words(out_words: &mut [u16]) -> usize {
        let count = MDC1200_ROGER_BURST.len().min(out_words.len());
        out_words[..count].copy_from_slice(&MDC1200_ROGER_BURST[..count]);
        count
    }
}
