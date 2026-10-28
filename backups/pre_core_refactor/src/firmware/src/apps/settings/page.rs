//! Declarative Settings Abstractions: Page IDs, Metrics, Choices, and Field Enums.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PageId {
    Root = 0,
    Display = 1,
    Audio = 2,
    Buttons = 3,
    Battery = 4,
    Device = 5,
    Security = 6,
    System = 7,
    ButtonsTweaks = 8,
    ButtonsShortcuts = 9,
    BatteryInfo = 10,
    BatteryTweaks = 11,
    SystemStorage = 12,
    Apps = 13,
    BootSplash = 14,
    #[cfg(feature = "settings_calib")]
    SystemCalibration = 15,
    #[cfg(feature = "settings_debug")]
    SystemDebug = 16,
    ManageApps = 17,
    AppDetail = 18,
    ManageServices = 19,
    ServiceDetail = 20,
}

// Backward compatibility constants for UI page numbers
pub const PAGE_ROOT: u8 = PageId::Root as u8;
pub const PAGE_DISPLAY: u8 = PageId::Display as u8;
pub const PAGE_AUDIO: u8 = PageId::Audio as u8;
pub const PAGE_BUTTONS: u8 = PageId::Buttons as u8;
pub const PAGE_BATTERY: u8 = PageId::Battery as u8;
pub const PAGE_DEVICE: u8 = PageId::Device as u8;
pub const PAGE_SECURITY: u8 = PageId::Security as u8;
pub const PAGE_SYSTEM: u8 = PageId::System as u8;
pub const PAGE_BUTTONS_TWEAKS: u8 = PageId::ButtonsTweaks as u8;
pub const PAGE_BUTTONS_SHORTCUTS: u8 = PageId::ButtonsShortcuts as u8;
pub const PAGE_BATTERY_INFO: u8 = PageId::BatteryInfo as u8;
pub const PAGE_BATTERY_TWEAKS: u8 = PageId::BatteryTweaks as u8;
pub const PAGE_SYSTEM_STORAGE: u8 = PageId::SystemStorage as u8;
pub const PAGE_APPS: u8 = PageId::Apps as u8;
pub const PAGE_BOOT_SPLASH: u8 = PageId::BootSplash as u8;
#[cfg(feature = "settings_calib")]
pub const PAGE_SYSTEM_CALIBRATION: u8 = PageId::SystemCalibration as u8;
#[cfg(feature = "settings_debug")]
pub const PAGE_SYSTEM_DEBUG: u8 = PageId::SystemDebug as u8;
pub const PAGE_MANAGE_APPS: u8 = PageId::ManageApps as u8;
pub const PAGE_APP_DETAIL: u8 = PageId::AppDetail as u8;
pub const PAGE_MANAGE_SERVICES: u8 = PageId::ManageServices as u8;
pub const PAGE_SERVICE_DETAIL: u8 = PageId::ServiceDetail as u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MetricId {
    BatteryVolts = 1,
    BatteryRawAdc = 2,
    BatteryPct = 3,
    VddaRef = 4,
    BattCalibRef = 5,
    McuTemp = 6,
    Uptime = 7,
    SysSerial = 8,
    SysMac = 9,
    SysClock = 10,
    StorageUsed = 11,
    StorageFree = 12,
    StorageFiles = 13,
    StorageWear = 14,
    StorageDeleted = 15,
    PasscodeStatus = 16,
    DeviceOwner = 17,
    DeviceAsset = 18,
    DeviceRadioId = 19,
    CalibSig = 20,
    CalibXtal = 21,
    CalibLna = 22,
    CalibMix = 23,
    CalibVol = 24,
    DebugSwd = 25,
    DebugDevKey = 26,
    DebugMk = 27,
    BatteryCurrent = 28,
    BatteryStatus = 29,
    SysBuild = 30,
    DebugRdp = 31,
    StackHeadroom = 32,
    StackPeak = 33,
    BootCrashes = 34,
    AppsInstalled = 35,
}

pub const METRIC_APPS_INSTALLED: u8 = MetricId::AppsInstalled as u8;

pub const METRIC_BATTERY_VOLTS: u8 = MetricId::BatteryVolts as u8;
pub const METRIC_BATTERY_RAW_ADC: u8 = MetricId::BatteryRawAdc as u8;
pub const METRIC_BATTERY_PCT: u8 = MetricId::BatteryPct as u8;
pub const METRIC_VDDA_REF: u8 = MetricId::VddaRef as u8;
pub const METRIC_BATT_CALIB_REF: u8 = MetricId::BattCalibRef as u8;
pub const METRIC_MCU_TEMP: u8 = MetricId::McuTemp as u8;
pub const METRIC_UPTIME: u8 = MetricId::Uptime as u8;
pub const METRIC_SYS_SERIAL: u8 = MetricId::SysSerial as u8;
pub const METRIC_SYS_MAC: u8 = MetricId::SysMac as u8;
pub const METRIC_SYS_CLOCK: u8 = MetricId::SysClock as u8;
pub const METRIC_SYS_BUILD: u8 = MetricId::SysBuild as u8;
pub const METRIC_STORAGE_USED: u8 = MetricId::StorageUsed as u8;
pub const METRIC_STORAGE_FREE: u8 = MetricId::StorageFree as u8;
pub const METRIC_STORAGE_FILES: u8 = MetricId::StorageFiles as u8;
pub const METRIC_STORAGE_WEAR: u8 = MetricId::StorageWear as u8;
pub const METRIC_STORAGE_DELETED: u8 = MetricId::StorageDeleted as u8;
pub const METRIC_PASSCODE_STATUS: u8 = MetricId::PasscodeStatus as u8;
pub const METRIC_DEVICE_OWNER: u8 = MetricId::DeviceOwner as u8;
pub const METRIC_DEVICE_ASSET: u8 = MetricId::DeviceAsset as u8;
pub const METRIC_DEVICE_RADIO_ID: u8 = MetricId::DeviceRadioId as u8;
pub const METRIC_CALIB_SIG: u8 = MetricId::CalibSig as u8;
pub const METRIC_CALIB_XTAL: u8 = MetricId::CalibXtal as u8;
pub const METRIC_CALIB_LNA: u8 = MetricId::CalibLna as u8;
pub const METRIC_CALIB_MIX: u8 = MetricId::CalibMix as u8;
pub const METRIC_CALIB_VOL: u8 = MetricId::CalibVol as u8;
pub const METRIC_DEBUG_SWD: u8 = MetricId::DebugSwd as u8;
pub const METRIC_DEBUG_RDP: u8 = MetricId::DebugRdp as u8;
pub const METRIC_DEBUG_DEVKEY: u8 = MetricId::DebugDevKey as u8;
pub const METRIC_DEBUG_MK: u8 = MetricId::DebugMk as u8;
pub const METRIC_BATTERY_CURRENT: u8 = MetricId::BatteryCurrent as u8;
pub const METRIC_BATTERY_STATUS: u8 = MetricId::BatteryStatus as u8;
pub const METRIC_STACK_HEADROOM: u8 = MetricId::StackHeadroom as u8;
pub const METRIC_STACK_PEAK: u8 = MetricId::StackPeak as u8;
pub const METRIC_BOOT_CRASHES: u8 = MetricId::BootCrashes as u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionId {
    Passcode = 1,
    EditOwner = 2,
    EditTag = 3,
    EditRadioId = 4,
    RogerChooser = 5,
    SplashChooser = 6,
    ChooseModel = 7,
    ClearLogs = 8,
}

pub const ACTION_PASSCODE: u8 = ActionId::Passcode as u8;
pub const ACTION_EDIT_OWNER: u8 = ActionId::EditOwner as u8;
pub const ACTION_EDIT_TAG: u8 = ActionId::EditTag as u8;
pub const ACTION_EDIT_RADIO_ID: u8 = ActionId::EditRadioId as u8;
pub const ACTION_ROGER_CHOOSER: u8 = ActionId::RogerChooser as u8;
pub const ACTION_SPLASH_CHOOSER: u8 = ActionId::SplashChooser as u8;
pub const ACTION_CHOOSE_MODEL: u8 = ActionId::ChooseModel as u8;
pub const ACTION_CLEAR_LOGS: u8 = ActionId::ClearLogs as u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChoiceGroup {
    BacklightTime = 0,
    BacklightTr = 1,
    SplashTime = 2,
    KeyBeep = 3,
    BootJingle = 4,
    AudioProfile = 5,
    AutoLock = 6,
    FKeyTimeout = 7,
    FastScroll = 8,
    HoldDelay = 9,
    RepeatRate = 10,
    Shortcut = 11,
    MLong = 12,
    BatteryFormat = 13,
    DeepSleep = 14,
    PowerProfile = 15,
    OffOn = 16,
    CpuClock = 17,
    LauncherStyle = 18,
    StartupApp = 19,
    VfoDualWatch = 20,
    BootAnim = 21,
    PttMode = 22,
    UartBaud = 23,
    ShadingMode = 24,
    UiAnimations = 25,
    RogerBeep = 26,
}

pub const CHOICE_BACKLIGHT_TIME: u8 = ChoiceGroup::BacklightTime as u8;
pub const CHOICE_BACKLIGHT_TR: u8 = ChoiceGroup::BacklightTr as u8;
pub const CHOICE_SPLASH_TIME: u8 = ChoiceGroup::SplashTime as u8;
pub const CHOICE_KEY_BEEP: u8 = ChoiceGroup::KeyBeep as u8;
pub const CHOICE_BOOT_JINGLE: u8 = ChoiceGroup::BootJingle as u8;
pub const CHOICE_AUDIO_PROFILE: u8 = ChoiceGroup::AudioProfile as u8;
pub const CHOICE_AUTO_LOCK: u8 = ChoiceGroup::AutoLock as u8;
pub const CHOICE_F_KEY_TIMEOUT: u8 = ChoiceGroup::FKeyTimeout as u8;
pub const CHOICE_FAST_SCROLL: u8 = ChoiceGroup::FastScroll as u8;
pub const CHOICE_HOLD_DELAY: u8 = ChoiceGroup::HoldDelay as u8;
pub const CHOICE_REPEAT_RATE: u8 = ChoiceGroup::RepeatRate as u8;
pub const CHOICE_SHORTCUT: u8 = ChoiceGroup::Shortcut as u8;
pub const CHOICE_M_LONG: u8 = ChoiceGroup::MLong as u8;
pub const CHOICE_BATTERY_FORMAT: u8 = ChoiceGroup::BatteryFormat as u8;
pub const CHOICE_DEEP_SLEEP: u8 = ChoiceGroup::DeepSleep as u8;
pub const CHOICE_PTT_MODE: u8 = ChoiceGroup::PttMode as u8;
pub const CHOICE_POWER_PROFILE: u8 = ChoiceGroup::PowerProfile as u8;
pub const CHOICE_OFF_ON: u8 = ChoiceGroup::OffOn as u8;
pub const CHOICE_CPU_CLOCK: u8 = ChoiceGroup::CpuClock as u8;
pub const CHOICE_LAUNCHER_STYLE: u8 = ChoiceGroup::LauncherStyle as u8;
pub const CHOICE_STARTUP_APP: u8 = ChoiceGroup::StartupApp as u8;
pub const CHOICE_VFO_DUAL_WATCH: u8 = ChoiceGroup::VfoDualWatch as u8;
pub const CHOICE_BOOT_ANIM: u8 = ChoiceGroup::BootAnim as u8;
pub const CHOICE_UART_BAUD: u8 = ChoiceGroup::UartBaud as u8;
pub const CHOICE_SHADING_MODE: u8 = ChoiceGroup::ShadingMode as u8;
pub const CHOICE_UI_ANIMATIONS: u8 = ChoiceGroup::UiAnimations as u8;
pub const CHOICE_ROGER_BEEP: u8 = ChoiceGroup::RogerBeep as u8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    None,
    Db,
    Percent,
}

impl Unit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Unit::None => "",
            Unit::Db => "dB",
            Unit::Percent => "%",
        }
    }
}

pub const RANGE_0_10: u8 = 0;
pub const RANGE_MIC_GAIN: u8 = 1;
pub const RANGE_BACKLIGHT_MAX: u8 = 2;
pub const RANGE_CONTRAST: u8 = 3;

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum SettingKind {
    Submenu(u8),
    Choice(u8),
    BatteryType,
    Range(u8),
    CalibTweak,
    Toggle,
    Live(u8),
    Action(u8),
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

    // Battery (domain 4)
    pub const BATTERY_TYPE: Self = Self::byte(4, 1);
    pub const BATTERY_FORMAT: Self = Self::byte(4, 2);
    pub const POWER_PROFILE: Self = Self::byte(4, 3);
    pub const AUTO_POWER_OFF: Self = Self::byte(4, 4);
    pub const CHARGE_DETECT: Self = Self::byte(4, 5);
    pub const CALIB_TWEAK_MV: Self = Self::CalibTweak;
    pub const CPU_CLOCK: Self = Self::byte(4, 12);

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
