//! Squelch Hysteresis and Signal Metrics for BK4829.

use kernel::service::radio::squelch::SquelchThresholds;

/// Convert coarse 0..9 squelch level into noise/glitch/rssi thresholds.
///
/// Units follow the BK4819 squelch registers: RSSI in 0.5 dB steps above
/// -160 dBm (REG_78), Ex-noise 0..127 (REG_4F), glitch 0..255 (REG_4D/4E).
/// Noise/glitch are inverted: lower = stricter. Curve matches the fagci
/// `GetSql` defaults (RSSI 50→180, noise 64→12, glitch 100→0 over 0..10),
/// precomputed so Cortex-M0+ needs no divide.
pub fn squelch_level_to_thresholds(level: u8) -> SquelchThresholds {
    const HYST: u16 = 4;
    // (open_rssi, open_noise, open_glitch) for levels 1..=9
    const TABLE: [(u16, u16, u16); 9] = [
        (63, 59, 90),
        (76, 54, 80),
        (89, 49, 70),
        (102, 44, 60),
        (115, 38, 50),
        (128, 33, 40),
        (141, 28, 30),
        (154, 23, 20),
        (167, 18, 10),
    ];
    if level == 0 {
        // Squelch off: always open.
        return SquelchThresholds {
            open_rssi: 0,
            close_rssi: 0,
            open_noise: 127,
            close_noise: 127,
            open_glitch: 255,
            close_glitch: 255,
        };
    }
    let (ro, no, go) = TABLE[(level.min(9) - 1) as usize];
    SquelchThresholds {
        open_rssi: ro,
        close_rssi: ro - HYST,
        open_noise: no,
        close_noise: no + HYST,
        open_glitch: go,
        close_glitch: go + HYST,
    }
}

/// Convert raw register 0x67 RSSI count into calibrated dBm.
#[inline(always)]
pub fn raw_rssi_to_dbm(raw: u16) -> i16 {
    -160 + (raw as i16 / 2)
}
