//! System Domain Settings & Submenu Declarations

use super::options::{
    CHOICE_CPU_CLOCK, CHOICE_UART_BAUD, METRIC_STORAGE_DELETED, METRIC_STORAGE_FILES,
    METRIC_STORAGE_FREE, METRIC_STORAGE_USED, METRIC_STORAGE_WEAR, METRIC_SYS_BUILD,
    METRIC_SYS_MAC, METRIC_SYS_SERIAL, METRIC_UPTIME, PAGE_SYSTEM_STORAGE, SettingDef,
    SettingField, SettingKind,
};

pub static CPU_CLOCK_OPTIONS: [&'static str; 2] = ["48 MHz", "72 MHz"];
pub static UART_BAUD_OPTIONS: [&'static str; 2] = ["115200", "38400"];

#[cfg(feature = "settings_calib")]
use super::options::{
    METRIC_BATT_CALIB_REF, METRIC_CALIB_LNA, METRIC_CALIB_MIX, METRIC_CALIB_SIG, METRIC_CALIB_VOL,
    METRIC_CALIB_XTAL, PAGE_SYSTEM_CALIBRATION,
};

#[cfg(feature = "settings_debug")]
use super::options::{
    ACTION_CLEAR_LOGS, METRIC_BOOT_CRASHES, METRIC_DEBUG_DEVKEY, METRIC_DEBUG_MK, METRIC_DEBUG_RDP,
    METRIC_DEBUG_SWD, METRIC_STACK_HEADROOM, METRIC_STACK_PEAK, PAGE_SYSTEM_DEBUG,
};

pub static SYSTEM_ROOT: &[SettingDef] = &[
    SettingDef {
        title: "Firmware",
        kind: SettingKind::Live(METRIC_SYS_BUILD),
        field: SettingField::None,
    },
    SettingDef {
        title: "Serial",
        kind: SettingKind::Live(METRIC_SYS_SERIAL),
        field: SettingField::None,
    },
    SettingDef {
        title: "MAC",
        kind: SettingKind::Live(METRIC_SYS_MAC),
        field: SettingField::None,
    },
    SettingDef {
        title: "Clock",
        kind: SettingKind::Choice(CHOICE_CPU_CLOCK),
        field: SettingField::CPU_CLOCK,
    },
    SettingDef {
        title: "UART Baud",
        kind: SettingKind::Choice(CHOICE_UART_BAUD),
        field: SettingField::UART_BAUD,
    },
    SettingDef {
        title: "Uptime",
        kind: SettingKind::Live(METRIC_UPTIME),
        field: SettingField::None,
    },
    SettingDef {
        title: "Storage",
        kind: SettingKind::Submenu(PAGE_SYSTEM_STORAGE),
        field: SettingField::None,
    },
    #[cfg(feature = "settings_calib")]
    SettingDef {
        title: "Calibration",
        kind: SettingKind::Submenu(PAGE_SYSTEM_CALIBRATION),
        field: SettingField::None,
    },
    #[cfg(feature = "settings_debug")]
    SettingDef {
        title: "Debug",
        kind: SettingKind::Submenu(PAGE_SYSTEM_DEBUG),
        field: SettingField::None,
    },
];

pub static SYSTEM_STORAGE: [SettingDef; 5] = [
    SettingDef {
        title: "Used",
        kind: SettingKind::Live(METRIC_STORAGE_USED),
        field: SettingField::None,
    },
    SettingDef {
        title: "Free",
        kind: SettingKind::Live(METRIC_STORAGE_FREE),
        field: SettingField::None,
    },
    SettingDef {
        title: "Files",
        kind: SettingKind::Live(METRIC_STORAGE_FILES),
        field: SettingField::None,
    },
    SettingDef {
        title: "Wear Cycles",
        kind: SettingKind::Live(METRIC_STORAGE_WEAR),
        field: SettingField::None,
    },
    SettingDef {
        title: "Deleted",
        kind: SettingKind::Live(METRIC_STORAGE_DELETED),
        field: SettingField::None,
    },
];

#[cfg(feature = "settings_calib")]
pub static SYSTEM_CALIBRATION: [SettingDef; 6] = [
    SettingDef {
        title: "Calib Sig",
        kind: SettingKind::Live(METRIC_CALIB_SIG),
        field: SettingField::None,
    },
    SettingDef {
        title: "XTAL Trim",
        kind: SettingKind::Live(METRIC_CALIB_XTAL),
        field: SettingField::None,
    },
    SettingDef {
        title: "Batt Ref",
        kind: SettingKind::Live(METRIC_BATT_CALIB_REF),
        field: SettingField::None,
    },
    SettingDef {
        title: "LNA Gain",
        kind: SettingKind::Live(METRIC_CALIB_LNA),
        field: SettingField::None,
    },
    SettingDef {
        title: "Mixer Gain",
        kind: SettingKind::Live(METRIC_CALIB_MIX),
        field: SettingField::None,
    },
    SettingDef {
        title: "Volume Gain",
        kind: SettingKind::Live(METRIC_CALIB_VOL),
        field: SettingField::None,
    },
];

#[cfg(feature = "settings_debug")]
pub static SYSTEM_DEBUG: [SettingDef; 8] = [
    SettingDef {
        title: "Stack Margin",
        kind: SettingKind::Live(METRIC_STACK_HEADROOM),
        field: SettingField::None,
    },
    SettingDef {
        title: "Peak Stack",
        kind: SettingKind::Live(METRIC_STACK_PEAK),
        field: SettingField::None,
    },
    SettingDef {
        title: "Crashes",
        kind: SettingKind::Live(METRIC_BOOT_CRASHES),
        field: SettingField::None,
    },
    SettingDef {
        title: "SWD Port",
        kind: SettingKind::Live(METRIC_DEBUG_SWD),
        field: SettingField::None,
    },
    SettingDef {
        title: "MCU RDP",
        kind: SettingKind::Live(METRIC_DEBUG_RDP),
        field: SettingField::None,
    },
    SettingDef {
        title: "Dev Key",
        kind: SettingKind::Live(METRIC_DEBUG_DEVKEY),
        field: SettingField::None,
    },
    SettingDef {
        title: "MK Hash",
        kind: SettingKind::Live(METRIC_DEBUG_MK),
        field: SettingField::None,
    },
    SettingDef {
        title: "Clear Logs",
        kind: SettingKind::Action(ACTION_CLEAR_LOGS),
        field: SettingField::None,
    },
];
