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
    ManageApps,
    AppDetail,
    ManageServices,
    ServiceDetail,
    ShortcutRules,
    ShortcutRule,
}

impl PageId {
    pub const COUNT: usize = PageId::ShortcutRule as usize + 1;

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
    ChooseModel = 1,
    #[cfg(feature = "text_input")]
    EditOwner = 2,
    #[cfg(feature = "text_input")]
    EditTag = 3,
    #[cfg(feature = "text_input")]
    EditRadioId = 4,
    #[cfg(feature = "settings_debug")]
    ClearLogs = 5,
    #[cfg(feature = "filesystem")]
    FormatKeepCalib = 6,
    #[cfg(feature = "filesystem")]
    FormatWipeAll = 7,
    ResetSettings = 10,
    RebootFirmware = 11,
    #[cfg(feature = "filesystem")]
    DeletePackage = 12,
}

impl ActionId {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(ActionId::ChooseModel),
            #[cfg(feature = "text_input")]
            2 => Some(ActionId::EditOwner),
            #[cfg(feature = "text_input")]
            3 => Some(ActionId::EditTag),
            #[cfg(feature = "text_input")]
            4 => Some(ActionId::EditRadioId),
            #[cfg(feature = "settings_debug")]
            5 => Some(ActionId::ClearLogs),
            #[cfg(feature = "filesystem")]
            6 => Some(ActionId::FormatKeepCalib),
            #[cfg(feature = "filesystem")]
            7 => Some(ActionId::FormatWipeAll),
            10 => Some(ActionId::ResetSettings),
            11 => Some(ActionId::RebootFirmware),
            #[cfg(feature = "filesystem")]
            12 => Some(ActionId::DeletePackage),
            _ => None,
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

#[derive(Clone, Copy)]
pub struct RangeDef {
    pub min: i16,
    pub max: i16,
    pub step: i16,
    pub unit: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RangeId {
    Range0_10 = 0,
    Range1_10 = 1,
    MicGain = 2,
    Contrast = 3,
    CalibTweak = 4,
}

pub static RANGES: [RangeDef; 5] = [
    RangeDef {
        min: 0,
        max: 10,
        step: 1,
        unit: "",
    },
    RangeDef {
        min: 1,
        max: 10,
        step: 1,
        unit: "",
    },
    RangeDef {
        min: 0,
        max: 31,
        step: 1,
        unit: "dB",
    },
    RangeDef {
        min: 0,
        max: 100,
        step: 5,
        unit: "%",
    },
    RangeDef {
        min: -500,
        max: 500,
        step: 10,
        unit: "mV",
    },
];

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

pub static CHOICES: &[&'static str] = &[
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

#[cfg(feature = "filesystem")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AssetId {
    Splash = 0,
    Roger = 1,
    Jingle = 2,
}

#[cfg(feature = "filesystem")]
pub static ASSETS: [&'static super::asset_chooser::AssetDescriptor; 3] = [
    &super::device::SPLASH_ASSET,
    &super::audio::ROGER_ASSET,
    &super::audio::JINGLE_ASSET,
];

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

    #[inline(always)]
    pub const fn byte(dom: u8, off: u8) -> Self {
        Self((dom << 5) | (off & 0x1F))
    }

    // Display (domain 0)
    pub const BACKLIGHT_TIME: Self = Self::byte(0, 1);
    pub const BACKLIGHT_MAX: Self = Self::byte(0, 2);
    pub const BACKLIGHT_MIN: Self = Self::byte(0, 3);
    pub const CONTRAST: Self = Self::byte(0, 4);
    pub const INVERT_PIXELS: Self = Self::byte(0, 5);
    pub const BOOT_SPLASH_ANIM: Self = Self::byte(0, 6);
    pub const BOOT_SPLASH_DURATION: Self = Self::byte(0, 8);
    pub const SHADING_MODE: Self = Self::byte(0, 9);
    pub const UI_ANIMATIONS: Self = Self::byte(0, 10);
    pub const REFRESH_RATE: Self = Self::byte(0, 11);

    // Audio (domain 1)
    pub const VOLUME: Self = Self::byte(1, 1);
    pub const KEY_BEEP: Self = Self::byte(1, 2);
    pub const MIC_GAIN: Self = Self::byte(1, 3);
    pub const ROGER_BEEP: Self = Self::byte(1, 4);
    pub const BEEP_GAIN: Self = Self::byte(1, 6);
    pub const BOOT_JINGLE: Self = Self::byte(1, 7);
    pub const AUDIO_PROFILE: Self = Self::byte(1, 8);

    // Keys (domain 2, KeyConfig v3)
    pub const AUTO_LOCK: Self = Self::byte(2, 2);
    pub const F_KEY_TIMEOUT: Self = Self::byte(2, 3);
    pub const FAST_SCROLL: Self = Self::byte(2, 4);
    pub const HOLD_DELAY: Self = Self::byte(2, 5);
    pub const REPEAT_RATE: Self = Self::byte(2, 6);
    pub const PTT_MODE: Self = Self::byte(2, 7);
    pub const F_DRAWER: Self = Self::byte(2, 8);

    // System (domain 3)
    pub const UART_BAUD: Self = Self::byte(3, 2);
    pub const CPU_CLOCK: Self = Self::byte(3, 3);

    // Battery (domain 4)
    pub const BATTERY_TYPE: Self = Self::byte(4, 1);
    pub const BATTERY_FORMAT: Self = Self::byte(4, 2);
    pub const POWER_PROFILE: Self = Self::byte(4, 3);
    pub const AUTO_POWER_OFF: Self = Self::byte(4, 4);
    pub const CHARGE_DETECT: Self = Self::byte(4, 5);
    pub const CALIB_TWEAK_MV: Self = Self::CalibTweak;

    // Apps (domain 5)
    pub const LAUNCHER_STYLE: Self = Self::byte(5, 1);
    pub const SHOW_NUMBERS: Self = Self::byte(5, 2);
    pub const SHOW_LABELS: Self = Self::byte(5, 3);
    pub const STARTUP_APP: Self = Self::byte(5, 4);
    pub const VFO_DUAL_WATCH: Self = Self::byte(5, 5);
    pub const SCANNER_DWELL: Self = Self::byte(5, 6);

    #[inline(never)]
    pub fn get_u8(self, config: &crate::config::MasterConfig) -> u8 {
        if self.0 >= 0xFE {
            0
        } else {
            let dom = (self.0 >> 5) as usize;
            let off = (self.0 & 0x1F) as usize;
            config.config_bytes(dom).get(off).copied().unwrap_or(0)
        }
    }

    #[inline(always)]
    pub fn get_i16(self, config: &crate::config::MasterConfig) -> i16 {
        if self.0 == Self::CalibTweak.0 {
            config.battery.calib_tweak_mv
        } else {
            self.get_u8(config) as i16
        }
    }

    pub fn set_val(self, val: i32, config: &mut crate::config::MasterConfig) -> bool {
        if self.0 == Self::None.0 {
            false
        } else if self.0 == Self::CalibTweak.0 {
            let clamped = (val as i16).clamp(-500, 500);
            if config.battery.calib_tweak_mv != clamped {
                config.battery.calib_tweak_mv = clamped;
                true
            } else {
                false
            }
        } else {
            let dom = (self.0 >> 5) as usize;
            let off = (self.0 & 0x1F) as usize;
            Self::set_byte(config.config_bytes_mut(dom), off, val as u8)
        }
    }

    #[inline(always)]
    fn set_byte(slice: &mut [u8], off: usize, val: u8) -> bool {
        if let Some(target) = slice.get_mut(off) {
            if *target != val {
                *target = val;
                return true;
            }
        }
        false
    }
}

pub const TAG_SUBMENU: u8 = 0;
pub const TAG_CHOICE: u8 = 1;
pub const TAG_BATTERY_TYPE: u8 = 2;
pub const TAG_RANGE: u8 = 3;
pub const TAG_TOGGLE: u8 = 4;
pub const TAG_METRIC: u8 = 5;
pub const TAG_ACTION: u8 = 6;
pub const TAG_INFO: u8 = 7;
pub const TAG_ASSET: u8 = 8;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SettingDef {
    pub title: &'static str,
    pub field: SettingField,
    pub tag: u8,
    pub aux: u8,
}

impl SettingDef {
    pub const fn submenu(title: &'static str, target: PageId) -> Self {
        Self {
            title,
            field: SettingField::None,
            tag: TAG_SUBMENU,
            aux: target as u8,
        }
    }

    pub const fn choice(title: &'static str, choice: ChoiceId, field: SettingField) -> Self {
        Self {
            title,
            field,
            tag: TAG_CHOICE,
            aux: choice as u8,
        }
    }

    pub const fn range(title: &'static str, range: RangeId, field: SettingField) -> Self {
        Self {
            title,
            field,
            tag: TAG_RANGE,
            aux: range as u8,
        }
    }

    pub const fn toggle(title: &'static str, field: SettingField) -> Self {
        Self {
            title,
            field,
            tag: TAG_TOGGLE,
            aux: 0,
        }
    }

    pub const fn metric(title: &'static str, metric: MetricId) -> Self {
        Self {
            title,
            field: SettingField::None,
            tag: TAG_METRIC,
            aux: metric as u8,
        }
    }

    #[cfg(feature = "filesystem")]
    pub const fn asset(title: &'static str, asset: AssetId) -> Self {
        Self {
            title,
            field: SettingField::None,
            tag: TAG_ASSET,
            aux: asset as u8,
        }
    }

    pub const fn action(title: &'static str, action: ActionId) -> Self {
        Self {
            title,
            field: SettingField::None,
            tag: TAG_ACTION,
            aux: action as u8,
        }
    }

    pub const fn battery_type(title: &'static str) -> Self {
        Self {
            title,
            field: SettingField::BATTERY_TYPE,
            tag: TAG_BATTERY_TYPE,
            aux: 0,
        }
    }

    pub const fn info(title: &'static str) -> Self {
        Self {
            title,
            field: SettingField::None,
            tag: TAG_INFO,
            aux: 0,
        }
    }
}
