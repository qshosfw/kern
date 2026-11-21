//! Automatic Gain Control (AGC), Front-End Attenuation, and AM Linearity Mapping.

/// Listening mode presets for AGC tuning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgcListeningMode {
    Fm,
    AmListening,
    Ssb,
}

/// Dynamic AGC attack and decay speed setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgcSpeed {
    Slow,
    Medium,
    Fast,
    /// Ultra-fast microsecond-response AGC loop for coherent SSB/CW demodulation.
    SsbFast,
}

/// Standard AGC tables from GoguFW and Fagci-K1.
pub const AGC_DEFAULT_TABLE: [u16; 5] = [0x0000, 0x1020, 0x2040, 0x3060, 0x4080];

/// 43-entry calibrated front-end gain table `(reg_13_val, gain_db)` for linearizing
/// high-modulation-depth AM signals (`-93 dB` down to `0 dB`).
pub static AM_LINEARITY_GAIN_TABLE: [(u16, i8); 43] = [
    (0x03BE, -7),  // 0: original base reference
    (0x0000, -93), // 1
    (0x0008, -91), // 2
    (0x0010, -88), // 3
    (0x0001, -87), // 4
    (0x0009, -85), // 5
    (0x0011, -82), // 6
    (0x0002, -81), // 7
    (0x000A, -79), // 8
    (0x0012, -76), // 9
    (0x0003, -75), // 10
    (0x000B, -73), // 11
    (0x0013, -70), // 12
    (0x0004, -69), // 13
    (0x000C, -67), // 14
    (0x0014, -64), // 15
    (0x0084, -63), // 16
    (0x008C, -61), // 17
    (0x0094, -58), // 18
    (0x0104, -57), // 19
    (0x010C, -55), // 20
    (0x0114, -52), // 21
    (0x0184, -50), // 22
    (0x018C, -48), // 23
    (0x0194, -45), // 24
    (0x0204, -44), // 25
    (0x020C, -42), // 26
    (0x0214, -39), // 27
    (0x0284, -36), // 28
    (0x028C, -34), // 29
    (0x0294, -31), // 30
    (0x0304, -29), // 31
    (0x030C, -27), // 32
    (0x0314, -24), // 33
    (0x0384, -21), // 34
    (0x038C, -19), // 35
    (0x0394, -16), // 36
    (0x038D, -13), // 37
    (0x0395, -10), // 38
    (0x038E, -7),  // 39
    (0x0396, -4),  // 40
    (0x038F, -1),  // 41
    (0x0397, 0),   // 42: max gain
];

/// Dynamic AM Linearity Tracker.
///
/// Adjusts front-end gain inversely to carrier RSSI so the internal IF/ADC operates near
/// its sweet spot (`-89 dBm`), preventing envelope detector saturation clipping on strong
/// AM aviation signals while maintaining fast attack and 300ms release hysteresis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmLinearityTracker {
    pub gain_idx: usize,
    pub prev_rssi_half_db: i16,
    pub hold_ticks_10ms: u8,
    pub enabled: bool,
}

impl AmLinearityTracker {
    /// Target IF/ADC operating point in half-dB steps above -160 dBm (`(-89 + 160) * 2 = 142`).
    pub const DESIRED_RSSI_HALF_DB: i16 = (-89 + 160) * 2;

    pub const fn new() -> Self {
        Self {
            gain_idx: 0,
            prev_rssi_half_db: 0,
            hold_ticks_10ms: 0,
            enabled: true,
        }
    }

    pub fn reset(&mut self) {
        self.gain_idx = 0;
        self.prev_rssi_half_db = 0;
        self.hold_ticks_10ms = 0;
    }

    /// Evaluate a 10ms tick given the current calibrated `rssi_dbm` and return `Some(reg_13_val)`
    /// if the front-end gain register should be updated.
    pub fn update_10ms(&mut self, rssi_dbm: i16) -> Option<u16> {
        if !self.enabled {
            return None;
        }

        let rssi_half_db = (rssi_dbm + 160).max(0) * 2;
        let smoothed = if self.prev_rssi_half_db > 0 {
            (self.prev_rssi_half_db + rssi_half_db) / 2
        } else {
            rssi_half_db
        };
        self.prev_rssi_half_db = rssi_half_db;

        if self.hold_ticks_10ms > 0 {
            self.hold_ticks_10ms -= 1;
        }

        let diff_db = (smoothed - Self::DESIRED_RSSI_HALF_DB) / 2;
        let prev_idx = self.gain_idx;

        if diff_db > 0 {
            let mut idx = self.gain_idx;
            if diff_db >= 10 {
                // Fast attack on strong carrier spike (+8 dB headroom margin for AM peaks)
                let desired_gain_db = (AM_LINEARITY_GAIN_TABLE[idx].1 as i16) - diff_db - 8;
                idx = (1..AM_LINEARITY_GAIN_TABLE.len())
                    .find(|&i| (AM_LINEARITY_GAIN_TABLE[i].1 as i16) >= desired_gain_db)
                    .unwrap_or(1);
            } else if idx > 1 {
                idx -= 1;
            }
            self.gain_idx = idx;
            self.hold_ticks_10ms = 30; // 300ms hold before stepping gain up
        } else if diff_db <= -6 && self.hold_ticks_10ms == 0 {
            self.gain_idx = if self.gain_idx >= AM_LINEARITY_GAIN_TABLE.len() - 1 {
                AM_LINEARITY_GAIN_TABLE.len() - 1
            } else {
                self.gain_idx + 1
            };
        }

        if self.gain_idx != prev_idx {
            Some(AM_LINEARITY_GAIN_TABLE[self.gain_idx].0)
        } else {
            None
        }
    }
}

/// Declarative interface for RF front-end AGC thresholds, manual attenuation, and speed.
pub trait RfAgc {
    fn set_agc_table(&mut self, table: &[u16; 5]);
    fn set_agc_mode(&mut self, mode: AgcListeningMode, disable: bool);
    fn set_agc_speed(&mut self, speed: AgcSpeed);
    /// Apply dynamic AM linearity gain compensation for the given carrier `rssi_dbm`.
    fn compensate_am_linearity(&mut self, _rssi_dbm: i16) {}

    /// Read internal AGC RSSI indicator from silicon (`Reg 0x62` bits `[7:0]`).
    fn get_agc_rssi(&mut self) -> u8 {
        0
    }

    /// Read active front-end AGC gain index from silicon (`Reg 0x7E` bits `[14:12]`).
    fn get_agc_index(&mut self) -> i8 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_am_linearity_fast_attack_and_hold() {
        let mut tracker = AmLinearityTracker::new();
        // Strong -50 dBm AM signal (39 dB above -89 dBm target) triggers immediate attenuation
        let reg13 = tracker.update_10ms(-50);
        assert!(reg13.is_some());
        assert!(AM_LINEARITY_GAIN_TABLE[tracker.gain_idx].1 < -30);
        assert_eq!(tracker.hold_ticks_10ms, 30);
    }
}
