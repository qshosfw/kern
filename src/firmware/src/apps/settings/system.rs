//! System Domain Settings & Submenu Declarations

use super::page::{ActionId, ChoiceId, MetricId, PageId, SettingDef, SettingField};

pub static CPU_CLOCK_OPTIONS: &str = "16 MHz\024 MHz\032 MHz\048 MHz\072 MHz\0";
pub static UART_BAUD_OPTIONS: &str = "115200\038400\0";

pub static SYSTEM_ROOT: &[SettingDef] = &[
    SettingDef::metric("Firmware", MetricId::SystemFirmware),
    SettingDef::metric("Serial", MetricId::SystemSerial),
    SettingDef::metric("MAC", MetricId::SystemMac),
    SettingDef::choice("Clock", ChoiceId::CpuClock, SettingField::CPU_CLOCK),
    SettingDef::choice("UART Baud", ChoiceId::UartBaud, SettingField::UART_BAUD),
    SettingDef::metric("Uptime", MetricId::SystemUptime),
    SettingDef::submenu("Storage", PageId::SystemStorage),
    #[cfg(feature = "settings_calib")]
    SettingDef::submenu("Calibration", PageId::SystemCalibration),
    #[cfg(feature = "settings_debug")]
    SettingDef::submenu("Debug", PageId::SystemDebug),
    SettingDef::action("Bootloader", ActionId::RebootBootloader),
];

pub static SYSTEM_STORAGE: &[SettingDef] = &[
    SettingDef::metric("Used", MetricId::StorageUsed),
    SettingDef::metric("Free", MetricId::StorageFree),
    SettingDef::metric("Files", MetricId::StorageFiles),
    SettingDef::metric("Wear Cycles", MetricId::StorageWear),
    SettingDef::metric("Deleted", MetricId::StorageDeleted),
    #[cfg(feature = "filesystem")]
    SettingDef::action("Format Storage", ActionId::FormatKeepCalib),
    #[cfg(feature = "filesystem")]
    SettingDef::action("Erase All Data", ActionId::FormatWipeAll),
    #[cfg(feature = "filesystem")]
    SettingDef::action("Blank Chip & DFU", ActionId::Wipe0xFFBootloader),
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
