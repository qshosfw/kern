#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KeyConfig {
    pub version: u8,    // 2
    pub nav_layout: u8, // 0=K1, 1=K5
    pub f1_short: u8,
    pub f1_long: u8,
    pub f2_short: u8,
    pub f2_long: u8,
    pub m_long: u8,
    pub auto_lock: u8,     // 0=Off, 1=15s, 2=30s, 3=1m, 4=2m
    pub f_key_timeout: u8, // 0=Toggle, 1=1s, 2=2s, 3=3s, 4=5s
    pub fast_scroll: u8,   // 0=Off, 1=On
    pub hold_delay: u8,    // 0=250ms, 1=350ms, 2=500ms, 3=750ms
    pub repeat_rate: u8,   // 0=30ms (turbo), 1=50ms (fast), 2=80ms (normal), 3=120ms (slow)
    pub f_digits: [u8; 9], // Preset shortcuts for F+1 through F+9
    pub _pad: [u8; 3],     // 4-byte alignment
    pub crc32: u32,
}

impl KeyConfig {
    pub const fn new() -> Self {
        Self {
            version: 2,
            nav_layout: 0,
            f1_short: 1,      // Flashlight
            f1_long: 2,       // Monitor (Squelch open)
            f2_short: 7,      // Toggle VFO
            f2_long: 8,       // Toggle TX Power
            m_long: 6,        // Lock Keypad
            auto_lock: 0,     // Off
            f_key_timeout: 3, // 3s
            fast_scroll: 1,   // On
            hold_delay: 1,    // 400ms (DeltaFW)
            repeat_rate: 1,   // 80ms (DeltaFW)
            f_digits: [0; 9], // None by default
            _pad: [0; 3],
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

    pub fn f_key_timeout_ticks(&self) -> u32 {
        match self.f_key_timeout {
            0 => 0,   // Toggle mode (no timeout)
            1 => 20,  // 1s @ 50ms tick
            2 => 40,  // 2s
            3 => 60,  // 3s
            4 => 100, // 5s
            _ => 60,
        }
    }

    pub fn auto_lock_ticks(&self) -> u32 {
        match self.auto_lock {
            0 => 0,        // Off
            1 => 15 * 20,  // 15s (300 ticks @ 50ms)
            2 => 30 * 20,  // 30s (600 ticks)
            3 => 60 * 20,  // 1m  (1200 ticks)
            4 => 120 * 20, // 2m  (2400 ticks)
            _ => 0,
        }
    }
}

crate::config_block!(KeyConfig, "/pref/keys.cfg", 2);
