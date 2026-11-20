//! Declarative Settings Abstractions: Page IDs, Metrics, Choices, and Field Enums.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageId {
    Root = 0,
    Display,
    Audio,
    Buttons,
    Battery,
    Device,
    #[cfg(feature = "passcode")]
    Security,
    System,
    ButtonsTweaks,
    ButtonsShortcuts,
    BatteryInfo,
    BatteryTweaks,
    #[cfg(feature = "settings_info")]
    SystemInfo,
    #[cfg(feature = "filesystem")]
    SystemStorage,
    Apps,
    BootSplash,
    #[cfg(feature = "settings_calib")]
    SystemCalibration,
    #[cfg(feature = "settings_debug")]
    SystemDebug,
    AppPrefs,
}

impl PageId {
    pub const COUNT: usize = PageId::AppPrefs as usize + 1;

    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if (val as usize) < Self::COUNT {
            Some(unsafe { core::mem::transmute::<u8, Self>(val) })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DestructiveLevel {
    /// Non-destructive settings edit or picker.
    None = 0,
    /// Restarts device out of user session (e.g. DFU bootloader hold).
    Reboot = 1,
    /// Erases user settings/files while keeping calibration intact.
    UserDataLoss = 2,
    /// Erases calibration and/or raw flash sectors.
    FactoryDataWipe = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionId {
    ChooseModel = 0,
    #[cfg(feature = "text_input")]
    EditOwner,
    #[cfg(feature = "text_input")]
    EditTag,
    #[cfg(feature = "text_input")]
    EditRadioId,
    #[cfg(feature = "settings_debug")]
    ClearLogs,
    #[cfg(feature = "filesystem")]
    FormatKeepCalib,
    #[cfg(feature = "filesystem")]
    FormatWipeAll,
    ResetSettings,
    RebootFirmware,
    #[cfg(feature = "filesystem")]
    DeletePackage,
}

impl ActionId {
    #[cfg(feature = "filesystem")]
    pub const COUNT: usize = ActionId::DeletePackage as usize + 1;
    #[cfg(not(feature = "filesystem"))]
    pub const COUNT: usize = ActionId::RebootFirmware as usize + 1;

    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if (val as usize) < Self::COUNT {
            Some(unsafe { core::mem::transmute::<u8, Self>(val) })
        } else {
            None
        }
    }

    pub const fn destructive_level(&self) -> DestructiveLevel {
        match self {
            #[cfg(feature = "filesystem")]
            ActionId::FormatKeepCalib | ActionId::DeletePackage => DestructiveLevel::UserDataLoss,
            #[cfg(not(feature = "filesystem"))]
            ActionId::FormatKeepCalib => DestructiveLevel::UserDataLoss,
            ActionId::ResetSettings => DestructiveLevel::UserDataLoss,
            #[cfg(feature = "filesystem")]
            ActionId::FormatWipeAll => DestructiveLevel::FactoryDataWipe,
            ActionId::RebootFirmware => DestructiveLevel::Reboot,
            _ => DestructiveLevel::None,
        }
    }
}

utils::static_bytes!(
    static RANGE_UNITS = b"\0dB\0%\0mV\0";
);

/// Unit suffix of a range row, an index into `RANGE_UNITS`.
pub const UNIT_NONE: u8 = 0;
pub const UNIT_DB: u8 = 1;
pub const UNIT_PERCENT: u8 = 2;
pub const UNIT_MV: u8 = 3;

/// Bounds of the range row editing `field`: `(min, max, step)` from `abi::prop`.
#[inline(never)]
pub fn range_of(field: SettingField) -> (i16, i16, i16) {
    match ::abi::prop::find(field.0) {
        Some(p) => (p.min as i16, p.max as i16, p.step as i16),
        None => (-500, 500, 10),
    }
}

#[inline(always)]
pub fn range_unit(unit: u8) -> &'static str {
    utils::format::nth_cstr(&RANGE_UNITS, unit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChoiceId {
    BacklightTime = 0,
    Shading = 1,
    Animation = 2,
    KeyBeep = 3,
    BootJingle = 4,
    AudioProfile = 5,
    PttMode = 6,
    AutoLock = 7,
    FKeyTimeout = 8,
    HoldDelay = 9,
    RepeatRate = 10,
    BatteryFormat = 11,
    AutoPowerOff = 12,
    PowerProfile = 13,
    SplashAnim = 14,
    SplashDuration = 15,
    LauncherStyle = 16,
    StartupApp = 17,
    CpuClock = 18,
    #[cfg(feature = "uart")]
    UartBaud = 19,
}

pub const fn packed_strs_len(items: &[&str], nul_sep: bool) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < items.len() {
        n += items[i].len() + if nul_sep { 1 } else { 0 };
        i += 1;
    }
    n
}

pub const fn pack_strs<const N: usize>(items: &[&str], nul_sep: bool) -> [u8; N] {
    let mut out = [0u8; N];
    let mut pos = 0;
    let mut i = 0;
    while i < items.len() {
        let b = items[i].as_bytes();
        let mut j = 0;
        while j < b.len() {
            out[pos] = b[j];
            pos += 1;
            j += 1;
        }
        if nul_sep {
            out[pos] = 0;
            pos += 1;
        }
        i += 1;
    }
    out
}

pub const fn pack_offsets<const M: usize>(items: &[&str]) -> [u16; M] {
    let mut out = [0u16; M];
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < items.len() {
        out[i] = pos as u16;
        pos += items[i].len();
        i += 1;
    }
    out[items.len()] = pos as u16;
    out
}

const RAW_CHOICES: &[&str] = &[
    super::display::BACKLIGHT_TIME_OPTIONS,
    super::display::SHADING_OPTIONS,
    super::display::ANIMATION_OPTIONS,
    super::audio::KEY_BEEP_OPTIONS,
    super::audio::BOOT_JINGLE_OPTIONS,
    super::audio::AUDIO_PROFILE_OPTIONS,
    super::buttons::PTT_MODE_OPTIONS,
    super::buttons::AUTO_LOCK_OPTIONS,
    super::buttons::F_KEY_TIMEOUT_OPTIONS,
    super::buttons::HOLD_DELAY_OPTIONS,
    super::buttons::REPEAT_RATE_OPTIONS,
    super::battery::BATTERY_FORMAT_OPTIONS,
    super::battery::AUTO_POWER_OFF_OPTIONS,
    super::battery::POWER_PROFILE_OPTIONS,
    super::device::BOOT_ANIM_OPTIONS,
    super::device::SPLASH_TIME_OPTIONS,
    super::apps::LAUNCHER_STYLE_OPTIONS,
    super::apps::STARTUP_APP_OPTIONS,
    super::system::CPU_CLOCK_OPTIONS,
    #[cfg(feature = "uart")]
    super::system::UART_BAUD_OPTIONS,
];

const CHOICE_BLOB_LEN: usize = packed_strs_len(RAW_CHOICES, false);
static CHOICE_BLOB: [u8; CHOICE_BLOB_LEN] = pack_strs(RAW_CHOICES, false);
static CHOICE_OFFS: [u16; RAW_CHOICES.len() + 1] = pack_offsets(RAW_CHOICES);

#[inline(always)]
pub fn choice_options(idx: u8) -> Option<&'static str> {
    let i = idx as usize;
    if i + 1 >= CHOICE_OFFS.len() {
        return None;
    }
    let start = CHOICE_OFFS[i] as usize;
    let end = CHOICE_OFFS[i + 1] as usize;
    // SAFETY: `CHOICE_BLOB` is concatenated purely from `&'static str` UTF-8 literals.
    Some(unsafe { core::str::from_utf8_unchecked(&CHOICE_BLOB[start..end]) })
}

#[cfg(feature = "filesystem")]
pub use super::asset_chooser::AssetId;

#[derive(Clone, Copy)]
pub struct MenuFrame {
    pub page: PageId,
    pub selected_idx: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MetricId {
    // Battery
    BatteryVoltage,
    BatteryPercentage,
    BatteryStatus,
    BatteryVdda,
    BatteryCalibRef,
    BatteryCoreTemp,

    // Storage
    #[cfg(feature = "filesystem")]
    StorageUsed,
    #[cfg(feature = "filesystem")]
    StorageFree,
    #[cfg(feature = "filesystem")]
    StorageFiles,
    #[cfg(feature = "filesystem")]
    StorageWear,
    #[cfg(feature = "filesystem")]
    StorageDeleted,

    // Calibration
    #[cfg(feature = "settings_calib")]
    CalibSig,
    #[cfg(feature = "settings_calib")]
    CalibXtal,
    #[cfg(feature = "settings_calib")]
    CalibBatt,
    #[cfg(feature = "settings_calib")]
    CalibLna,
    #[cfg(feature = "settings_calib")]
    CalibMix,
    #[cfg(feature = "settings_calib")]
    CalibVol,

    // Debug Diagnostics
    #[cfg(feature = "settings_debug")]
    DebugStackMargin,
    #[cfg(feature = "settings_debug")]
    DebugStackPeak,
    #[cfg(feature = "settings_debug")]
    DebugCrashes,
    #[cfg(feature = "settings_debug")]
    DebugSwd,
    #[cfg(feature = "settings_debug")]
    DebugRdp,
    #[cfg(feature = "settings_debug")]
    DebugDevKey,
    #[cfg(feature = "settings_debug")]
    DebugMkHash,

    // Device Identity
    DeviceOwner,
    DeviceTag,
    DeviceRadioId,
}

impl MetricId {
    pub const COUNT: usize = MetricId::DeviceRadioId as usize + 1;

    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if (val as usize) < Self::COUNT {
            Some(unsafe { core::mem::transmute::<u8, Self>(val) })
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SettingField(pub u8);

impl SettingField {
    #[allow(non_upper_case_globals)]
    pub const None: Self = Self(0xFF);
    #[allow(non_upper_case_globals)]
    pub const CalibTweak: Self = Self(0xFE);

    // Display (domain 0)
    pub const BACKLIGHT_TIME: Self = Self(::abi::prop::BACKLIGHT_TIME);
    pub const BACKLIGHT_MAX: Self = Self(::abi::prop::BACKLIGHT_MAX);
    pub const BACKLIGHT_MIN: Self = Self(::abi::prop::BACKLIGHT_MIN);
    pub const CONTRAST: Self = Self(::abi::prop::CONTRAST);
    pub const INVERT_PIXELS: Self = Self(::abi::prop::INVERT_PIXELS);
    pub const BOOT_SPLASH_ANIM: Self = Self(::abi::prop::BOOT_SPLASH_ANIM);
    pub const BOOT_SPLASH_DURATION: Self = Self(::abi::prop::BOOT_SPLASH_DURATION);
    pub const SHADING_MODE: Self = Self(::abi::prop::SHADING_MODE);
    pub const UI_ANIMATIONS: Self = Self(::abi::prop::UI_ANIMATIONS);
    pub const REFRESH_RATE: Self = Self(::abi::prop::REFRESH_RATE);

    // Audio (domain 1)
    pub const VOLUME: Self = Self(::abi::prop::VOLUME);
    pub const KEY_BEEP: Self = Self(::abi::prop::KEY_BEEP);
    pub const MIC_GAIN: Self = Self(::abi::prop::MIC_GAIN);
    pub const ROGER_BEEP: Self = Self(::abi::prop::ROGER_BEEP);
    pub const BEEP_GAIN: Self = Self(::abi::prop::BEEP_GAIN);
    pub const BOOT_JINGLE: Self = Self(::abi::prop::BOOT_JINGLE);
    pub const AUDIO_PROFILE: Self = Self(::abi::prop::AUDIO_PROFILE);

    // Keys (domain 2, KeyConfig v3)
    pub const AUTO_LOCK: Self = Self(::abi::prop::AUTO_LOCK);
    pub const F_KEY_TIMEOUT: Self = Self(::abi::prop::F_KEY_TIMEOUT);
    pub const FAST_SCROLL: Self = Self(::abi::prop::FAST_SCROLL);
    pub const HOLD_DELAY: Self = Self(::abi::prop::HOLD_DELAY);
    pub const REPEAT_RATE: Self = Self(::abi::prop::REPEAT_RATE);
    pub const PTT_MODE: Self = Self(::abi::prop::PTT_MODE);
    pub const F_DRAWER: Self = Self(::abi::prop::F_DRAWER);

    // System (domain 3)
    pub const UART_BAUD: Self = Self(::abi::prop::UART_BAUD);
    pub const CPU_CLOCK: Self = Self(::abi::prop::CPU_CLOCK);

    // Battery (domain 4)
    pub const BATTERY_TYPE: Self = Self(::abi::prop::BATTERY_TYPE);
    pub const BATTERY_FORMAT: Self = Self(::abi::prop::BATTERY_FORMAT);
    pub const POWER_PROFILE: Self = Self(::abi::prop::POWER_PROFILE);
    pub const AUTO_POWER_OFF: Self = Self(::abi::prop::AUTO_POWER_OFF);
    pub const CHARGE_DETECT: Self = Self(::abi::prop::CHARGE_DETECT);
    pub const CALIB_TWEAK_MV: Self = Self::CalibTweak;

    // Apps (domain 5)
    pub const LAUNCHER_STYLE: Self = Self(::abi::prop::LAUNCHER_STYLE);
    pub const SHOW_NUMBERS: Self = Self(::abi::prop::SHOW_NUMBERS);
    pub const SHOW_LABELS: Self = Self(::abi::prop::SHOW_LABELS);
    pub const STARTUP_APP: Self = Self(::abi::prop::STARTUP_APP);

    #[inline(always)]
    pub fn get_u8(self, config: &crate::config::MasterConfig) -> u8 {
        config.prop_get(self.0)
    }

    #[inline(always)]
    pub fn get_i16(self, config: &crate::config::MasterConfig) -> i16 {
        if self.0 == Self::CalibTweak.0 {
            config.battery.calib_tweak_mv
        } else {
            self.get_u8(config) as i16
        }
    }

    /// Store `val` through the property registry: bounds, CRC seal, hardware
    /// apply and persistence are queued there, so pages keep no bookkeeping.
    pub fn set_val(self, val: i32, config: &mut crate::config::MasterConfig) -> bool {
        if self.0 != Self::CalibTweak.0 {
            return config.prop_set(self.0, val as u8);
        }
        config.battery.calib_tweak_mv = (val as i16).clamp(-500, 500);
        config.mark_domain_dirty(4);
        true
    }
}

pub const TAG_SUBMENU: u8 = 0;
pub const TAG_CHOICE: u8 = 1;
pub const TAG_BATTERY_TYPE: u8 = 2;
pub const TAG_RANGE: u8 = 3;
pub const TAG_TOGGLE: u8 = 4;
pub const TAG_METRIC: u8 = 5;
pub const TAG_ACTION: u8 = 6;
pub const TAG_ASSET: u8 = 7;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SettingDef {
    pub field: SettingField,
    pub tag: u8,
    pub aux: u8,
}

#[derive(Clone, Copy)]
pub struct PageSpec {
    pub titles: &'static [u8],
    pub defs: &'static [SettingDef],
}

impl PageSpec {
    pub const EMPTY: Self = Self {
        titles: b"",
        defs: &[],
    };

    #[inline(always)]
    pub fn title(&self, idx: usize) -> &'static str {
        utils::format::nth_cstr(self.titles, idx as u8)
    }
}

macro_rules! setting_page {
    ($vis:vis $name:ident = [ $( $(#[$m:meta])* ($title:literal, $def:expr) ),* $(,)? ]) => {
        $vis static $name: crate::apps::settings::page::PageSpec = {
            const RAW: &[&str] = &[ $( $(#[$m])* $title, )* ];
            const LEN: usize = crate::apps::settings::page::packed_strs_len(RAW, true);
            static TITLES: [u8; LEN] = crate::apps::settings::page::pack_strs(RAW, true);
            static DEFS: &[crate::apps::settings::page::SettingDef] = &[ $( $(#[$m])* $def, )* ];
            crate::apps::settings::page::PageSpec {
                titles: &TITLES,
                defs: DEFS,
            }
        };
    };
}
pub(super) use setting_page;

impl SettingDef {
    pub const fn submenu(target: PageId) -> Self {
        Self {
            field: SettingField::None,
            tag: TAG_SUBMENU,
            aux: target as u8,
        }
    }

    pub const fn choice(choice: ChoiceId, field: SettingField) -> Self {
        Self {
            field,
            tag: TAG_CHOICE,
            aux: choice as u8,
        }
    }

    pub const fn range(unit: u8, field: SettingField) -> Self {
        Self {
            field,
            tag: TAG_RANGE,
            aux: unit,
        }
    }

    pub const fn toggle(field: SettingField) -> Self {
        Self {
            field,
            tag: TAG_TOGGLE,
            aux: 0,
        }
    }

    pub const fn metric(metric: MetricId) -> Self {
        Self {
            field: SettingField::None,
            tag: TAG_METRIC,
            aux: metric as u8,
        }
    }

    #[cfg(feature = "filesystem")]
    pub const fn asset(asset: AssetId) -> Self {
        Self {
            field: SettingField::None,
            tag: TAG_ASSET,
            aux: asset as u8,
        }
    }

    pub const fn action(action: ActionId) -> Self {
        Self {
            field: SettingField::None,
            tag: TAG_ACTION,
            aux: action as u8,
        }
    }

    pub const fn battery_type() -> Self {
        Self {
            field: SettingField::BATTERY_TYPE,
            tag: TAG_BATTERY_TYPE,
            aux: 0,
        }
    }
}
