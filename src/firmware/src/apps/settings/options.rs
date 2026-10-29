#[cfg(feature = "settings_calib")]
use crate::services::calibration::CalibrationService;
use crate::services::display::DisplayService;
use crate::services::power::{PowerService, PowerServiceConfigExt};
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub use super::apps::{LAUNCHER_STYLE_OPTIONS, STARTUP_APP_OPTIONS};
pub use super::audio::{AUDIO_PROFILE_OPTIONS, BOOT_JINGLE_OPTIONS, KEY_BEEP_OPTIONS};
pub use super::battery::{
    AUTO_POWER_OFF_OPTIONS, BATTERY_FORMAT_OPTIONS, BATTERY_K1_OPTIONS, BATTERY_K1_TYPES,
    BATTERY_K5_OPTIONS, BATTERY_K5_TYPES, POWER_PROFILE_OPTIONS,
};
pub use super::buttons::{
    AUTO_LOCK_OPTIONS, F_KEY_TIMEOUT_OPTIONS, FAST_SCROLL_OPTIONS, HOLD_DELAY_OPTIONS,
    M_LONG_OPTIONS, PTT_MODE_OPTIONS, REPEAT_RATE_OPTIONS, SHORTCUT_OPTIONS,
};
pub use super::device::{BOOT_ANIM_OPTIONS, MODEL_OPTIONS, SPLASH_TIME_OPTIONS};
pub use super::display::{
    ANIMATION_OPTIONS, BACKLIGHT_TIME_OPTIONS, BACKLIGHT_TR_OPTIONS, SHADING_OPTIONS,
};
pub use super::system::{CPU_CLOCK_OPTIONS, UART_BAUD_OPTIONS};

pub static OFF_ON: [&'static str; 2] = ["OFF", "ON"];

pub use super::page::*;

static ALL_CHOICE_OPTIONS: [&[&str]; 27] = [
    &BACKLIGHT_TIME_OPTIONS,
    &BACKLIGHT_TR_OPTIONS,
    &SPLASH_TIME_OPTIONS,
    &KEY_BEEP_OPTIONS,
    &BOOT_JINGLE_OPTIONS,
    &AUDIO_PROFILE_OPTIONS,
    &AUTO_LOCK_OPTIONS,
    &F_KEY_TIMEOUT_OPTIONS,
    &FAST_SCROLL_OPTIONS,
    &HOLD_DELAY_OPTIONS,
    &REPEAT_RATE_OPTIONS,
    &SHORTCUT_OPTIONS,
    &M_LONG_OPTIONS,
    &BATTERY_FORMAT_OPTIONS,
    &AUTO_POWER_OFF_OPTIONS,
    &POWER_PROFILE_OPTIONS,
    &OFF_ON,
    &CPU_CLOCK_OPTIONS,
    &LAUNCHER_STYLE_OPTIONS,
    &STARTUP_APP_OPTIONS,
    &[],
    &BOOT_ANIM_OPTIONS,
    &PTT_MODE_OPTIONS,
    &UART_BAUD_OPTIONS,
    &SHADING_OPTIONS,
    &ANIMATION_OPTIONS,
    &[],
];

#[inline(always)]
pub fn get_choice_options(id: u8) -> &'static [&'static str] {
    ALL_CHOICE_OPTIONS.get(id as usize).copied().unwrap_or(&[])
}

pub static ROOT_DEFS: &[SettingDef] = &[
    SettingDef {
        title: "Display",
        kind: SettingKind::Submenu(PAGE_DISPLAY),
        field: SettingField::None,
    },
    SettingDef {
        title: "Audio",
        kind: SettingKind::Submenu(PAGE_AUDIO),
        field: SettingField::None,
    },
    SettingDef {
        title: "Buttons",
        kind: SettingKind::Submenu(PAGE_BUTTONS),
        field: SettingField::None,
    },
    SettingDef {
        title: "Battery",
        kind: SettingKind::Submenu(PAGE_BATTERY),
        field: SettingField::None,
    },
    SettingDef {
        title: "Device",
        kind: SettingKind::Submenu(PAGE_DEVICE),
        field: SettingField::None,
    },
    SettingDef {
        title: "Apps",
        kind: SettingKind::Submenu(PAGE_APPS),
        field: SettingField::None,
    },
    #[cfg(feature = "passcode")]
    SettingDef {
        title: "Security",
        kind: SettingKind::Submenu(PAGE_SECURITY),
        field: SettingField::None,
    },
    SettingDef {
        title: "System",
        kind: SettingKind::Submenu(PAGE_SYSTEM),
        field: SettingField::None,
    },
];

#[cfg(all(feature = "settings_debug", feature = "passcode"))]
#[inline(never)]
fn format_debug_metric(metric: u8, ctx: &mut crate::SysContext, out: &mut ShortString) {
    let sec = crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode);
    match metric {
        METRIC_DEBUG_SWD => out.set_str(if sec.swd_open { "Open" } else { "Locked" }),
        METRIC_DEBUG_RDP => out.set_str(sec.rdp.short_str()),
        _ => {
            let hash = if metric == METRIC_DEBUG_DEVKEY {
                sec.dev_key_hash
            } else {
                sec.master_key_hash
            };
            match hash {
                Some(h) => *out = ShortString::from_hex32(h),
                None => out.set_str("None"),
            }
        }
    }
}

#[cfg(all(feature = "settings_debug", not(feature = "passcode")))]
#[inline(never)]
fn format_debug_metric(metric: u8, _ctx: &mut crate::SysContext, out: &mut ShortString) {
    match metric {
        METRIC_DEBUG_SWD => out.set_str(if crate::security::port_lock::is_swd_enabled() {
            "Open"
        } else {
            "Locked"
        }),
        METRIC_DEBUG_RDP => out.set_str(py32_hal::flash::InternalFlash::rdp_status().short_str()),
        _ => out.set_str("None"),
    }
}

#[inline(never)]
pub fn format_live_metric(metric: u8, ctx: &mut crate::SysContext, out: &mut ShortString) {
    match metric {
        METRIC_BATTERY_STATUS => {
            out.set_str(if ctx.power.is_charging() {
                "Charging"
            } else {
                "Discharging"
            });
        }
        #[cfg(feature = "settings_calib")]
        METRIC_CALIB_SIG => {
            out.set_str(if ctx.calibration.is_valid() {
                "Valid"
            } else {
                "None"
            });
        }
        METRIC_PASSCODE_STATUS => {
            out.set_str(if crate::security::passcode::is_passcode_set(ctx.storage) {
                "Set"
            } else {
                "None"
            });
        }
        METRIC_SYS_BUILD => out.set_str(crate::build_info::BUILD_STAMP),
        METRIC_DEVICE_OWNER => out.set_str(ctx.config.system.owner_str()),
        METRIC_DEVICE_ASSET => out.set_str(ctx.config.system.tag_str()),
        METRIC_UPTIME => {
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 8]) };
            let _ = utils::format::format_uptime_hms(ctx.power.uptime_secs(), arr);
            out.len = 8;
        }
        METRIC_SYS_SERIAL => {
            let serial = bsp::uid::get_crockford_serial();
            out.set_str(unsafe { core::str::from_utf8_unchecked(&serial[..14]) });
        }
        METRIC_SYS_MAC => {
            let mac = bsp::uid::read_mac_address();
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 17]) };
            let _ = utils::format::format_mac_address(&mac, arr);
            out.len = 17;
        }
        METRIC_BATTERY_VOLTS => {
            let mv = ctx.power.battery_voltage_mv() as u32;
            let v_deci2 = ((mv * 52429) >> 19) as i32;
            out.push_int_fixed(v_deci2, 2, "", "V");
        }
        METRIC_MCU_TEMP => {
            out.push_int_fixed(ctx.power.mcu_temperature_deci_c() as i32, 1, "", " C");
        }
        #[cfg(feature = "settings_debug")]
        METRIC_DEBUG_SWD | METRIC_DEBUG_RDP | METRIC_DEBUG_DEVKEY | METRIC_DEBUG_MK => {
            format_debug_metric(metric, ctx, out);
        }
        _ => {
            const UNIT_NONE: u8 = 0;
            const UNIT_KB: u8 = 1;
            const UNIT_PCT: u8 = 2;
            const UNIT_MA: u8 = 3;
            const UNIT_MV: u8 = 4;
            const UNIT_MHZ: u8 = 5;
            const UNIT_B: u8 = 6;
            const UNIT_SEC: u8 = 7;
            #[cfg(feature = "settings_calib")]
            const UNIT_HZ: u8 = 8;

            let (val, unit_code): (i32, u8) = if metric >= METRIC_STORAGE_USED && metric <= METRIC_STORAGE_DELETED {
                let st = ctx.storage.stats();
                match metric {
                    METRIC_STORAGE_USED => (st.used_sectors as i32 * 4, UNIT_KB),
                    METRIC_STORAGE_FREE => (st.free_sectors as i32 * 4, UNIT_KB),
                    METRIC_STORAGE_FILES => (st.active_files as i32, UNIT_NONE),
                    METRIC_STORAGE_WEAR => (st.wear_cycles as i32, UNIT_NONE),
                    _ => (st.deleted_sectors as i32, UNIT_SEC),
                }
            } else {
                match metric {
                    METRIC_BATTERY_RAW_ADC => (ctx.power.raw_battery_adc() as i32, UNIT_NONE),
                    METRIC_BATTERY_PCT => (ctx.power.battery_percentage() as i32, UNIT_PCT),
                    METRIC_BATTERY_CURRENT => (ctx.power.battery_current_ma() as i32, UNIT_MA),
                    METRIC_VDDA_REF => (ctx.power.vdda_voltage_mv() as i32, UNIT_MV),
                    METRIC_SYS_CLOCK => (
                        if ctx.config.battery.cpu_clock == 1 {
                            72
                        } else {
                            48
                        },
                        UNIT_MHZ,
                    ),
                    METRIC_DEVICE_RADIO_ID => (ctx.config.system.radio_id as i32, UNIT_NONE),
                    METRIC_STACK_HEADROOM => (
                        kernel::sys::SystemDiagnostics::stack_margin_bytes() as i32,
                        UNIT_B,
                    ),
                    METRIC_STACK_PEAK => (
                        kernel::sys::SystemDiagnostics::stack_peak_used_bytes() as i32,
                        UNIT_B,
                    ),
                    #[cfg(feature = "flight_recorder")]
                    METRIC_BOOT_CRASHES => (
                        crate::panic::BOOT_RECORD
                            .get()
                            .latest_valid_slot()
                            .map_or(0, |s| s.crash_count as i32),
                        UNIT_NONE,
                    ),
                    #[cfg(not(feature = "flight_recorder"))]
                    METRIC_BOOT_CRASHES => (0, UNIT_NONE),
                    METRIC_APPS_INSTALLED => (
                        crate::apps::launcher::catalog::get_catalog().entries.len() as i32,
                        UNIT_NONE,
                    ),
                    #[cfg(feature = "settings_calib")]
                    METRIC_BATT_CALIB_REF => (ctx.calibration.battery_calib_const() as i32, UNIT_NONE),
                    #[cfg(feature = "settings_calib")]
                    METRIC_CALIB_XTAL => (ctx.calibration.xtal_freq_low() as i32, UNIT_HZ),
                    #[cfg(feature = "settings_calib")]
                    METRIC_CALIB_LNA => (ctx.calibration.lna_calib() as i32, UNIT_NONE),
                    #[cfg(feature = "settings_calib")]
                    METRIC_CALIB_MIX => (ctx.calibration.mix_calib() as i32, UNIT_NONE),
                    #[cfg(feature = "settings_calib")]
                    METRIC_CALIB_VOL => (ctx.calibration.volume_gain() as i32, UNIT_NONE),
                    _ => (-1, UNIT_NONE),
                }
            };
            if val >= 0 {
                let unit = match unit_code {
                    UNIT_KB => " KB",
                    UNIT_PCT => "%",
                    UNIT_MA => " mA",
                    UNIT_MV => " mV",
                    UNIT_MHZ => " MHz",
                    UNIT_B => " B",
                    UNIT_SEC => " sec",
                    #[cfg(feature = "settings_calib")]
                    UNIT_HZ => " Hz",
                    _ => "",
                };
                out.push_int_fixed(val, 0, "", unit);
            }
        }
    }
}

#[inline(never)]
fn get_action_value(action: u8, ctx: &mut crate::SysContext, out: &mut ShortString) {
    match action {
        ACTION_CHOOSE_MODEL => out.set_str(ctx.config.system.model_str()),
        ACTION_EDIT_OWNER => out.set_str(ctx.config.system.owner_str()),
        ACTION_EDIT_TAG => out.set_str(ctx.config.system.tag_str()),
        ACTION_EDIT_RADIO_ID => {
            out.push_int_fixed(ctx.config.system.radio_id as i32, 0, "", "");
        }
        #[cfg(feature = "boot_assets")]
        ACTION_SPLASH_CHOOSER => {
            if ctx.config.display.boot_splash_mode == 0 {
                out.set_str("None");
            } else {
                *out =
                    super::asset_chooser::format_asset_name(ctx.config.display.splash_path_str());
            }
        }
        #[cfg(feature = "boot_assets")]
        ACTION_ROGER_CHOOSER => {
            if ctx.config.audio.roger_beep == 0 {
                out.set_str("Off");
            } else {
                *out = super::asset_chooser::format_asset_name(ctx.config.audio.roger_path_str());
            }
        }
        #[cfg(feature = "boot_assets")]
        ACTION_JINGLE_CHOOSER => {
            if ctx.config.audio.boot_jingle == 0 {
                out.set_str("Off");
            } else {
                *out = super::asset_chooser::format_asset_name(ctx.config.audio.jingle_path_str());
            }
        }
        _ => {}
    }
}

#[inline(always)]
pub fn battery_options_and_types(nav_layout: u8) -> (&'static [&'static str], &'static [u8]) {
    if nav_layout == 0 {
        (&BATTERY_K1_OPTIONS, &BATTERY_K1_TYPES)
    } else {
        (&BATTERY_K5_OPTIONS, &BATTERY_K5_TYPES)
    }
}

static RANGES: [(u8, u8, u8, &'static str); 4] = [
    (0, 10, 1, ""),
    (0, 31, 1, "dB"),
    (1, 10, 1, ""),
    (0, 100, 5, "%"),
];

#[inline(never)]
fn build_item_type(def: &SettingDef, ctx: &mut crate::SysContext, nav_layout: u8) -> MenuItemType {
    match def.kind {
        SettingKind::Choice(choice_id) => {
            let opts = get_choice_options(choice_id);
            let raw_byte = def.field.get_u8(&ctx.config);
            let sel = (raw_byte as usize).min(opts.len().saturating_sub(1)) as u16;
            MenuItemType::Choice {
                selected: sel,
                options: opts,
            }
        }
        SettingKind::BatteryType => {
            let raw_byte = def.field.get_u8(&ctx.config);
            let (opts, types) = battery_options_and_types(nav_layout);
            let sel = types.iter().position(|&t| t == raw_byte).unwrap_or(0) as u16;
            MenuItemType::Choice {
                selected: sel,
                options: opts,
            }
        }
        SettingKind::Range(range_id) => {
            let &(min, max, step, unit) = RANGES.get(range_id as usize).unwrap_or(&RANGES[0]);
            let raw_byte = def.field.get_u8(&ctx.config);
            MenuItemType::Range {
                val: raw_byte.clamp(min, max) as i16,
                min: min as i16,
                max: max as i16,
                step: step as i16,
                unit,
            }
        }
        SettingKind::CalibTweak => MenuItemType::Range {
            val: def.field.get_i16(&ctx.config),
            min: -500,
            max: 500,
            step: 10,
            unit: "mV",
        },
        SettingKind::Toggle => MenuItemType::Toggle(def.field.get_u8(&ctx.config) != 0),
        SettingKind::Submenu(_) => MenuItemType::Submenu,
        SettingKind::Live(metric) => {
            let mut s = ShortString::empty();
            format_live_metric(metric, ctx, &mut s);
            MenuItemType::Value(s)
        }
        SettingKind::Action(action) => {
            let mut s = ShortString::empty();
            get_action_value(action, ctx, &mut s);
            MenuItemType::Value(s)
        }
        SettingKind::Info => MenuItemType::Info("Ascon-128"),
    }
}

#[inline(never)]
pub fn populate_defs(items: &mut [MenuItem], defs: &[SettingDef], ctx: &mut crate::SysContext) {
    let nav_layout = ctx.config.keys.nav_layout;
    for (i, def) in defs.iter().enumerate() {
        if i >= items.len() {
            break;
        }
        items[i].title = def.title;
        items[i].item_type = build_item_type(def, ctx, nav_layout);
    }
}

pub fn apply_page_changes(page_id: u8, ctx: &mut crate::SysContext) {
    match page_id {
        PAGE_DISPLAY => {
            ctx.config.mark_display_dirty();
            let cfg = ctx.config.display;
            if let Some(ref mut display) = ctx.display {
                display.set_contrast(cfg.contrast);
                display.set_inverted(cfg.inverted != 0);
                display.set_shading_mode(cfg.shading_mode);
                display.set_backlight_mode(cfg.backlight_mode_service());
                display.set_backlight_brightness(
                    cfg.backlight_max.clamp(1, 10),
                    cfg.backlight_min.min(10),
                );
            }
        }
        PAGE_AUDIO => {
            ctx.config.mark_audio_dirty();
            let cfg = ctx.config.audio;
            ctx.set_volume(cfg.volume);
            ctx.radio
                .set_tone_gain((cfg.beep_gain as u16 * 12).min(127) as u8);
            ctx.radio.set_mic_gain(cfg.mic_gain);
            ctx.radio.set_audio_profile(cfg.audio_profile);
        }
        PAGE_BUTTONS | PAGE_BUTTONS_TWEAKS | PAGE_BUTTONS_SHORTCUTS => {
            ctx.config.mark_keys_dirty();
        }
        PAGE_BATTERY | PAGE_BATTERY_TWEAKS | PAGE_SYSTEM => {
            ctx.config.mark_battery_dirty();
            ctx.config.mark_system_dirty();
            ctx.power.load_battery_config(&ctx.config.battery);
            if ctx.config.battery.cpu_clock == 1 {
                py32_hal::rcc::set_sysclk_72mhz();
            } else {
                py32_hal::rcc::set_sysclk_48mhz();
            }
            bsp::Board::uart_set_baud(ctx.config.system.baud_rate());
        }
        PAGE_DEVICE | PAGE_BOOT_SPLASH => {
            ctx.config.mark_system_dirty();
            ctx.config.mark_display_dirty();
        }
        PAGE_APPS => {
            ctx.config.mark_apps_dirty();
        }
        _ => {}
    }
}

pub struct MenuPageDef {
    pub title: &'static str,
    pub defs: &'static [SettingDef],
}

pub static MANAGE_APPS_PAGE: MenuPageDef = MenuPageDef {
    title: "Installed",
    defs: &[],
};

pub static APP_DETAIL_PAGE: MenuPageDef = MenuPageDef {
    title: "App Info",
    defs: &[],
};

pub static MANAGE_SERVICES_PAGE: MenuPageDef = MenuPageDef {
    title: "Services",
    defs: &[],
};

pub static SERVICE_DETAIL_PAGE: MenuPageDef = MenuPageDef {
    title: "Service Info",
    defs: &[],
};

pub static PAGES: &[MenuPageDef] = &[
    MenuPageDef {
        title: "Settings",
        defs: ROOT_DEFS,
    },
    MenuPageDef {
        title: "Display",
        defs: &super::display::DISPLAY_DEFS,
    },
    MenuPageDef {
        title: "Audio",
        defs: &super::audio::AUDIO_DEFS,
    },
    MenuPageDef {
        title: "Buttons",
        defs: &super::buttons::BUTTONS_ROOT,
    },
    MenuPageDef {
        title: "Battery",
        defs: &super::battery::BATTERY_ROOT,
    },
    MenuPageDef {
        title: "Device",
        defs: &super::device::DEVICE_ROOT,
    },
    MenuPageDef {
        title: "Security",
        defs: &[],
    },
    MenuPageDef {
        title: "System",
        defs: super::system::SYSTEM_ROOT,
    },
    MenuPageDef {
        title: "Tweaks",
        defs: &super::buttons::BUTTONS_TWEAKS,
    },
    MenuPageDef {
        title: "Shortcuts",
        defs: &super::buttons::BUTTONS_SHORTCUTS,
    },
    MenuPageDef {
        title: "Battery Info",
        defs: &super::battery::BATTERY_INFO,
    },
    MenuPageDef {
        title: "Battery Tweaks",
        defs: &super::battery::BATTERY_TWEAKS,
    },
    MenuPageDef {
        title: "Storage",
        defs: &super::system::SYSTEM_STORAGE,
    },
    MenuPageDef {
        title: "Apps",
        defs: super::apps::APPS_DEFS,
    },
    MenuPageDef {
        title: "Boot Splash",
        defs: super::device::SPLASH_PAGE_DEFS,
    },
    #[cfg(feature = "settings_calib")]
    MenuPageDef {
        title: "Calibration",
        defs: &super::system::SYSTEM_CALIBRATION,
    },
    #[cfg(feature = "settings_debug")]
    MenuPageDef {
        title: "Debug",
        defs: &super::system::SYSTEM_DEBUG,
    },
];

#[inline(always)]
pub fn find_page(page_id: u8) -> Option<&'static MenuPageDef> {
    match page_id {
        PAGE_MANAGE_APPS => Some(&MANAGE_APPS_PAGE),
        PAGE_APP_DETAIL => Some(&APP_DETAIL_PAGE),
        PAGE_MANAGE_SERVICES => Some(&MANAGE_SERVICES_PAGE),
        PAGE_SERVICE_DETAIL => Some(&SERVICE_DETAIL_PAGE),
        _ => PAGES.get(page_id as usize),
    }
}
