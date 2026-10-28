//! Squelch Hysteresis and Signal Metrics for BK4829.

use kernel::service::radio::squelch::SquelchThresholds;

/// Convert coarse 0..9 squelch level into noise/glitch/rssi thresholds.
pub fn squelch_level_to_thresholds(level: u8) -> SquelchThresholds {
    let noise = match level {
        0 => 0x0000,
        1 => 0x0020,
        2 => 0x0040,
        3 => 0x0060,
        4 => 0x0080,
        5 => 0x00A0,
        6 => 0x00C0,
        7 => 0x00E0,
        8 => 0x0100,
        _ => 0x0120,
    };
    SquelchThresholds {
        open_noise: noise,
        close_noise: noise.saturating_add(0x0010),
        open_glitch: 0x0020,
        close_glitch: 0x0028,
        open_rssi: 0x0040,
        close_rssi: 0x0030,
    }
}

/// Convert raw register 0x67 RSSI count into calibrated dBm.
#[inline(always)]
pub fn raw_rssi_to_dbm(raw: u16) -> i16 {
    -160 + (raw as i16 / 2)
}
