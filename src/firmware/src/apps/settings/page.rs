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
    Security,
    System,
    ButtonsTweaks,
    ButtonsShortcuts,
    BatteryInfo,
    BatteryTweaks,
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
}

impl PageId {
    pub const COUNT: usize = PageId::ServiceDetail as usize + 1;
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
    ClearLogs = 5,
}

#[derive(Clone, Copy)]
pub struct RangeDef {
    pub min: i16,
    pub max: i16,
    pub step: i16,
    pub unit: &'static str,
}

pub static RANGE_0_10: RangeDef = RangeDef {
    min: 0,
    max: 10,
    step: 1,
    unit: "",
};
pub static RANGE_1_10: RangeDef = RangeDef {
    min: 1,
    max: 10,
    step: 1,
    unit: "",
};
pub static RANGE_MIC_GAIN: RangeDef = RangeDef {
    min: 0,
    max: 31,
    step: 1,
    unit: "dB",
};
pub static RANGE_CONTRAST: RangeDef = RangeDef {
    min: 0,
    max: 100,
    step: 5,
    unit: "%",
};
pub static RANGE_CALIB_TWEAK: RangeDef = RangeDef {
    min: -500,
    max: 500,
    step: 10,
    unit: "mV",
};

#[derive(Clone, Copy)]
pub struct MenuFrame {
    pub page: PageId,
    pub selected_idx: usize,
    pub title: &'static str,
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
    StorageUsed,
    StorageFree,
    StorageFiles,
    StorageWear,
    StorageDeleted,

    // System Identity & Health
    SystemFirmware,
    SystemSerial,
    SystemMac,
    SystemUptime,

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

#[derive(Clone, Copy)]
pub enum SettingKind {
    Submenu(PageId),
    Choice(&'static [&'static str]),
    Shortcut,
    BatteryType,
    Range(&'static RangeDef),
    Toggle,
    Metric(MetricId),
    #[cfg(feature = "boot_assets")]
    Asset(&'static super::asset_chooser::AssetDescriptor),
    Action(ActionId),
    Info,
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

    // Keys (domain 2)
    pub const F1_SHORT: Self = Self::byte(2, 2);
    pub const F1_LONG: Self = Self::byte(2, 3);
    pub const F2_SHORT: Self = Self::byte(2, 4);
    pub const F2_LONG: Self = Self::byte(2, 5);
    pub const M_LONG: Self = Self::byte(2, 6);
    pub const AUTO_LOCK: Self = Self::byte(2, 7);
    pub const F_KEY_TIMEOUT: Self = Self::byte(2, 8);
    pub const FAST_SCROLL: Self = Self::byte(2, 9);
    pub const HOLD_DELAY: Self = Self::byte(2, 10);
    pub const REPEAT_RATE: Self = Self::byte(2, 11);
    #[inline(always)]
    pub const fn f_digit(idx: u8) -> Self {
        Self::byte(2, 12 + idx)
    }
    pub const PTT_MODE: Self = Self::byte(2, 21);
    pub const F_0: Self = Self::byte(2, 22);
    pub const F_STAR: Self = Self::byte(2, 23);
    pub const F_MENU: Self = Self::byte(2, 24);
    pub const F_SIDE1: Self = Self::byte(2, 25);
    pub const F_SIDE2: Self = Self::byte(2, 26);

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

    #[inline(always)]
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

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SettingDef {
    pub title: &'static str,
    pub kind: SettingKind,
    pub field: SettingField,
}

impl SettingDef {
    pub const fn submenu(title: &'static str, target: PageId) -> Self {
        Self {
            title,
            kind: SettingKind::Submenu(target),
            field: SettingField::None,
        }
    }

    pub const fn choice(
        title: &'static str,
        options: &'static [&'static str],
        field: SettingField,
    ) -> Self {
        Self {
            title,
            kind: SettingKind::Choice(options),
            field,
        }
    }

    pub const fn shortcut(title: &'static str, field: SettingField) -> Self {
        Self {
            title,
            kind: SettingKind::Shortcut,
            field,
        }
    }

    pub const fn range(title: &'static str, range: &'static RangeDef, field: SettingField) -> Self {
        Self {
            title,
            kind: SettingKind::Range(range),
            field,
        }
    }

    pub const fn toggle(title: &'static str, field: SettingField) -> Self {
        Self {
            title,
            kind: SettingKind::Toggle,
            field,
        }
    }

    pub const fn metric(title: &'static str, metric: MetricId) -> Self {
        Self {
            title,
            kind: SettingKind::Metric(metric),
            field: SettingField::None,
        }
    }

    #[cfg(feature = "boot_assets")]
    pub const fn asset(
        title: &'static str,
        desc: &'static super::asset_chooser::AssetDescriptor,
    ) -> Self {
        Self {
            title,
            kind: SettingKind::Asset(desc),
            field: SettingField::None,
        }
    }

    pub const fn action(title: &'static str, action: ActionId) -> Self {
        Self {
            title,
            kind: SettingKind::Action(action),
            field: SettingField::None,
        }
    }

    pub const fn battery_type(title: &'static str) -> Self {
        Self {
            title,
            kind: SettingKind::BatteryType,
            field: SettingField::BATTERY_TYPE,
        }
    }

    pub const fn info(title: &'static str) -> Self {
        Self {
            title,
            kind: SettingKind::Info,
            field: SettingField::None,
        }
    }
}
