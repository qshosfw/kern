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
    utils::format::ascii_str(&buf[..len]).unwrap_or("")
}

#[inline(never)]
pub(crate) fn write_cstr(buf: &mut [u8], path: &str) {
    buf.fill(0);
    let copy_len = path.len().min(buf.len());
    buf[..copy_len].copy_from_slice(&path.as_bytes()[..copy_len]);
}

#[inline(never)]
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

#[inline(never)]
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

#[inline(never)]
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

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MasterConfig {
    pub dirty: u8,
    pub hw_dirty: u8,
    pub _pad: [u8; 2],
    pub display: DisplayConfig,
    pub battery: BatteryConfig,
    pub system: SystemConfig,
    pub keys: KeyConfig,
    pub apps: AppsConfig,
    pub audio: AudioConfig,
}

const _: () = {
    assert!(core::mem::size_of::<DisplayConfig>() == 48);
    assert!(core::mem::offset_of!(DisplayConfig, crc32) == 44);
    assert!(core::mem::size_of::<AudioConfig>() == 80);
    assert!(core::mem::offset_of!(AudioConfig, crc32) == 76);
    assert!(core::mem::size_of::<KeyConfig>() == 64);
    assert!(core::mem::offset_of!(KeyConfig, crc32) == 60);
    assert!(core::mem::size_of::<SystemConfig>() == 44);
    assert!(core::mem::offset_of!(SystemConfig, crc32) == 40);
    assert!(core::mem::size_of::<BatteryConfig>() == 20);
    assert!(core::mem::offset_of!(BatteryConfig, crc32) == 16);
    assert!(core::mem::size_of::<AppsConfig>() == 64);
    assert!(core::mem::align_of::<AppsConfig>() == 4);
    assert!(core::mem::offset_of!(AppsConfig, autostart_hashes) == 16);
    assert!(core::mem::offset_of!(AppsConfig, crc32) == 60);
};

#[inline(never)]
pub fn load_or_init_raw(buf: &mut [u8], version: u8, path: &str, storage: &mut SystemStorage) {
    let temp_buf = &mut crate::data::arena::scratch_tail_bytes_mut()[1920..2000];
    let n = buf.len().min(80);
    let loaded = storage
        .read_file(path, &mut temp_buf[..n])
        .is_ok_and(|len| len >= n && is_raw_valid(&temp_buf[..n], version));
    if loaded {
        buf[..n].copy_from_slice(&temp_buf[..n]);
    } else {
        update_raw_crc(buf);
        let _ = save_config_raw(storage, path, buf);
    }
}

#[repr(C)]
struct ConfigMeta {
    offset: u16,
    size: u8,
    version: u8,
}

utils::static_bytes!(
    static CONFIG_PATHS = b"\
/pref/display.cfg\0\
/pref/audio.cfg\0\
/pref/keys.cfg\0\
/pref/system.cfg\0\
/pref/battery.cfg\0\
/pref/apps.cfg\0";
);

#[inline(always)]
fn config_path(idx: usize) -> &'static str {
    utils::format::nth_cstr(&CONFIG_PATHS, idx as u8)
}

static CONFIG_METAS: [ConfigMeta; 6] = [
    ConfigMeta {
        offset: core::mem::offset_of!(MasterConfig, display) as u16,
        size: core::mem::size_of::<DisplayConfig>() as u8,
        version: DisplayConfig::VERSION,
    },
    ConfigMeta {
        offset: core::mem::offset_of!(MasterConfig, audio) as u16,
        size: core::mem::size_of::<AudioConfig>() as u8,
        version: AudioConfig::VERSION,
    },
    ConfigMeta {
        offset: core::mem::offset_of!(MasterConfig, keys) as u16,
        size: core::mem::size_of::<KeyConfig>() as u8,
        version: KeyConfig::VERSION,
    },
    ConfigMeta {
        offset: core::mem::offset_of!(MasterConfig, system) as u16,
        size: core::mem::size_of::<SystemConfig>() as u8,
        version: SystemConfig::VERSION,
    },
    ConfigMeta {
        offset: core::mem::offset_of!(MasterConfig, battery) as u16,
        size: core::mem::size_of::<BatteryConfig>() as u8,
        version: BatteryConfig::VERSION,
    },
    ConfigMeta {
        offset: core::mem::offset_of!(MasterConfig, apps) as u16,
        size: core::mem::size_of::<AppsConfig>() as u8,
        version: AppsConfig::VERSION,
    },
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
        _pad: [0; 2],
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

        for (i, meta) in CONFIG_METAS.iter().enumerate() {
            let tweak = self.battery.calib_tweak_mv;
            if self.config_bytes(i)[0] != meta.version || (i == 4 && !(-500..=500).contains(&tweak))
            {
                let def = DEFAULT_MASTER_CONFIG.config_bytes(i);
                self.config_bytes_mut(i).copy_from_slice(def);
                repaired |= 1u8 << i;
            }
        }
        // Every declared property is held inside its bounds; a stray byte falls
        // back to its own default without discarding the rest of the block.
        for p in &::abi::prop::TABLE {
            if !p.admits(self.prop_get(p.id)) {
                self.config_bytes_mut(p.domain())[p.offset()] =
                    DEFAULT_MASTER_CONFIG.prop_get(p.id);
                repaired |= 1u8 << p.domain();
            }
        }
        if !cfg!(feature = "overclock_72mhz") && self.system.cpu_clock > 3 {
            self.system.cpu_clock = 3;
            repaired |= DIRTY_SYSTEM;
        }
        if self
            .system
            .tag
            .iter()
            .all(|&b| b == 0 || b == b' ' || b == b'0')
        {
            self.system.init_default_tag();
            repaired |= DIRTY_SYSTEM;
        }

        let expected_nav = self.system.nav_layout();
        if self.keys.nav_layout != expected_nav {
            self.keys.nav_layout = expected_nav;
            repaired |= DIRTY_KEYS;
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

        if repaired != 0 {
            for i in 0..CONFIG_METAS.len() {
                if (repaired & (1u8 << i)) != 0 {
                    self.seal(i);
                }
            }
        }

        repaired
    }

    /// Current value of property `id` (`abi::prop`); 0 for an undeclared one.
    #[inline(never)]
    pub fn prop_get(&self, id: u8) -> u8 {
        let dom = (id >> 5) as usize;
        if dom >= CONFIG_METAS.len() {
            return 0;
        }
        self.config_bytes(dom)
            .get((id & 0x1F) as usize)
            .copied()
            .unwrap_or(0)
    }

    /// Store property `id` if it is declared and `val` is inside its bounds.
    /// The owning domain is sealed and queued for hardware apply and persistence.
    #[inline(never)]
    pub fn prop_set(&mut self, id: u8, val: u8) -> bool {
        let Some(p) = ::abi::prop::find(id) else {
            return false;
        };
        if !p.admits(val) {
            return false;
        }
        let byte = &mut self.config_bytes_mut(p.domain())[p.offset()];
        if *byte != val {
            *byte = val;
            self.mark_domain_dirty(p.domain());
        }
        true
    }

    #[inline(never)]
    pub fn config_bytes(&self, idx: usize) -> &[u8] {
        let m = &CONFIG_METAS[idx.min(5)];
        unsafe {
            core::slice::from_raw_parts(
                (self as *const Self as *const u8).add(m.offset as usize),
                m.size as usize,
            )
        }
    }

    #[inline(always)]
    pub fn config_bytes_mut(&mut self, idx: usize) -> &mut [u8] {
        let s = self.config_bytes(idx);
        unsafe { core::slice::from_raw_parts_mut(s.as_ptr() as *mut u8, s.len()) }
    }

    pub fn load_into(&mut self, storage: &mut SystemStorage) {
        for (i, meta) in CONFIG_METAS.iter().enumerate() {
            load_or_init_raw(
                self.config_bytes_mut(i),
                meta.version,
                config_path(i),
                storage,
            );
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
    #[inline(never)]
    pub fn flush_dirty(&mut self, storage: &mut SystemStorage) -> Result<(), ConfigError> {
        if self.dirty == 0 {
            return Ok(());
        }
        let repaired = self.sanitize();
        self.dirty |= repaired;
        let mut first_err = None;
        for (i, meta) in CONFIG_METAS.iter().enumerate() {
            let flag = 1u8 << i;
            if (self.dirty & flag) == 0 {
                continue;
            }
            let path = config_path(i);
            if !is_raw_valid(self.config_bytes(i), meta.version) {
                load_or_init_raw(self.config_bytes_mut(i), meta.version, path, storage);
                self.seal(i);
                self.dirty &= !flag;
                continue;
            }
            let buf = self.config_bytes_mut(i);
            if let Err(e) = save_config_raw(storage, path, buf) {
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

    #[test]
    fn test_keys_and_audio_upper_bounds_and_repair_sealing() {
        let mut cfg = MasterConfig::DEFAULT;
        cfg.system.tag = *b"TESTTAG ";
        cfg.battery.battery_type = 3;
        cfg.keys.f_key_timeout = 4; // 5s option
        cfg.keys.repeat_rate = 3; // Slow 120ms option
        cfg.audio.key_beep = 6; // Custom beep
        cfg.audio.roger_beep = 12; // CustomFile roger
        assert_eq!(cfg.sanitize(), 0);
        assert_eq!(cfg.keys.f_key_timeout, 4);
        assert_eq!(cfg.keys.repeat_rate, 3);
        assert_eq!(cfg.audio.key_beep, 6);
        assert_eq!(cfg.audio.roger_beep, 12);

        // Model switch repairs keys.nav_layout and battery.battery_type and must re-seal them.
        cfg.seal_all();
        cfg.system.model = 1; // UV-K5 V3
        let repaired = cfg.sanitize();
        assert_ne!(repaired & (DIRTY_KEYS | DIRTY_BATTERY), 0);
        assert!(is_raw_valid(cfg.config_bytes(2), CONFIG_METAS[2].version));
        assert!(is_raw_valid(cfg.config_bytes(4), CONFIG_METAS[4].version));
    }

    #[test]
    fn test_property_registry_bounds_seal_and_readable_display() {
        use ::abi::prop;
        let mut cfg = MasterConfig::DEFAULT;
        cfg.system.tag = *b"TESTTAG ";
        cfg.battery.battery_type = 3;
        cfg.seal_all();
        cfg.dirty = 0;
        cfg.hw_dirty = 0;

        assert!(cfg.prop_set(prop::CONTRAST, 60));
        assert_eq!(cfg.prop_get(prop::CONTRAST), 60);
        assert_eq!(cfg.dirty, DIRTY_DISPLAY);
        assert_eq!(cfg.hw_dirty, HW_DIRTY_DISPLAY);
        assert!(is_raw_valid(cfg.config_bytes(0), CONFIG_METAS[0].version));

        // Out of bounds, undeclared offsets and the version byte are all refused.
        assert!(!cfg.prop_set(prop::BACKLIGHT_MAX, 0));
        assert!(!cfg.prop_set(prop::VOLUME, 11));
        assert!(!cfg.prop_set(0, 9));
        assert!(!cfg.prop_set(0xFF, 1));
        assert_eq!(cfg.prop_get(0xFF), 0);
        assert_eq!(
            cfg.display.backlight_max,
            DisplayConfig::DEFAULT.backlight_max
        );

        // A corrupt byte is repaired alone; the rest of the block survives.
        cfg.display.backlight_max = 0;
        assert_ne!(cfg.sanitize() & DIRTY_DISPLAY, 0);
        assert_eq!(
            cfg.display.backlight_max,
            DisplayConfig::DEFAULT.backlight_max
        );
        assert_eq!(cfg.display.contrast, 60);

        // With no Settings app installed the defaults must stay readable.
        let d = MasterConfig::DEFAULT;
        assert!(d.display.backlight_max >= 5 && d.display.contrast >= 40);
        for p in &prop::TABLE {
            assert!(
                p.admits(d.prop_get(p.id)),
                "default out of bounds: {:#x}",
                p.id
            );
        }
    }

    #[test]
    fn test_key_config_v3_64b_and_ctx_rules() {
        let mut cfg = KeyConfig::new();
        assert_eq!(core::mem::size_of::<KeyConfig>(), 64);
        assert_eq!(
            crate::apps::shortcut::resolve_slot(&cfg, 2, crate::apps::shortcut::CTX_GLOBAL),
            cfg.actions[2]
        );
        cfg.ctx_rules[0] = keys::CtxRule {
            slot: 2,
            ctx: crate::apps::shortcut::CTX_VFO,
            action: crate::apps::shortcut::TARGET_HOME,
        };
        assert_eq!(
            crate::apps::shortcut::resolve_slot(&cfg, 2, crate::apps::shortcut::CTX_VFO),
            crate::apps::shortcut::TARGET_HOME
        );
        assert_eq!(
            crate::apps::shortcut::resolve_slot(&cfg, 2, crate::apps::shortcut::CTX_HOME),
            cfg.actions[2]
        );
    }
}
