//! System Domain Settings & Submenu Declarations

#[cfg(feature = "settings_debug")]
use super::page::ActionId;
use super::page::{MetricId, PageId, SettingDef, SettingField};

pub static CPU_CLOCK_OPTIONS: [&'static str; 2] = ["48 MHz", "72 MHz"];
pub static UART_BAUD_OPTIONS: [&'static str; 2] = ["115200", "38400"];

pub static SYSTEM_ROOT: &[SettingDef] = &[
    SettingDef::metric("Firmware", MetricId::SystemFirmware),
    SettingDef::metric("Serial", MetricId::SystemSerial),
    SettingDef::metric("MAC", MetricId::SystemMac),
    SettingDef::choice("Clock", &CPU_CLOCK_OPTIONS, SettingField::CPU_CLOCK),
    SettingDef::choice("UART Baud", &UART_BAUD_OPTIONS, SettingField::UART_BAUD),
    SettingDef::metric("Uptime", MetricId::SystemUptime),
    SettingDef::submenu("Storage", PageId::SystemStorage),
    #[cfg(feature = "settings_calib")]
    SettingDef::submenu("Calibration", PageId::SystemCalibration),
    #[cfg(feature = "settings_debug")]
    SettingDef::submenu("Debug", PageId::SystemDebug),
];

pub static SYSTEM_STORAGE: [SettingDef; 5] = [
    SettingDef::metric("Used", MetricId::StorageUsed),
    SettingDef::metric("Free", MetricId::StorageFree),
    SettingDef::metric("Files", MetricId::StorageFiles),
    SettingDef::metric("Wear Cycles", MetricId::StorageWear),
    SettingDef::metric("Deleted", MetricId::StorageDeleted),
];

#[cfg(feature = "settings_calib")]
pub static SYSTEM_CALIBRATION: [SettingDef; 6] = [
    SettingDef::metric("Calib Sig", MetricId::CalibSig),
    SettingDef::metric("XTAL Trim", MetricId::CalibXtal),
    SettingDef::metric("Batt Ref", MetricId::CalibBatt),
    SettingDef::metric("LNA Gain", MetricId::CalibLna),
    SettingDef::metric("Mixer Gain", MetricId::CalibMix),
    SettingDef::metric("Volume Gain", MetricId::CalibVol),
];

#[cfg(feature = "settings_debug")]
pub static SYSTEM_DEBUG: [SettingDef; 8] = [
    SettingDef::metric("Stack Margin", MetricId::DebugStackMargin),
    SettingDef::metric("Peak Stack", MetricId::DebugStackPeak),
    SettingDef::metric("Crashes", MetricId::DebugCrashes),
    SettingDef::metric("SWD Port", MetricId::DebugSwd),
    SettingDef::metric("MCU RDP", MetricId::DebugRdp),
    SettingDef::metric("Dev Key", MetricId::DebugDevKey),
    SettingDef::metric("MK Hash", MetricId::DebugMkHash),
    SettingDef::action("Clear Logs", ActionId::ClearLogs),
];
