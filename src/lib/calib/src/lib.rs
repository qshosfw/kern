#![no_std]

/// KERN Calibration Service
/// Parses legacy RF calibration data into safe Rust types

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct CalibrationBlock {
    pub squelch_open_rssi: [u8; 2],
    pub squelch_close_rssi: [u8; 2],
    pub squelch_open_noise: [u8; 2],
    pub squelch_close_noise: [u8; 2],
    pub squelch_close_glitch: [u8; 2],
    pub squelch_open_glitch: [u8; 2],

    // Padding or reserved space up to RSSI section
    pub _reserved1: [u8; 0x100C0 - 0x10006],

    pub rssi_level_3: [u8; 8],
    pub rssi_level_0: [u8; 8],

    pub tx_power_low: [u8; 7],
    pub tx_power_mid: [u8; 7],
    pub tx_power_high: [u8; 7],

    // Padding or reserved space up to Battery section
    pub _reserved2: [u8; 0x10140 - (0x100D0 + 21)],

    pub battery_voltages: [u16; 6], // 12 bytes

    pub vox_gain_1: [u8; 12],
    pub vox_gain_0: [u8; 12],

    // Padding up to Misc section
    pub _reserved3: [u8; 0x10188 - (0x10150 + 24)],

    // Misc
    pub freq_calib: u16,
    pub lna_calib: u16,
    pub mix_calib: u16,
    pub _reserved4: u16,
}

impl CalibrationBlock {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}
