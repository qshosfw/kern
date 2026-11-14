pub mod apps;
pub mod audio;
pub mod battery;
pub mod display;
pub mod keys;
pub mod system;

pub use apps::AppsConfig;
pub use audio::AudioConfig;
pub use battery::BatteryConfig;
pub use display::DisplayConfig;
pub use keys::KeyConfig;
pub use system::SystemConfig;

#[inline(never)]
pub(crate) fn read_cstr(buf: &[u8]) -> &str {
    let len = gfx::text::find_byte(buf, 0).unwrap_or(buf.len());
    unsafe { core::str::from_utf8_unchecked(&buf[..len]) }
}

#[inline(never)]
pub(crate) fn write_cstr(buf: &mut [u8], path: &str) {
    buf.fill(0);
    let bytes = path.as_bytes();
    let copy_len = bytes.len().min(buf.len());
    buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
}

pub fn update_raw_crc(buf: &mut [u8]) {
    let len = buf.len();
    if len > 4 {
        let crc = crypto::crc::CrcEngine::crc32(&buf[..len - 4]);
        let b = crc.to_le_bytes();
        let tail = len - 4;
        buf[tail] = b[0];
        buf[tail + 1] = b[1];
        buf[tail + 2] = b[2];
        buf[tail + 3] = b[3];
    }
}

pub fn is_raw_valid(buf: &[u8], expected_version: u8) -> bool {
    let len = buf.len();
    if len <= 4 || buf[0] != expected_version {
        return false;
    }
    let calc_crc = crypto::crc::CrcEngine::crc32(&buf[..len - 4]);
    let tail = len - 4;
    let stored_crc = u32::from_le_bytes([buf[tail], buf[tail + 1], buf[tail + 2], buf[tail + 3]]);
    stored_crc == calc_crc
}

pub fn save_config_raw(
    storage: &mut SystemStorage,
    path: &str,
    buf: &mut [u8],
) -> Result<(), ConfigError> {
    update_raw_crc(buf);
    let _ = storage.create_dir("/pref");
    storage
        .write_file(path, buf)
        .map_err(|_| ConfigError::StorageWriteFailed)
}

pub trait ConfigBlock: Sized + 'static {
    const VERSION: u8;
    const PATH: &'static str;

    fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                core::mem::size_of::<Self>(),
            )
        }
    }

    fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self as *mut Self as *mut u8,
                core::mem::size_of::<Self>(),
            )
        }
    }

    fn update_crc(&mut self) {
        update_raw_crc(self.as_bytes_mut());
    }

    fn is_valid(&self) -> bool {
        is_raw_valid(self.as_bytes(), Self::VERSION)
    }
}

#[macro_export]
macro_rules! config_block {
    ($struct_name:ident, $path:expr, $version:expr) => {
        impl $crate::config::ConfigBlock for $struct_name {
            const VERSION: u8 = $version;
            const PATH: &'static str = $path;
        }

        impl $struct_name {
            #[inline(always)]
            pub fn update_crc(&mut self) {
                $crate::config::ConfigBlock::update_crc(self);
            }

            #[inline(always)]
            pub fn is_valid(&self) -> bool {
                $crate::config::ConfigBlock::is_valid(self)
            }
        }

        impl Default for $struct_name {
            fn default() -> Self {
                let mut cfg = Self::new();
                $crate::config::ConfigBlock::update_crc(&mut cfg);
                cfg
            }
        }
    };
}

use crate::services::SystemStorage;
#[derive(Debug)]
pub enum ConfigError {
    NotFound,
    IoError,
    ParseError,
    StorageWriteFailed,
}

pub const DIRTY_DISPLAY: u8 = 1 << 0;
pub const DIRTY_AUDIO: u8 = 1 << 1;
pub const DIRTY_KEYS: u8 = 1 << 2;
pub const DIRTY_SYSTEM: u8 = 1 << 3;
pub const DIRTY_BATTERY: u8 = 1 << 4;
pub const DIRTY_APPS: u8 = 1 << 5;

pub const HW_DIRTY_DISPLAY: u8 = DIRTY_DISPLAY;
pub const HW_DIRTY_AUDIO: u8 = DIRTY_AUDIO;
pub const HW_DIRTY_KEYS: u8 = DIRTY_KEYS;
pub const HW_DIRTY_SYSTEM: u8 = DIRTY_SYSTEM;
pub const HW_DIRTY_BATTERY: u8 = DIRTY_BATTERY;

#[derive(Debug, Clone, Copy, Default)]
pub struct MasterConfig {
    pub display: DisplayConfig,
    pub audio: AudioConfig,
    pub keys: KeyConfig,
    pub system: SystemConfig,
    pub battery: BatteryConfig,
    pub apps: AppsConfig,

    pub dirty: u8,
    pub hw_dirty: u8,
}

const _: () = {
    assert!(core::mem::size_of::<DisplayConfig>() == 48);
    assert!(core::mem::offset_of!(DisplayConfig, crc32) == 44);
    assert!(core::mem::size_of::<AudioConfig>() == 80);
    assert!(core::mem::offset_of!(AudioConfig, crc32) == 76);
    assert!(core::mem::size_of::<KeyConfig>() == 36);
    assert!(core::mem::offset_of!(KeyConfig, crc32) == 32);
    assert!(core::mem::size_of::<SystemConfig>() == 44);
    assert!(core::mem::offset_of!(SystemConfig, crc32) == 40);
    assert!(core::mem::size_of::<BatteryConfig>() == 20);
    assert!(core::mem::offset_of!(BatteryConfig, crc32) == 16);
    assert!(core::mem::size_of::<AppsConfig>() == 64);
    assert!(core::mem::align_of::<AppsConfig>() == 4);
    assert!(core::mem::offset_of!(AppsConfig, autostart_hashes) == 16);
    assert!(core::mem::offset_of!(AppsConfig, crc32) == 60);
};

pub fn load_or_init_raw(buf: &mut [u8], version: u8, path: &str, storage: &mut SystemStorage) {
    let mut temp = [0u8; 128];
    let n = buf.len().min(temp.len());
    let loaded = if let Ok(len) = storage.read_file(path, &mut temp[..n]) {
        len >= n && is_raw_valid(&temp[..n], version)
    } else {
        false
    };
    if loaded {
        buf[..n].copy_from_slice(&temp[..n]);
    } else {
        update_raw_crc(buf);
        let _ = save_config_raw(storage, path, buf);
    }
}

struct ConfigMeta {
    version: u8,
    path: &'static str,
}

static CONFIG_METAS: [ConfigMeta; 6] = [
    ConfigMeta {
        version: DisplayConfig::VERSION,
        path: DisplayConfig::PATH,
    },
    ConfigMeta {
        version: AudioConfig::VERSION,
        path: AudioConfig::PATH,
    },
    ConfigMeta {
        version: KeyConfig::VERSION,
        path: KeyConfig::PATH,
    },
    ConfigMeta {
        version: SystemConfig::VERSION,
        path: SystemConfig::PATH,
    },
    ConfigMeta {
        version: BatteryConfig::VERSION,
        path: BatteryConfig::PATH,
    },
    ConfigMeta {
        version: AppsConfig::VERSION,
        path: AppsConfig::PATH,
    },
];

#[inline(never)]
fn check_bounds(bytes: &[u8], expected_ver: u8, rules: &[[u8; 2]]) -> bool {
    if bytes[0] != expected_ver {
        return false;
    }
    for &[offset, max] in rules {
        if bytes[offset as usize] > max {
            return false;
        }
    }
    true
}

static KEY_RULES: [[u8; 2]; 6] = [[7, 5], [8, 3], [9, 2], [10, 3], [11, 2], [21, 2]];
static BATT_RULES: [[u8; 2]; 4] = [[2, 8], [3, 3], [4, 5], [5, 1]];
static DISPLAY_RULES: [[u8; 2]; 11] = [
    [1, 5],
    [2, 10],
    [3, 10],
    [4, 100],
    [5, 1],
    [6, 3],
    [7, 1],
    [8, 4],
    [9, 2],
    [10, 1],
    [11, 3],
];
static AUDIO_RULES: [[u8; 2]; 7] = [[1, 10], [2, 5], [3, 31], [4, 1], [6, 10], [7, 4], [8, 4]];
static APPS_RULES: [[u8; 2]; 10] = [
    [1, 1],   // launcher_style: 0..=1
    [2, 1],   // show_numbers: 0..=1
    [3, 1],   // show_labels: 0..=1
    [4, 2],   // startup_app: 0..=2 (Launcher, Radio, Custom KRX)
    [6, 2],   // pager_policy: 0..=2
    [7, 2],   // swap_policy: 0..=2
    [12, 16], // daemon_budget_ms <= 16
    [13, 32], // hook_budget_ms <= 32
    [14, 3],  // max_strikes <= 3
    [15, 4],  // zone_b_cache_kb <= 4
];

static DEFAULT_MASTER_CONFIG: MasterConfig = MasterConfig::DEFAULT;

impl MasterConfig {
    pub const DEFAULT: Self = Self {
        display: DisplayConfig::DEFAULT,
        audio: AudioConfig::DEFAULT,
        keys: KeyConfig::DEFAULT,
        system: SystemConfig::DEFAULT,
        battery: BatteryConfig::DEFAULT,
        apps: AppsConfig::DEFAULT,
        dirty: 0,
        hw_dirty: 0,
    };

    pub fn new() -> Self {
        Self::default()
    }

    #[inline(never)]
    pub unsafe fn init_in_place(ptr: *mut Self) {
        unsafe {
            core::ptr::copy_nonoverlapping(&DEFAULT_MASTER_CONFIG, ptr, 1);
            let nav = (*ptr).system.nav_layout();
            (*ptr).keys.nav_layout = nav;
        }
    }

    pub fn is_k1(&self) -> bool {
        self.system.is_k1()
    }

    pub fn nav_layout(&self) -> u8 {
        self.system.nav_layout()
    }

    #[inline(never)]
    pub fn mark_domain_dirty(&mut self, idx: usize) {
        let bit = 1u8 << idx;
        self.dirty |= bit;
        self.hw_dirty |= bit & 0x1F;
        self.seal(idx);
    }

    pub fn mark_display_dirty(&mut self) {
        self.mark_domain_dirty(0);
    }

    pub fn mark_audio_dirty(&mut self) {
        self.mark_domain_dirty(1);
    }

    pub fn mark_keys_dirty(&mut self) {
        self.mark_domain_dirty(2);
    }

    pub fn mark_system_dirty(&mut self) {
        self.mark_domain_dirty(3);
    }

    pub fn mark_battery_dirty(&mut self) {
        self.mark_domain_dirty(4);
    }

    pub fn mark_apps_dirty(&mut self) {
        self.mark_domain_dirty(5);
    }

    /// Re-seal a block's CRC after a legitimate mutation. Any later stray write
    /// (stack overflow, wild pointer) breaks the seal and is caught by `flush_dirty`.
    #[inline(never)]
    pub fn seal(&mut self, idx: usize) {
        update_raw_crc(self.config_bytes_mut(idx));
    }

    fn seal_all(&mut self) {
        for i in 0..CONFIG_METAS.len() {
            self.seal(i);
        }
    }

    #[inline(never)]
    pub fn sanitize(&mut self) -> u8 {
        let mut repaired = 0u8;

        if self.system.version != SystemConfig::VERSION {
            self.system = DEFAULT_MASTER_CONFIG.system;
            repaired |= DIRTY_SYSTEM;
        }
        if self.system.model > 2 || self.system.uart_baud > 1 || self.system.cpu_clock > 4 {
            self.system.model = self.system.model.min(2);
            self.system.uart_baud = self.system.uart_baud.min(1);
            self.system.cpu_clock = self.system.cpu_clock.min(4);
            repaired |= DIRTY_SYSTEM;
        }
        let mut is_empty_tag = true;
        for &b in &self.system.tag {
            if b != 0 && b != b' ' && b != b'0' {
                is_empty_tag = false;
                break;
            }
        }
        if is_empty_tag {
            self.system.init_default_tag();
            repaired |= DIRTY_SYSTEM;
        }

        let expected_nav = self.system.nav_layout();
        if !check_bounds(self.keys.as_bytes(), KeyConfig::VERSION, &KEY_RULES) {
            self.keys = DEFAULT_MASTER_CONFIG.keys;
            repaired |= DIRTY_KEYS;
        }
        if self.keys.nav_layout != expected_nav {
            self.keys.nav_layout = expected_nav;
            repaired |= DIRTY_KEYS;
        }

        let batt_invalid = self.battery.version != BatteryConfig::VERSION
            || !check_bounds(self.battery.as_bytes(), BatteryConfig::VERSION, &BATT_RULES)
            || self.battery.calib_tweak_mv < -500
            || self.battery.calib_tweak_mv > 500;
        if batt_invalid {
            self.battery = DEFAULT_MASTER_CONFIG.battery;
            repaired |= DIRTY_BATTERY;
        }
        if self.system.is_k1() {
            if self.battery.battery_type < 3 || self.battery.battery_type > 5 {
                self.battery.battery_type = 3;
                repaired |= DIRTY_BATTERY;
            }
        } else if self.battery.battery_type >= 3 {
            self.battery.battery_type = 0;
            repaired |= DIRTY_BATTERY;
        }

        if !check_bounds(
            self.display.as_bytes(),
            DisplayConfig::VERSION,
            &DISPLAY_RULES,
        ) || self.display.backlight_max == 0
        {
            self.display = DEFAULT_MASTER_CONFIG.display;
            repaired |= DIRTY_DISPLAY;
        }

        if !check_bounds(self.audio.as_bytes(), AudioConfig::VERSION, &AUDIO_RULES) {
            self.audio = DEFAULT_MASTER_CONFIG.audio;
            repaired |= DIRTY_AUDIO;
        }

        if !check_bounds(self.apps.as_bytes(), AppsConfig::VERSION, &APPS_RULES)
            || self.apps.daemon_budget_ms < 2
            || self.apps.hook_budget_ms < 2
            || self.apps.zone_b_cache_kb == 0
        {
            self.apps = DEFAULT_MASTER_CONFIG.apps;
            repaired |= DIRTY_APPS;
        }

        repaired
    }

    pub fn config_bytes(&self, idx: usize) -> &[u8] {
        match idx {
            0 => self.display.as_bytes(),
            1 => self.audio.as_bytes(),
            2 => self.keys.as_bytes(),
            3 => self.system.as_bytes(),
            4 => self.battery.as_bytes(),
            _ => self.apps.as_bytes(),
        }
    }

    pub fn config_bytes_mut(&mut self, idx: usize) -> &mut [u8] {
        match idx {
            0 => self.display.as_bytes_mut(),
            1 => self.audio.as_bytes_mut(),
            2 => self.keys.as_bytes_mut(),
            3 => self.system.as_bytes_mut(),
            4 => self.battery.as_bytes_mut(),
            _ => self.apps.as_bytes_mut(),
        }
    }

    pub fn load_into(&mut self, storage: &mut SystemStorage) {
        for (i, meta) in CONFIG_METAS.iter().enumerate() {
            load_or_init_raw(self.config_bytes_mut(i), meta.version, meta.path, storage);
        }
        let repaired = self.sanitize();
        self.seal_all();
        if repaired != 0 {
            self.dirty |= repaired;
            let _ = self.flush_dirty(storage);
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty != 0
    }

    /// Persist dirty blocks to storage atomically with CRC32 protection.
    pub fn flush_dirty(&mut self, storage: &mut SystemStorage) -> Result<(), ConfigError> {
        if self.dirty == 0 {
            return Ok(());
        }
        let _ = self.sanitize();
        let mut first_err = None;
        for (i, meta) in CONFIG_METAS.iter().enumerate() {
            let flag = 1u8 << i;
            if (self.dirty & flag) == 0 {
                continue;
            }
            if !is_raw_valid(self.config_bytes(i), meta.version) {
                load_or_init_raw(self.config_bytes_mut(i), meta.version, meta.path, storage);
                self.seal(i);
                self.dirty &= !flag;
                continue;
            }
            let buf = self.config_bytes_mut(i);
            if let Err(e) = save_config_raw(storage, meta.path, buf) {
                first_err = first_err.or(Some(e));
            } else {
                self.dirty &= !flag;
            }
        }
        if let Some(err) = first_err {
            Err(err)
        } else {
            Ok(())
        }
    }

    pub fn save_all(&mut self, storage: &mut SystemStorage) -> Result<(), ConfigError> {
        self.sanitize();
        self.seal_all();
        self.dirty = 0x3F;
        self.flush_dirty(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_config_blocks_valid_and_deterministic_crc() {
        let mut a = MasterConfig::DEFAULT;
        a.system.tag = *b"TESTTAG ";
        a.battery.battery_type = 3;
        let mut b = a;
        assert_eq!(a.sanitize(), 0);
        for i in 0..6 {
            a.seal(i);
            b.seal(i);
            assert_eq!(a.config_bytes(i), b.config_bytes(i));
            assert!(is_raw_valid(a.config_bytes(i), CONFIG_METAS[i].version));
        }
    }

    #[test]
    fn test_apps_config_v2_bounds_and_custom_startup_allowed() {
        let mut cfg = MasterConfig::DEFAULT;
        cfg.system.tag = *b"TESTTAG ";
        cfg.battery.battery_type = 3;
        cfg.apps.startup_app = 2; // Custom KRX (startup_hash)
        cfg.apps.startup_hash = 0xDEAD_BEEF;
        cfg.apps.autostart_hashes[0] = 0x1234_5678;
        cfg.apps.autostart_states[0] = apps::AUTOSTART_STATE_ENABLED;
        assert_eq!(cfg.sanitize(), 0);

        cfg.apps.daemon_budget_ms = 0; // Below 2 ms minimum -> repaired
        assert_ne!(cfg.sanitize() & DIRTY_APPS, 0);
        assert_eq!(
            cfg.apps.daemon_budget_ms,
            AppsConfig::DEFAULT.daemon_budget_ms
        );
    }
}
