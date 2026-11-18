use crate::apps::shortcut::{
    KEY_SHORTCUT_COUNT, TARGET_FLASHLIGHT, TARGET_HOME, TARGET_LOCK_KEYPAD, TARGET_NONE,
    TARGET_SETTINGS, TARGET_SLEEP, hash_app,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CtxRule {
    pub slot: u8,
    pub ctx: u8, // 0=Off/Disabled, 1=VFO, 2=Home, 3=Settings, 4=Apps
    pub action: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KeyConfig {
    pub version: u8,                        // 3
    pub nav_layout: u8,                     // 0=K1, 1=K5
    pub auto_lock: u8,                      // 0=Off, 1=15s, 2=30s, 3=1m, 4=2m
    pub f_key_timeout: u8,                  // 0=Toggle, 1=1s, 2=2s, 3=3s, 4=5s
    pub fast_scroll: u8,                    // 0=Off, 1=On
    pub hold_delay: u8,                     // 0=250ms, 1=350ms, 2=500ms, 3=750ms
    pub repeat_rate: u8,                    // 0=30ms, 1=50ms, 2=80ms, 3=120ms
    pub ptt_mode: u8,                       // 0=Momentary, 1=Toggle/Latch, 2=Double-press Latch
    pub f_drawer: u8,                       // 0=Off, 1=Slide-up F-shortcut badge drawer
    pub _pad: u8,                           // 2-byte alignment for actions: [u16; 19]
    pub actions: [u16; KEY_SHORTCUT_COUNT], // 38 B: 0..4 side/menu, 5..18 F+keys
    pub ctx_rules: [CtxRule; 3],            // 12 B: 3 contextual overrides
    pub crc32: u32,                         // offset 60 -> 64 B total (2^6)
}

impl KeyConfig {
    pub const DEFAULT: Self = Self::new();

    pub const fn new() -> Self {
        let mut actions = [TARGET_NONE; KEY_SHORTCUT_COUNT];
        actions[0] = TARGET_FLASHLIGHT; // S1 short
        actions[1] = TARGET_LOCK_KEYPAD; // S1 long
        actions[2] = hash_app("/apps/explorer.krx"); // S2 short
        actions[3] = TARGET_SETTINGS; // S2 long
        actions[4] = hash_app("/apps/tasks.krx"); // Menu long -> Processes!
        actions[13] = TARGET_LOCK_KEYPAD; // F+9
        actions[14] = TARGET_HOME; // F+0
        actions[15] = hash_app("/apps/scanner.krx"); // F+*
        actions[16] = TARGET_SETTINGS; // F+Menu
        let _ = TARGET_SLEEP;

        Self {
            version: 3,
            nav_layout: 0,
            auto_lock: 0,
            f_key_timeout: 3,
            fast_scroll: 1,
            hold_delay: 1,
            repeat_rate: 1,
            ptt_mode: 0,
            f_drawer: 1,
            _pad: 0,
            actions,
            ctx_rules: [CtxRule {
                slot: 0,
                ctx: 0,
                action: 0,
            }; 3],
            crc32: 0,
        }
    }

    pub fn hold_delay_ms(&self) -> u16 {
        match self.hold_delay {
            0 => 300,
            1 => 400,
            2 => 500,
            3 => 750,
            _ => 400,
        }
    }

    pub fn repeat_rate_ms(&self) -> u16 {
        match self.repeat_rate {
            0 => 60,
            1 => 80,
            2 => 100,
            3 => 150,
            _ => 80,
        }
    }

    pub fn hold_ticks(&self) -> u32 {
        (self.hold_delay_ms() as u32) / 50
    }

    pub fn repeat_ticks(&self) -> u32 {
        (self.repeat_rate_ms() as u32).max(50) / 50
    }

    pub fn f_key_timeout_ms(&self) -> u32 {
        match self.f_key_timeout {
            0 => 0,
            1 => 1000,
            2 => 2000,
            3 => 3000,
            4 => 5000,
            _ => 3000,
        }
    }

    pub fn auto_lock_ms(&self) -> u32 {
        match self.auto_lock {
            0 => 0,
            1 => 15_000,
            2 => 30_000,
            3 => 60_000,
            4 => 120_000,
            _ => 0,
        }
    }

    pub fn f_key_timeout_ticks(&self) -> u32 {
        self.f_key_timeout_ms() / 50
    }

    pub fn auto_lock_ticks(&self) -> u32 {
        self.auto_lock_ms() / 50
    }
}

crate::config_block!(KeyConfig, "/pref/keys.cfg", 3);
