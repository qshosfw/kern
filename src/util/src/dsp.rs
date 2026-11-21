//! Lightweight Fixed-Point Baseband DSP Primitives (`#![no_std]`, Zero-Alloc).
//!
//! Provides integer Q14 Goertzel tone magnitude detectors and Bell 202 (1200/2200 Hz)
//! AFSK symbol correlators for decoding packet telemetry (AX.25/APRS, telemetry bursts,
//! multi-tone signaling) from `AudioCaptureService` 16-bit PCM samples on Cortex-M0+.

/// Precomputed Q14 Goertzel coefficient (`round(2 * cos(2*pi*f/fs) * 16384)`)
/// for Bell 202 Mark (1200 Hz) at 8000 Hz sample rate.
pub const COEFF_Q14_1200HZ_AT_8KHZ: i32 = 19261;

/// Precomputed Q14 Goertzel coefficient (`round(2 * cos(2*pi*f/fs) * 16384)`)
/// for Bell 202 Space (2200 Hz) at 8000 Hz sample rate.
pub const COEFF_Q14_2200HZ_AT_8KHZ: i32 = -5126;

/// Compute the Q14 Goertzel coefficient `2 * cos(2 * pi * target_hz / sample_rate_hz) * 16384`
/// using a pure integer parabolic/Bhaskara cosine approximation (`#![no_std]`, `const fn`).
pub const fn goertzel_coeff_q14(target_hz: u32, sample_rate_hz: u32) -> i32 {
    if sample_rate_hz == 0 {
        return 0;
    }
    // Normalized phase in [0, 32768) representing [0, 2*pi)
    let phase = (((target_hz % sample_rate_hz) as u64) * 32768 / (sample_rate_hz as u64)) as i32;
    // Map phase to half-wave [0, 16384] via symmetry cos(2*pi - x) = cos(x)
    let half = if phase > 16384 { 32768 - phase } else { phase };
    // Map to quarter-wave [0, 8192] with sign flip in second quadrant
    let (q, sign) = if half > 8192 {
        (16384 - half, -1i32)
    } else {
        (half, 1i32)
    };
    // Normalized x in [0, 8192] where 0 -> cos=1, 8192 -> cos=0.
    // Bhaskara / parabolic cosine fit scaled to Q14 (16384 = 1.0):
    // 2 * cos(x) in Q14 has peak 32768 at q=0 and 0 at q=8192.
    let q2 = (q as i64 * q as i64) >> 13; // in [0, 8192]
    let num = 8192i64 - q2;
    let den = 8192i64 + (q2 >> 2);
    let cos2_q14 = ((num * 32768) / den) as i32;
    cos2_q14 * sign
}

/// Fixed-point Q14 Goertzel single-bin tone energy detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoertzelFilter {
    coeff_q14: i32,
    s1: i32,
    s2: i32,
}

impl GoertzelFilter {
    /// Create a Goertzel filter with an explicit Q14 coefficient.
    #[inline(always)]
    pub const fn with_coeff_q14(coeff_q14: i32) -> Self {
        Self {
            coeff_q14,
            s1: 0,
            s2: 0,
        }
    }

    /// Create a Goertzel filter for `target_hz` at `sample_rate_hz`.
    #[inline(always)]
    pub const fn new(target_hz: u32, sample_rate_hz: u32) -> Self {
        Self::with_coeff_q14(goertzel_coeff_q14(target_hz, sample_rate_hz))
    }

    /// Reset internal delay state for a new sample block.
    #[inline(always)]
    pub fn reset(&mut self) {
        self.s1 = 0;
        self.s2 = 0;
    }

    /// Feed a single 16-bit signed PCM audio sample into the recurrence.
    #[inline(always)]
    pub fn push_sample(&mut self, sample: i16) {
        let x = (sample as i32) >> 6;
        let s0 = x + ((self.coeff_q14 * self.s1) >> 14) - self.s2;
        self.s2 = self.s1;
        self.s1 = s0;
    }

    /// Feed a slice of 16-bit PCM samples and return the squared magnitude energy.
    pub fn process_block(&mut self, samples: &[i16]) -> u32 {
        self.reset();
        for &s in samples {
            self.push_sample(s);
        }
        self.magnitude_squared()
    }

    /// Compute the non-negative squared magnitude power of the accumulated block strictly in 32-bit math.
    #[inline]
    pub fn magnitude_squared(&self) -> u32 {
        let mut s1 = self.s1;
        let mut s2 = self.s2;
        let mut shift = 0u32;
        while s1.unsigned_abs().max(s2.unsigned_abs()) >= 16384 {
            s1 >>= 1;
            s2 >>= 1;
            shift += 1;
        }
        let cross = ((self.coeff_q14 * s1) >> 14) * s2;
        let power = s1 * s1 + s2 * s2 - cross;
        if power <= 0 {
            0
        } else {
            let p = power as u32;
            let sh = shift << 1;
            if sh == 0 {
                p
            } else if sh >= 32 || p.leading_zeros() < sh {
                u32::MAX
            } else {
                p << sh
            }
        }
    }
}

/// Bell 202 (1200 Hz Mark / 2200 Hz Space) AFSK symbol correlator and NRZI bit slicer.
#[derive(Debug, Clone, Copy)]
pub struct Bell202Correlator {
    mark: GoertzelFilter,
    space: GoertzelFilter,
    prev_tone_is_mark: bool,
    ones_run: u8,
}

impl Bell202Correlator {
    /// Construct a Bell 202 correlator tuned for an 8000 Hz capture sample rate.
    pub const fn new_8khz() -> Self {
        Self {
            mark: GoertzelFilter::with_coeff_q14(COEFF_Q14_1200HZ_AT_8KHZ),
            space: GoertzelFilter::with_coeff_q14(COEFF_Q14_2200HZ_AT_8KHZ),
            prev_tone_is_mark: true,
            ones_run: 0,
        }
    }

    /// Construct a Bell 202 correlator for a custom sample rate in Hz.
    pub const fn new(sample_rate_hz: u32) -> Self {
        Self {
            mark: GoertzelFilter::new(1200, sample_rate_hz),
            space: GoertzelFilter::new(2200, sample_rate_hz),
            prev_tone_is_mark: true,
            ones_run: 0,
        }
    }

    /// Reset NRZI state.
    pub fn reset(&mut self) {
        self.mark.reset();
        self.space.reset();
        self.prev_tone_is_mark = true;
        self.ones_run = 0;
    }

    /// Evaluate a single symbol window of PCM samples (e.g. 6 or 7 samples at 8 kHz for 1200 baud)
    /// and return `(is_mark, mark_power, space_power)`.
    pub fn classify_symbol(&mut self, symbol_samples: &[i16]) -> (bool, u32, u32) {
        let m_pwr = self.mark.process_block(symbol_samples);
        let s_pwr = self.space.process_block(symbol_samples);
        (m_pwr >= s_pwr, m_pwr, s_pwr)
    }

    /// Decode one NRZI symbol window into an HDLC bit (`None` if this bit was a stuffed `0` after five `1`s).
    ///
    /// In AX.25 / Bell 202 NRZI:
    /// - A tone transition (Mark <-> Space) encodes a logical `0`.
    /// - No tone transition encodes a logical `1`.
    pub fn decode_nrzi_symbol(&mut self, symbol_samples: &[i16]) -> Option<bool> {
        let (is_mark, _, _) = self.classify_symbol(symbol_samples);
        let nrzi_bit = is_mark == self.prev_tone_is_mark;
        self.prev_tone_is_mark = is_mark;

        if nrzi_bit {
            self.ones_run = self.ones_run.saturating_add(1);
            Some(true)
        } else {
            let was_stuffed = self.ones_run == 5;
            self.ones_run = 0;
            if was_stuffed { None } else { Some(false) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goertzel_discriminates_1200_vs_2200_at_8khz() {
        // 16 samples of 1200 Hz sine wave at 8000 Hz sample rate
        let tone_1200: [i16; 16] = [
            0, 13255, 15582, 5063, -9638, -16384, -9638, 5063, 15582, 13255, 0, -13255, -15582,
            -5063, 9638, 16384,
        ];
        // 16 samples of 2200 Hz sine wave at 8000 Hz sample rate
        let tone_2200: [i16; 16] = [
            0, 16182, -5063, -14598, 9638, 11585, -13255, -7438, 15582, 2563, -16384, 2563, 15582,
            -7438, -13255, 11585,
        ];

        let mut corr = Bell202Correlator::new_8khz();
        let (is_mark_1200, m1, s1) = corr.classify_symbol(&tone_1200);
        assert!(
            is_mark_1200,
            "1200 Hz must classify as Mark (m={}, s={})",
            m1, s1
        );
        assert!(m1 > s1 * 4);

        let (is_mark_2200, m2, s2) = corr.classify_symbol(&tone_2200);
        assert!(
            !is_mark_2200,
            "2200 Hz must classify as Space (m={}, s={})",
            m2, s2
        );
        assert!(s2 > m2 * 4);
    }

    #[test]
    fn test_nrzi_and_bit_unstuffing() {
        let tone_1200: [i16; 8] = [0, 13255, 15582, 5063, -9638, -16384, -9638, 5063];
        let tone_2200: [i16; 8] = [0, 16182, -5063, -14598, 9638, 11585, -13255, -7438];

        let mut corr = Bell202Correlator::new_8khz();
        // Start on Mark; staying on Mark for 5 symbols produces five 1s
        for _ in 0..5 {
            assert_eq!(corr.decode_nrzi_symbol(&tone_1200), Some(true));
        }
        // A transition to Space right after five 1s is a stuffed 0 -> returns None
        assert_eq!(corr.decode_nrzi_symbol(&tone_2200), None);
        // Next transition back to Mark is a real 0 -> returns Some(false)
        assert_eq!(corr.decode_nrzi_symbol(&tone_1200), Some(false));
    }
}
