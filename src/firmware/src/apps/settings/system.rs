//! System Domain Settings & Submenu Declarations

use super::page::{ActionId, ChoiceId, MetricId, PageId, SettingDef, SettingField};

#[cfg(feature = "overclock_72mhz")]
pub static CPU_CLOCK_OPTIONS: &str = "16 MHz\024 MHz\032 MHz\048 MHz\072 MHz\0";
#[cfg(not(feature = "overclock_72mhz"))]
pub static CPU_CLOCK_OPTIONS: &str = "16 MHz\024 MHz\032 MHz\048 MHz\0";

#[cfg(feature = "uart")]
pub static UART_BAUD_OPTIONS: &str = "115200\038400\0";

pub static SYSTEM_ROOT: &[SettingDef] = &[
    // Hardware & Bus Tuning
    SettingDef::choice("Clock", ChoiceId::CpuClock, SettingField::CPU_CLOCK),
    #[cfg(feature = "uart")]
    SettingDef::choice("UART Baud", ChoiceId::UartBaud, SettingField::UART_BAUD),
    // Identity, Storage & Diagnostics
    SettingDef::submenu("Info", PageId::SystemInfo),
    #[cfg(feature = "filesystem")]
    SettingDef::submenu("Storage", PageId::SystemStorage),
    #[cfg(feature = "settings_calib")]
    SettingDef::submenu("Calibration", PageId::SystemCalibration),
    #[cfg(feature = "settings_debug")]
    SettingDef::submenu("Debug", PageId::SystemDebug),
    // Maintenance & Power Control
    SettingDef::action("Reset Settings", ActionId::ResetSettings),
    #[cfg(feature = "dfu")]
    SettingDef::action("DFU Update", ActionId::RebootBootloader),
    SettingDef::action("Restart", ActionId::RebootFirmware),
];

pub static SYSTEM_INFO: &[SettingDef] = &[
    SettingDef::metric("Firmware", MetricId::SystemFirmware),
    SettingDef::metric("Bootloader", MetricId::SystemBootloader),
    SettingDef::metric("Serial", MetricId::SystemSerial),
    SettingDef::metric("QS-1 S/N", MetricId::SystemQs1Serial),
    SettingDef::metric("MAC", MetricId::SystemMac),
    SettingDef::metric("MCU Flash", MetricId::SystemFlashSize),
    SettingDef::metric("Uptime", MetricId::SystemUptime),
];

#[cfg(feature = "filesystem")]
pub static SYSTEM_STORAGE: &[SettingDef] = &[
    SettingDef::metric("Used", MetricId::StorageUsed),
    SettingDef::metric("Free", MetricId::StorageFree),
    SettingDef::metric("Files", MetricId::StorageFiles),
    SettingDef::metric("Wear Cycles", MetricId::StorageWear),
    SettingDef::metric("Deleted", MetricId::StorageDeleted),
    SettingDef::action("Format Storage", ActionId::FormatKeepCalib),
    SettingDef::action("Erase All Data", ActionId::FormatWipeAll),
    #[cfg(feature = "dfu")]
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
