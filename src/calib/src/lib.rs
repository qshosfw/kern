#![no_std]

//! KERN Silicon Calibration Subsystem
//!
//! Models the 512-byte packed factory calibration sector matching
//! Quansheng UV-K1 / UV-K5 EEPROM addresses 0x1E00..0x2000.
//!
//! Memory Geometry:
//! - 0x000..0x0BF (192 B): Squelch thresholds (2 banks x 96 B: UHF and VHF)
//! - 0x0C0..0x0CF ( 16 B): S-meter / RSSI thresholds (2 bands x 4 x u16: UHF and VHF)
//! - 0x0D0..0x13F (112 B): Per-band TX power PA bias (7 bands x 16 B)
//! - 0x140..0x14B ( 12 B): Battery ADC 6-point calibration curve
//! - 0x14C..0x14F (  4 B): Battery reserved padding
//! - 0x150..0x163 ( 20 B): VOX1 thresholds (10 x u16)
//! - 0x164..0x167 (  4 B): VOX1 reserved padding
//! - 0x168..0x17B ( 20 B): VOX0 thresholds (10 x u16)
//! - 0x17C..0x17F (  4 B): VOX0 reserved padding
//! - 0x180..0x184 (  5 B): Mic sensitivity gain dB (5 levels)
//! - 0x185..0x187 (  3 B): Mic sensitivity padding
//! - 0x188..0x18F (  8 B): Calibration Misc (XTAL trim, LNA/Mixer masks, Volume/DAC gains)
//! - 0x190..0x1FF (112 B): Reserved / Alignment padding to 512 bytes

/// Squelch parameter table for one frequency bank (96 bytes).
/// Contains 6 tables of 16 bytes for levels 0..9 (entries 10..15 reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct SquelchBandTable {
    pub open_rssi: [u8; 16],
    pub close_rssi: [u8; 16],
    pub open_noise: [u8; 16],
    pub close_noise: [u8; 16],
    pub close_glitch: [u8; 16],
    pub open_glitch: [u8; 16],
}

/// Squelch thresholds for BK4819 / BK4829 silicon registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SquelchThresholds {
    pub open_rssi: u16,
    pub close_rssi: u16,
    pub open_noise: u16,
    pub close_noise: u16,
    pub open_glitch: u16,
    pub close_glitch: u16,
}

/// 16-byte TX power calibration record for a single frequency band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BandTxPower {
    pub low: [u8; 3],  // Lower, mid, upper frequency points for Low power
    pub mid: [u8; 3],  // Lower, mid, upper frequency points for Mid power
    pub high: [u8; 3], // Lower, mid, upper frequency points for High power
    pub _pad: [u8; 7],
}

impl BandTxPower {
    #[inline(always)]
    pub fn power_slice(&self, power_level: u8) -> [u8; 3] {
        match power_level {
            0 => self.low,
            1 => self.mid,
            _ => self.high,
        }
    }
}

/// Complete 512-byte naturally-aligned calibration layout.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(4))]
pub struct CalibrationBlock {
    pub squelch_uhf: SquelchBandTable, // 0x000..0x05F (96 bytes)
    pub squelch_vhf: SquelchBandTable, // 0x060..0x0BF (96 bytes)
    pub rssi_uhf: [u16; 4],            // 0x0C0..0x0C7 (8 bytes)
    pub rssi_vhf: [u16; 4],            // 0x0C8..0x0CF (8 bytes)
    pub tx_power: [BandTxPower; 7],    // 0x0D0..0x13F (112 bytes)
    pub battery_curve: [u16; 6],       // 0x140..0x14B (12 bytes)
    pub _reserved_bat: [u8; 4],        // 0x14C..0x14F (4 bytes)
    pub vox1_thresholds: [u16; 10],    // 0x150..0x163 (20 bytes)
    pub _reserved_vox1: [u8; 4],       // 0x164..0x167 (4 bytes)
    pub vox0_thresholds: [u16; 10],    // 0x168..0x17B (20 bytes)
    pub _reserved_vox0: [u8; 4],       // 0x17C..0x17F (4 bytes)
    pub mic_sensitivity: [u8; 5],      // 0x180..0x184 (5 bytes)
    pub _reserved_mic: [u8; 3],        // 0x185..0x187 (3 bytes)
    pub xtal_freq_low: i16,            // 0x188..0x189 (2 bytes)
    pub lna_calib: u16,                // 0x18A..0x18B (2 bytes)
    pub mix_calib: u16,                // 0x18C..0x18D (2 bytes)
    pub volume_gain: u8,               // 0x18E (1 byte)
    pub dac_gain: u8,                  // 0x18F (1 byte)
    pub _reserved_end: [u8; 112],      // 0x190..0x1FF (112 bytes)
}

const _: () = {
    assert!(core::mem::size_of::<CalibrationBlock>() == 512);
    assert!(core::mem::offset_of!(CalibrationBlock, squelch_uhf) == 0x000);
    assert!(core::mem::offset_of!(CalibrationBlock, squelch_vhf) == 0x060);
    assert!(core::mem::offset_of!(CalibrationBlock, rssi_uhf) == 0x0C0);
    assert!(core::mem::offset_of!(CalibrationBlock, rssi_vhf) == 0x0C8);
    assert!(core::mem::offset_of!(CalibrationBlock, tx_power) == 0x0D0);
    assert!(core::mem::offset_of!(CalibrationBlock, battery_curve) == 0x140);
    assert!(core::mem::offset_of!(CalibrationBlock, vox1_thresholds) == 0x150);
    assert!(core::mem::offset_of!(CalibrationBlock, vox0_thresholds) == 0x168);
    assert!(core::mem::offset_of!(CalibrationBlock, mic_sensitivity) == 0x180);
    assert!(core::mem::offset_of!(CalibrationBlock, xtal_freq_low) == 0x188);
    assert!(core::mem::offset_of!(CalibrationBlock, lna_calib) == 0x18A);
    assert!(core::mem::offset_of!(CalibrationBlock, mix_calib) == 0x18C);
    assert!(core::mem::offset_of!(CalibrationBlock, volume_gain) == 0x18E);
    assert!(core::mem::offset_of!(CalibrationBlock, dac_gain) == 0x18F);
};

/// Frequency limits (lower, center, upper) in Hz for the 7 factory calibration bands.
pub const CALIB_BAND_LIMITS: [(u32, u32, u32); 7] = [
    (50_000_000, 63_000_000, 76_000_000),    // Band 0: 50 MHz
    (108_000_000, 122_500_000, 137_000_000), // Band 1: 108 MHz
    (137_000_000, 155_500_000, 174_000_000), // Band 2: 137 MHz (VHF)
    (174_000_000, 262_000_000, 350_000_000), // Band 3: 174 MHz
    (350_000_000, 375_000_000, 400_000_000), // Band 4: 350 MHz
    (400_000_000, 435_000_000, 470_000_000), // Band 5: 400 MHz (UHF)
    (470_000_000, 535_000_000, 600_000_000), // Band 6: 470 MHz
];

/// Identify calibration frequency band index (0..6) for a given frequency in Hz.
#[inline]
pub fn get_band_index(freq_hz: u32) -> usize {
    let mut band = 0;
    while band < 6 && freq_hz >= CALIB_BAND_LIMITS[band + 1].0 {
        band += 1;
    }
    band
}

fn lerp_u8(v0: u8, v1: u8, off: u32, span: u32) -> u8 {
    if span == 0 {
        return v0;
    }
    let diff = v1.abs_diff(v0) as u32;
    let delta = (diff * off) / span;
    if v1 >= v0 {
        (v0 as u32 + delta).min(255) as u8
    } else {
        v0.saturating_sub(delta as u8)
    }
}

/// Piecewise linear interpolation for TX power bias across band frequency limits.
pub fn calculate_tx_power(
    txp_low: u8,
    txp_mid: u8,
    txp_high: u8,
    lower: u32,
    middle: u32,
    upper: u32,
    freq: u32,
) -> u8 {
    if freq <= lower {
        return txp_low;
    }
    if freq >= upper {
        return txp_high;
    }
    if freq <= middle {
        lerp_u8(txp_low, txp_mid, freq - lower, middle - lower)
    } else {
        lerp_u8(txp_mid, txp_high, freq - middle, upper - middle)
    }
}

/// Piecewise linear interpolation across the 6 factory battery voltage points.
pub fn calculate_battery_percentage(raw_adc: u16, curve: &[u16; 6]) -> u8 {
    if raw_adc <= curve[0] {
        return 0;
    }
    if raw_adc >= curve[5] {
        return 100;
    }
    for i in 0..5 {
        let low_adc = curve[i];
        let high_adc = curve[i + 1];
        if raw_adc <= high_adc {
            let base = (i as u8) * 20;
            return lerp_u8(
                base,
                base + 20,
                raw_adc.saturating_sub(low_adc) as u32,
                high_adc.saturating_sub(low_adc) as u32,
            );
        }
    }
    100
}

/// Convert raw battery ADC reading to millivolts using the 7.60V baseline calibration constant.
#[inline(always)]
pub fn calculate_battery_mv(raw_adc: u16, calib_const: u16) -> u16 {
    let cal = if calib_const >= 1000 && calib_const <= 3500 {
        calib_const as u32
    } else {
        2227
    };
    ((raw_adc as u32 * 7600) / cal).clamp(0, 15000) as u16
}

impl CalibrationBlock {
    pub const SIZE: usize = core::mem::size_of::<Self>();

    /// Safe zero-copy transmutation from a 512-byte slice.
    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 512]) -> &Self {
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    /// Extract calibrated squelch thresholds for a given frequency and squelch level (0..9).
    pub fn get_squelch_thresholds(&self, freq_hz: u32, level: u8) -> SquelchThresholds {
        if level == 0 {
            return SquelchThresholds {
                open_rssi: 0,
                close_rssi: 0,
                open_noise: 127,
                close_noise: 127,
                open_glitch: 255,
                close_glitch: 255,
            };
        }

        let idx = (level.min(9)) as usize;
        let table = if freq_hz < 174_000_000 {
            &self.squelch_vhf
        } else {
            &self.squelch_uhf
        };
        SquelchThresholds {
            open_rssi: table.open_rssi[idx] as u16,
            close_rssi: table.close_rssi[idx] as u16,
            open_noise: table.open_noise[idx] as u16,
            close_noise: table.close_noise[idx] as u16,
            close_glitch: table.close_glitch[idx] as u16,
            open_glitch: table.open_glitch[idx] as u16,
        }
    }

    /// Calculate calibrated TX power PA bias value for REG_36.
    pub fn get_tx_power_bias(&self, freq_hz: u32, power_level: u8) -> u8 {
        let band = get_band_index(freq_hz);
        let (lower, middle, upper) = CALIB_BAND_LIMITS[band];
        let txp_band = self.tx_power[band];
        let txp = txp_band.power_slice(power_level);
        calculate_tx_power(txp[0], txp[1], txp[2], lower, middle, upper, freq_hz)
    }

    /// Retrieve the 4-level RSSI thresholds for S-meter evaluation.
    pub fn get_rssi_thresholds(&self, freq_hz: u32) -> [u16; 4] {
        if freq_hz < 174_000_000 {
            self.rssi_vhf
        } else {
            self.rssi_uhf
        }
    }

    /// Evaluate S-meter signal level (0..4) based on calibrated RSSI thresholds.
    pub fn get_s_meter_level(&self, freq_hz: u32, raw_rssi: u16) -> u8 {
        let th = self.get_rssi_thresholds(freq_hz);
        if raw_rssi >= th[3] {
            4
        } else if raw_rssi >= th[2] {
            3
        } else if raw_rssi >= th[1] {
            2
        } else if raw_rssi >= th[0] {
            1
        } else {
            0
        }
    }

    /// Convert raw register 0x67 RSSI count into calibrated dBm using 4-point calibration curve.
    pub fn raw_rssi_to_dbm(&self, freq_hz: u32, raw: u16) -> i16 {
        let th = self.get_rssi_thresholds(freq_hz);
        if th[0] == 0 || th[3] == 0 || th[3] <= th[0] {
            return (raw >> 1) as i16 - 160;
        }
        if raw <= th[0] {
            let diff = th[0].saturating_sub(raw);
            return -123 - (diff >> 1) as i16;
        }
        if raw >= th[3] {
            let diff = raw.saturating_sub(th[3]);
            return (-73 + (diff >> 1) as i16).min(-10);
        }
        const DBM_POINTS: [i16; 4] = [-123, -107, -83, -73];
        for i in 0..3 {
            if raw <= th[i + 1] {
                let span = th[i + 1].saturating_sub(th[i]) as u32;
                let offset = raw.saturating_sub(th[i]) as u32;
                let delta_dbm = (DBM_POINTS[i + 1] - DBM_POINTS[i]) as u8;
                return DBM_POINTS[i] + lerp_u8(0, delta_dbm, offset, span) as i16;
            }
        }
        -73
    }

    /// Battery calibration baseline constant at 7.60V (word 3).
    #[inline(always)]
    pub fn battery_calib_const(&self) -> u16 {
        self.battery_curve[3]
    }

    /// Battery voltage in millivolts calculated from raw ADC.
    #[inline(always)]
    pub fn battery_mv(&self, raw_adc: u16) -> u16 {
        calculate_battery_mv(raw_adc, self.battery_calib_const())
    }

    /// Battery remaining percentage (0..100) using 6-point curve interpolation.
    #[inline(always)]
    pub fn battery_percentage(&self, raw_adc: u16) -> u8 {
        let curve = self.battery_curve;
        calculate_battery_percentage(raw_adc, &curve)
    }

    /// Get calibrated VOX thresholds (VOX0 close threshold, VOX1 open threshold) for levels 1..10.
    pub fn get_vox_thresholds(&self, level: u8) -> (u16, u16) {
        if level == 0 {
            return (0, 0);
        }
        let idx = (level - 1).min(9) as usize;
        let v0 = self.vox0_thresholds[idx];
        let v1 = self.vox1_thresholds[idx];
        if v0 != 0 && v1 != 0 {
            (v0, v1)
        } else {
            match level {
                1 => (800, 850),
                2 => (700, 750),
                3 => (600, 650),
                4 => (500, 550),
                5 => (400, 450),
                6 => (300, 350),
                7 => (220, 260),
                8 => (150, 180),
                9 => (100, 120),
                _ => (50, 70),
            }
        }
    }

    /// Get calibrated microphone gain in dB for levels 0..4 (or 1..5).
    pub fn get_mic_gain_db(&self, level: u8) -> u8 {
        let idx = level.min(4) as usize;
        self.mic_sensitivity[idx]
    }

    #[inline(always)]
    pub fn xtal_freq_low(&self) -> i16 {
        self.xtal_freq_low
    }

    #[inline(always)]
    pub fn lna_calib(&self) -> u16 {
        self.lna_calib & 0x01FF
    }

    #[inline(always)]
    pub fn mix_calib(&self) -> u16 {
        self.mix_calib & 0x01FF
    }

    #[inline(always)]
    pub fn volume_gain(&self) -> u8 {
        if self.volume_gain < 64 {
            self.volume_gain
        } else {
            58
        }
    }

    #[inline(always)]
    pub fn dac_gain(&self) -> u8 {
        self.dac_gain.min(15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibration_block_size() {
        assert_eq!(core::mem::size_of::<CalibrationBlock>(), 512);
        assert_eq!(core::mem::size_of::<SquelchBandTable>(), 96);
        assert_eq!(core::mem::size_of::<BandTxPower>(), 16);
    }

    #[test]
    fn test_battery_percentage_interpolation() {
        let curve = [1438, 2050, 2138, 2227, 2314, 2600];
        assert_eq!(calculate_battery_percentage(1400, &curve), 0);
        assert_eq!(calculate_battery_percentage(1438, &curve), 0);
        assert_eq!(calculate_battery_percentage(2050, &curve), 20);
        assert_eq!(calculate_battery_percentage(2227, &curve), 60);
        assert_eq!(calculate_battery_percentage(2600, &curve), 100);
        assert_eq!(calculate_battery_percentage(2700, &curve), 100);
    }

    #[test]
    fn test_tx_power_interpolation() {
        // Band 2: 137..174 MHz, lower 137M, mid 155.5M, upper 174M
        let bias = calculate_tx_power(
            50,
            75,
            100,
            137_000_000,
            155_500_000,
            174_000_000,
            137_000_000,
        );
        assert_eq!(bias, 50);
        let bias = calculate_tx_power(
            50,
            75,
            100,
            137_000_000,
            155_500_000,
            174_000_000,
            155_500_000,
        );
        assert_eq!(bias, 75);
        let bias = calculate_tx_power(
            50,
            75,
            100,
            137_000_000,
            155_500_000,
            174_000_000,
            174_000_000,
        );
        assert_eq!(bias, 100);
    }

    #[test]
    fn test_rssi_and_vox_calibration() {
        let mut cal: CalibrationBlock = unsafe { core::mem::zeroed() };
        cal.rssi_vhf = [50, 100, 150, 200];
        cal.vox0_thresholds[0] = 780;
        cal.vox1_thresholds[0] = 820;

        assert_eq!(cal.get_vox_thresholds(1), (780, 820));
        assert_eq!(cal.get_vox_thresholds(2), (700, 750)); // fallback

        // Below point 0
        assert_eq!(cal.raw_rssi_to_dbm(144_000_000, 40), -128);
        // At point 0
        assert_eq!(cal.raw_rssi_to_dbm(144_000_000, 50), -123);
        // At point 1
        assert_eq!(cal.raw_rssi_to_dbm(144_000_000, 100), -107);
        // Midpoint between 0 and 1
        assert_eq!(cal.raw_rssi_to_dbm(144_000_000, 75), -115);
        // At point 3
        assert_eq!(cal.raw_rssi_to_dbm(144_000_000, 200), -73);
    }

    #[test]
    fn test_non_monotonic_and_flat_curves_do_not_underflow() {
        let non_mono_battery = [1400, 2200, 1800, 2300, 2400, 2600];
        assert_eq!(calculate_battery_percentage(1900, &non_mono_battery), 12);
        let flat_battery = [1400, 2000, 2000, 2000, 2400, 2600];
        assert_eq!(calculate_battery_percentage(2200, &flat_battery), 70);

        let mut cal: CalibrationBlock = unsafe { core::mem::zeroed() };
        cal.rssi_vhf = [50, 140, 100, 200];
        assert_eq!(cal.raw_rssi_to_dbm(144_000_000, 110), -113);
        cal.rssi_vhf = [50, 100, 100, 200];
        assert_eq!(cal.raw_rssi_to_dbm(144_000_000, 150), -78);
    }
}
