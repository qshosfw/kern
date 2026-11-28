//! Application Domain Configuration, Autostart Roster & Runtime Policies.
//! Stored in /pref/apps.cfg (CRC32-protected, power-of-two 64 bytes).

use crate::config_block;

pub const RUNTIME_FLAG_AUTO_SUSPEND: u8 = 1 << 0;
pub const RUNTIME_FLAG_SWAP_FLASH: u8 = 1 << 1;
pub const RUNTIME_FLAG_AUTO_QUARANTINE: u8 = 1 << 2;
pub const RUNTIME_FLAG_STRICT_BUDGET: u8 = 1 << 3;
pub const RUNTIME_FLAG_FULL_FORENSICS: u8 = 1 << 4;
pub const RUNTIME_FLAG_HINT_NEXT: u8 = 1 << 5;
pub const RUNTIME_FLAG_WARM_RESUME: u8 = 1 << 6;

pub const AUTOSTART_STATE_ENABLED: u8 = 1 << 7;
pub const AUTOSTART_STATE_QUARANTINED: u8 = 1 << 6;
pub const AUTOSTART_STATE_STRIKES_MASK: u8 = 0b0011_0000;
pub const AUTOSTART_STATE_STRIKES_SHIFT: u8 = 4;
pub const AUTOSTART_STATE_ROLE_MASK: u8 = 0b0000_1100;
pub const AUTOSTART_STATE_ROLE_SHIFT: u8 = 2;
pub const AUTOSTART_STATE_LAST_FAULT_MASK: u8 = 0b0000_0011;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AppsConfig {
    // Bytes 0..8: Core Launcher & Runtime Policies
    pub version: u8,        // 3
    pub launcher_style: u8, // 0 = Grid, 1 = List
    pub show_numbers: u8,   // 0 = Off, 1 = On (1..6 badges)
    pub show_labels: u8,    // 0 = Off, 1 = On
    pub startup_app: u8,    // 0 = Launcher, 1 = Radio (VFO), 2 = Custom KRX (startup_hash)
    pub runtime_flags: u8,  // RUNTIME_FLAG_* bitmask
    pub pager_policy: u8,   // 0 = Balanced, 1 = Fast (1KB Window), 2 = Max Concurrent
    pub swap_policy: u8,    // 0 = Off, 1 = SRAM LZSS, 2 = Flash Suspend

    // Bytes 8..16: Startup Custom App + Runtime Watchdog Budgets
    pub startup_hash: u32,    // Package hash of pinned custom startup KRX
    pub daemon_budget_ms: u8, // Max tick slice before warning/demote (default 4 ms)
    pub hook_budget_ms: u8,   // Max UI/VFO hook latency (default 8 ms)
    pub max_strikes: u8,      // Faults before quarantine (default 2)
    pub zone_b_cache_kb: u8,  // Pager cache ceiling inside Zone B (default 3 KB)

    // Bytes 16..56: Unified Autostart + Crash-Loop Guard (8 slots, naturally aligned)
    pub autostart_hashes: [u32; 8], // 0 = empty slot; else package filename hash
    pub autostart_states: [u8; 8],  // AUTOSTART_STATE_* flags

    // Bytes 56..64: Expansion + Integrity
    pub reserved: [u8; 4],
    pub crc32: u32,
}

config_block!(AppsConfig, "/pref/apps.cfg", 3);

impl AppsConfig {
    pub const DEFAULT: Self = Self::new();

    pub const fn new() -> Self {
        Self {
            version: 3,
            launcher_style: 0, // Grid by default
            show_numbers: 1,   // On by default
            show_labels: 1,    // On by default
            startup_app: 0,    // Launcher by default
            runtime_flags: RUNTIME_FLAG_AUTO_QUARANTINE | RUNTIME_FLAG_HINT_NEXT,
            pager_policy: 0,
            swap_policy: 0,
            startup_hash: 0,
            daemon_budget_ms: 4,
            hook_budget_ms: 8,
            max_strikes: 2,
            zone_b_cache_kb: 3,
            autostart_hashes: [0; 8],
            autostart_states: [0; 8],
            reserved: [0; 4],
            crc32: 0,
        }
    }
}
