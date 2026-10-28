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

pub fn update_raw_crc(buf: &mut [u8]) {
    let len = buf.len();
    if len > 4 {
        let crc = crypto::crc::CrcEngine::crc32(&buf[..len - 4]);
        buf[len - 4..].copy_from_slice(&crc.to_le_bytes());
    }
}

pub fn is_raw_valid(buf: &[u8], expected_version: u8) -> bool {
    let len = buf.len();
    if len <= 4 || buf[0] != expected_version {
        return false;
    }
    let calc_crc = crypto::crc::CrcEngine::crc32(&buf[..len - 4]);
    let stored_crc = u32::from_le_bytes(buf[len - 4..].try_into().unwrap_or([0; 4]));
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

pub const HW_DIRTY_KEYS: u8 = 1 << 0;
pub const HW_DIRTY_AUDIO: u8 = 1 << 1;
pub const HW_DIRTY_DISPLAY: u8 = 1 << 2;
pub const HW_DIRTY_BATTERY: u8 = 1 << 3;
pub const HW_DIRTY_SYSTEM: u8 = 1 << 4;

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

pub fn load_or_init_raw(buf: &mut [u8], version: u8, path: &str, storage: &mut SystemStorage) {
    let mut temp = [0u8; 48];
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
    flag: u8,
    version: u8,
    path: &'static str,
}

static CONFIG_METAS: [ConfigMeta; 6] = [
    ConfigMeta {
        flag: DIRTY_DISPLAY,
        version: DisplayConfig::VERSION,
        path: DisplayConfig::PATH,
    },
    ConfigMeta {
        flag: DIRTY_AUDIO,
        version: AudioConfig::VERSION,
        path: AudioConfig::PATH,
    },
    ConfigMeta {
        flag: DIRTY_KEYS,
        version: KeyConfig::VERSION,
        path: KeyConfig::PATH,
    },
    ConfigMeta {
        flag: DIRTY_SYSTEM,
        version: SystemConfig::VERSION,
        path: SystemConfig::PATH,
    },
    ConfigMeta {
        flag: DIRTY_BATTERY,
        version: BatteryConfig::VERSION,
        path: BatteryConfig::PATH,
    },
    ConfigMeta {
        flag: DIRTY_APPS,
        version: AppsConfig::VERSION,
        path: AppsConfig::PATH,
    },
];

impl MasterConfig {
    pub const fn new() -> Self {
        let sys = SystemConfig::new();
        let mut keys = KeyConfig::new();
        keys.nav_layout = sys.nav_layout();
        Self {
            display: DisplayConfig::new(),
            audio: AudioConfig::new(),
            keys,
            system: sys,
            battery: BatteryConfig::new(),
            apps: AppsConfig::new(),
            dirty: 0,
            hw_dirty: 0,
        }
    }

    #[inline(never)]
    pub unsafe fn init_in_place(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
            (*ptr).display = DisplayConfig::new();
            (*ptr).audio = AudioConfig::new();
            let sys = SystemConfig::new();
            let mut keys = KeyConfig::new();
            keys.nav_layout = sys.nav_layout();
            (*ptr).keys = keys;
            (*ptr).system = sys;
            (*ptr).battery = BatteryConfig::new();
            (*ptr).apps = AppsConfig::new();
        }
    }

    pub fn is_k1(&self) -> bool {
        self.system.is_k1()
    }

    pub fn nav_layout(&self) -> u8 {
        self.system.nav_layout()
    }

    pub fn mark_audio_dirty(&mut self) {
        self.dirty |= DIRTY_AUDIO;
        self.hw_dirty |= HW_DIRTY_AUDIO;
        self.seal(1);
    }

    pub fn mark_display_dirty(&mut self) {
        self.dirty |= DIRTY_DISPLAY;
        self.hw_dirty |= HW_DIRTY_DISPLAY;
        self.seal(0);
    }

    pub fn mark_keys_dirty(&mut self) {
        self.dirty |= DIRTY_KEYS;
        self.hw_dirty |= HW_DIRTY_KEYS;
        self.seal(2);
    }

    pub fn mark_battery_dirty(&mut self) {
        self.dirty |= DIRTY_BATTERY;
        self.hw_dirty |= HW_DIRTY_BATTERY;
        self.seal(4);
    }

    pub fn mark_system_dirty(&mut self) {
        self.dirty |= DIRTY_SYSTEM;
        self.seal(3);
    }

    /// Re-seal a block's CRC after a legitimate mutation. Any later stray write
    /// (stack overflow, wild pointer) breaks the seal and is caught by `flush_dirty`.
    #[inline(never)]
    fn seal(&mut self, idx: usize) {
        update_raw_crc(self.config_bytes_mut(idx));
    }

    fn seal_all(&mut self) {
        for i in 0..CONFIG_METAS.len() {
            self.seal(i);
        }
    }

    pub fn sanitize(&mut self) -> u8 {
        let mut repaired = 0u8;

        if self.system.version != SystemConfig::VERSION {
            self.system = SystemConfig::new();
            repaired |= DIRTY_SYSTEM;
        }
        if self.system.model > 2 {
            self.system.model = 0;
            repaired |= DIRTY_SYSTEM;
        }
        if self.system.uart_baud > 1 {
            self.system.uart_baud = 0;
            repaired |= DIRTY_SYSTEM;
        }
        let is_empty_tag = self.system.tag[0] == 0
            || &self.system.tag == b"00000000"
            || self
                .system
                .tag
                .iter()
                .all(|&b| b == 0 || b == b' ' || b == b'0');
        if is_empty_tag {
            self.system.init_default_tag();
            repaired |= DIRTY_SYSTEM;
        }

        if self.keys.version != KeyConfig::VERSION {
            self.keys = KeyConfig::new();
            repaired |= DIRTY_KEYS;
        }
        let expected_nav = self.system.nav_layout();
        if self.keys.nav_layout != expected_nav {
            self.keys.nav_layout = expected_nav;
            repaired |= DIRTY_KEYS;
        }
        if self.keys.auto_lock > 5
            || self.keys.f_key_timeout > 3
            || self.keys.fast_scroll > 2
            || self.keys.hold_delay > 3
            || self.keys.repeat_rate > 2
            || self.keys.ptt_mode > 2
        {
            let mut def = KeyConfig::new();
            def.nav_layout = expected_nav;
            self.keys = def;
            repaired |= DIRTY_KEYS;
        }

        if self.battery.version != BatteryConfig::VERSION {
            self.battery = BatteryConfig::new();
            repaired |= DIRTY_BATTERY;
        }
        if self.system.is_k1() {
            if self.battery.battery_type < 3 || self.battery.battery_type > 5 {
                self.battery.battery_type = 3;
                repaired |= DIRTY_BATTERY;
            }
        } else if self.battery.battery_type == 3
            || self.battery.battery_type == 4
            || self.battery.battery_type > 5
        {
            self.battery.battery_type = 0;
            repaired |= DIRTY_BATTERY;
        }
        if self.battery.battery_format > 8
            || self.battery.power_profile > 3
            || self.battery.auto_power_off_min > 5
            || self.battery.charging_detect > 1
            || self.battery.cpu_clock > 1
            || self.battery.calib_tweak_mv < -500
            || self.battery.calib_tweak_mv > 500
        {
            let btype = self.battery.battery_type;
            self.battery = BatteryConfig::new();
            self.battery.battery_type = btype;
            repaired |= DIRTY_BATTERY;
        }

        let d = &mut self.display;
        let def = DisplayConfig::new();
        if d.version != DisplayConfig::VERSION {
            *d = def;
            repaired |= DIRTY_DISPLAY;
        }
        if d.backlight_mode > 5 {
            d.backlight_mode = def.backlight_mode;
            repaired |= DIRTY_DISPLAY;
        }
        if d.backlight_max == 0 || d.backlight_max > 10 {
            d.backlight_max = def.backlight_max;
            repaired |= DIRTY_DISPLAY;
        }
        if d.backlight_min > 10 {
            d.backlight_min = def.backlight_min;
            repaired |= DIRTY_DISPLAY;
        }
        if d.contrast > 100 {
            d.contrast = def.contrast;
            repaired |= DIRTY_DISPLAY;
        }
        if d.inverted > 1 {
            d.inverted = def.inverted;
            repaired |= DIRTY_DISPLAY;
        }
        if d.boot_splash_anim > 3 {
            d.boot_splash_anim = def.boot_splash_anim;
            repaired |= DIRTY_DISPLAY;
        }
        if d.boot_splash_mode > 1 {
            d.boot_splash_mode = def.boot_splash_mode;
            repaired |= DIRTY_DISPLAY;
        }
        if d.boot_splash_duration > 4 {
            d.boot_splash_duration = def.boot_splash_duration;
            repaired |= DIRTY_DISPLAY;
        }
        if d.shading_mode > 2 {
            d.shading_mode = def.shading_mode;
            repaired |= DIRTY_DISPLAY;
        }
        if d.ui_animations > 1 {
            d.ui_animations = def.ui_animations;
            repaired |= DIRTY_DISPLAY;
        }
        if d.refresh_rate > 3 {
            d.refresh_rate = def.refresh_rate;
            repaired |= DIRTY_DISPLAY;
        }

        if self.audio.version != AudioConfig::VERSION
            || self.audio.volume > 10
            || self.audio.key_beep > 5
            || self.audio.mic_gain > 31
            || self.audio.roger_beep > 1
            || self.audio.beep_gain > 10
            || self.audio.boot_jingle > 4
            || self.audio.audio_profile > 4
        {
            self.audio = AudioConfig::new();
            repaired |= DIRTY_AUDIO;
        }

        if self.apps.version != AppsConfig::VERSION
            || self.apps.launcher_style > 1
            || self.apps.show_numbers > 1
            || self.apps.show_labels > 1
            || self.apps.startup_app > 1
            || self.apps.vfo_dual_watch > 3
        {
            self.apps = AppsConfig::new();
            repaired |= DIRTY_APPS;
        }

        repaired
    }

    pub fn mark_apps_dirty(&mut self) {
        self.dirty |= DIRTY_APPS;
        self.seal(5);
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

    pub fn load_all(storage: &mut SystemStorage) -> Self {
        let mut cfg = Self::new();
        cfg.load_into(storage);
        cfg
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty != 0
    }

    /// Persist dirty blocks to storage atomically with CRC32 protection.
    pub fn flush_dirty(&mut self, storage: &mut SystemStorage) -> Result<(), ConfigError> {
        if self.dirty == 0 {
            return Ok(());
        }
        let mut first_err = None;
        for (i, meta) in CONFIG_METAS.iter().enumerate() {
            if (self.dirty & meta.flag) == 0 {
                continue;
            }
            if !is_raw_valid(self.config_bytes(i), meta.version) {
                load_or_init_raw(self.config_bytes_mut(i), meta.version, meta.path, storage);
                let _ = self.sanitize();
                self.seal(i);
                self.dirty &= !meta.flag;
                continue;
            }
            let _ = self.sanitize();
            let buf = self.config_bytes_mut(i);
            if let Err(e) = save_config_raw(storage, meta.path, buf) {
                first_err = first_err.or(Some(e));
            } else {
                self.dirty &= !meta.flag;
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
