//! Application Domain Configuration & Launcher Preferences
//! Stored in /pref/apps.cfg (CRC32-protected, power-of-two 32 bytes).

use crate::config_block;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AppsConfig {
    pub version: u8,             // 1
    pub launcher_style: u8,      // 0 = Grid, 1 = List (Scroll)
    pub show_numbers: u8,        // 0 = Off, 1 = On (keypad 1..6 badges)
    pub show_labels: u8,         // 0 = Off, 1 = On (app titles)
    pub startup_app: u8,         // 0 = Launcher, 1 = Radio (VFO)
    pub vfo_dual_watch: u8,      // 0 = Off, 1 = Dual Watch, 2 = Full Watch
    pub scanner_dwell_time: u8,  // Scan dwell duration (100ms units)
    pub app_flags: u8,           // General application flags
    pub app_slots: [u8; 8],      // 8 dedicated userland / app configuration slots
    pub reserved_tail: [u8; 12], // Extra expansion padding
    pub crc32: u32,
}

config_block!(AppsConfig, "/pref/apps.cfg", 1);

impl AppsConfig {
    pub const fn new() -> Self {
        Self {
            version: 1,
            launcher_style: 0, // Grid by default
            show_numbers: 1,   // On by default
            show_labels: 1,    // On by default
            startup_app: 0,    // Launcher by default
            vfo_dual_watch: 0,
            scanner_dwell_time: 5, // 500ms
            app_flags: 0,
            app_slots: [0; 8],
            reserved_tail: [0; 12],
            crc32: 0,
        }
    }
}
