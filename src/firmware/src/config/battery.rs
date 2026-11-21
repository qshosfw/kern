#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BatteryConfig {
    pub version: u8,            // 1
    pub battery_type: u8,       // 0=1600mAh, 1=2200mAh, 2=3500mAh, 3=1500mAh K1, 4=2500mAh K1
    pub battery_format: u8,     // 0..8 UI representation format
    pub power_profile: u8,      // 0=Power Save, 1=Balanced, 2=Performance
    pub auto_power_off_min: u8, // 0=Off, 1=15m, 2=30m, 3=60m
    pub charging_detect: u8,    // 0=Disabled, 1=Enabled
    pub calib_tweak_mv: i16,    // Calibration offset tweak in mV (-500mV to +500mV)
    pub low_bat_warn_mv: u16,   // Warning threshold (default 6800 mV)
    pub low_bat_shut_mv: u16,   // Cutoff threshold (default 6300 mV)
    pub _reserved: [u8; 4],     // 4-byte alignment padding before crc32 (total 20 bytes)
    pub crc32: u32,
}

impl BatteryConfig {
    pub const DEFAULT: Self = Self::new();

    pub const fn new() -> Self {
        Self {
            version: 1,
            battery_type: 0,
            battery_format: 5,
            power_profile: 1,      // Balanced
            auto_power_off_min: 0, // Off
            charging_detect: 1,    // Enabled
            calib_tweak_mv: 0,
            low_bat_warn_mv: 6800,
            low_bat_shut_mv: 6300,
            _reserved: [0; 4],
            crc32: 0,
        }
    }
}

crate::config_block!(BatteryConfig, "/pref/battery.cfg", 1);
