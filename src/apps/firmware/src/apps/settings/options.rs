#[cfg(feature = "settings_calib")]
use crate::services::calibration::CalibrationService;
use crate::services::display::DisplayService;
use crate::services::power::{PowerService, PowerServiceConfigExt};
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub use super::apps::{LAUNCHER_STYLE_OPTIONS, STARTUP_APP_OPTIONS, VFO_DUAL_WATCH_OPTIONS};
pub use super::audio::{AUDIO_PROFILE_OPTIONS, BOOT_JINGLE_OPTIONS, KEY_BEEP_OPTIONS};
pub use super::battery::{
    AUTO_POWER_OFF_OPTIONS, BATTERY_FORMAT_OPTIONS, BATTERY_K1_OPTIONS, BATTERY_K1_TYPES,
    BATTERY_K5_OPTIONS, BATTERY_K5_TYPES, POWER_PROFILE_OPTIONS,
};
pub use super::buttons::{
    AUTO_LOCK_OPTIONS, F_KEY_TIMEOUT_OPTIONS, FAST_SCROLL_OPTIONS, HOLD_DELAY_OPTIONS,
    M_LONG_OPTIONS, REPEAT_RATE_OPTIONS, SHORTCUT_OPTIONS,
};
pub use super::device::{BOOT_ANIM_OPTIONS, MODEL_OPTIONS, SPLASH_TIME_OPTIONS};
pub use super::display::{BACKLIGHT_TIME_OPTIONS, BACKLIGHT_TR_OPTIONS};
pub use super::system::CPU_CLOCK_OPTIONS;

pub static OFF_ON: [&'static str; 2] = ["OFF", "ON"];

pub use super::page::*;

static ALL_CHOICE_OPTIONS: [&[&str]; 22] = [
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
    &VFO_DUAL_WATCH_OPTIONS,
    &BOOT_ANIM_OPTIONS,
];

#[inline(always)]
pub fn get_choice_options(id: u8) -> &'static [&'static str] {
    ALL_CHOICE_OPTIONS.get(id as usize).copied().unwrap_or(&[])
}

pub static ROOT_DEFS: [SettingDef; 8] = [
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

#[cfg(feature = "settings_debug")]
#[inline(never)]
fn format_security_metric(metric: u8, ctx: &mut crate::SysContext, out: &mut ShortString) {
    let sec = crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode);
    match metric {
        METRIC_DEBUG_SWD => out.set_str(if sec.swd_open { "Open" } else { "Locked" }),
        METRIC_DEBUG_RDP => out.set_str(sec.rdp.short_str()),
        METRIC_DEBUG_DEVKEY => match sec.dev_key_hash {
            Some(h) => *out = ShortString::from_hex32(h),
            None => out.set_str("None"),
        },
        _ => match sec.master_key_hash {
            Some(h) => *out = ShortString::from_hex32(h),
            None => out.set_str("None"),
        },
    }
}

const METRIC_UNITS: [&str; 8] = ["", "%", " mA", " mV", " MHz", " KB", " sec", " Hz"];

#[inline(never)]
fn format_string_metric(metric: u8, ctx: &mut crate::SysContext, out: &mut ShortString) -> bool {
    let s = match metric {
        METRIC_BATTERY_STATUS => {
            if ctx.power.is_charging() {
                "Charging"
            } else {
                "Discharging"
            }
        }
        #[cfg(feature = "settings_calib")]
        METRIC_CALIB_SIG => {
            if ctx.calibration.is_valid() {
                "Valid"
            } else {
                "None"
            }
        }
        METRIC_PASSCODE_STATUS => {
            if crate::security::passcode::is_passcode_set(ctx.storage) {
                "Set"
            } else {
                "None"
            }
        }
        METRIC_SYS_BUILD => crate::build_info::BUILD_STAMP,
        METRIC_DEVICE_OWNER => ctx.config.system.owner_str(),
        METRIC_DEVICE_ASSET => ctx.config.system.tag_str(),
        _ => return false,
    };
    out.set_str(s);
    true
}

#[inline(never)]
pub fn format_live_metric(metric: u8, ctx: &mut crate::SysContext, out: &mut ShortString) {
    if format_string_metric(metric, ctx, out) {
        return;
    }

    match metric {
        METRIC_BATTERY_VOLTS => {
            let mv = ctx.power.battery_voltage_mv() as u32;
            let adc = ctx.power.raw_battery_adc();
            let v_deci2 = ((mv * 52429) >> 19) as i32;
            out.push_int_fixed(v_deci2, 2, "", "v (");
            out.push_int_fixed(adc as i32, 0, "", ")");
            return;
        }
        METRIC_UPTIME => {
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 8]) };
            let _ = gfx::text::format_uptime_hms(ctx.power.uptime_secs(), arr);
            out.len = 8;
            return;
        }
        METRIC_SYS_SERIAL => {
            let serial = bsp::uid::get_crockford_serial();
            let slice = unsafe { core::str::from_utf8_unchecked(&serial[..14]) };
            out.set_str(slice);
            return;
        }
        METRIC_SYS_MAC => {
            let mac = bsp::uid::read_mac_address();
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 17]) };
            let _ = gfx::text::format_mac_address(&mac, arr);
            out.len = 17;
            return;
        }
        #[cfg(feature = "settings_debug")]
        METRIC_DEBUG_SWD | METRIC_DEBUG_RDP | METRIC_DEBUG_DEVKEY | METRIC_DEBUG_MK => {
            format_security_metric(metric, ctx, out);
            return;
        }
        METRIC_MCU_TEMP => {
            out.push_int_fixed(ctx.power.mcu_temperature_deci_c() as i32, 1, "", " C");
            return;
        }
        _ => {}
    }

    let (v, u) = match metric {
        METRIC_BATTERY_RAW_ADC => (ctx.power.raw_battery_adc() as i32, 0),
        METRIC_BATTERY_PCT => (ctx.power.battery_percentage() as i32, 1),
        METRIC_BATTERY_CURRENT => (ctx.power.battery_current_ma() as i32, 2),
        METRIC_VDDA_REF => (ctx.power.vdda_voltage_mv() as i32, 3),
        METRIC_SYS_CLOCK => (
            if ctx.config.battery.cpu_clock == 1 {
                72
            } else {
                48
            },
            4,
        ),
        METRIC_DEVICE_RADIO_ID => (ctx.config.system.radio_id as i32, 0),
        METRIC_STORAGE_USED
        | METRIC_STORAGE_FREE
        | METRIC_STORAGE_FILES
        | METRIC_STORAGE_WEAR
        | METRIC_STORAGE_DELETED => {
            let stats = ctx.storage.stats();
            match metric {
                METRIC_STORAGE_USED => (stats.used_sectors as i32 * 4, 5),
                METRIC_STORAGE_FREE => (stats.free_sectors as i32 * 4, 5),
                METRIC_STORAGE_FILES => (stats.active_files as i32, 0),
                METRIC_STORAGE_WEAR => (stats.wear_cycles as i32, 0),
                _ => (stats.deleted_sectors as i32, 6),
            }
        }
        #[cfg(feature = "settings_calib")]
        METRIC_BATT_CALIB_REF => (ctx.calibration.battery_calib_const() as i32, 0),
        #[cfg(feature = "settings_calib")]
        METRIC_CALIB_XTAL => (ctx.calibration.xtal_freq_low() as i32, 7),
        #[cfg(feature = "settings_calib")]
        METRIC_CALIB_LNA => (ctx.calibration.lna_calib() as i32, 0),
        #[cfg(feature = "settings_calib")]
        METRIC_CALIB_MIX => (ctx.calibration.mix_calib() as i32, 0),
        #[cfg(feature = "settings_calib")]
        METRIC_CALIB_VOL => (ctx.calibration.volume_gain() as i32, 0),
        _ => return,
    };
    out.push_int_fixed(
        v,
        0,
        "",
        METRIC_UNITS.get(u as usize).copied().unwrap_or(""),
    );
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
        ACTION_SPLASH_CHOOSER => {
            if ctx.config.display.boot_splash_mode == 0 {
                out.set_str("None");
            } else {
                *out = crate::boot::splash::get_splash_name(ctx.config.display.splash_path_str());
            }
        }
        ACTION_ROGER_CHOOSER => {
            if ctx.config.audio.roger_beep == 0 {
                out.set_str("None");
            } else {
                *out = crate::services::melody::get_roger_name(
                    ctx.storage,
                    ctx.config.audio.roger_path_str(),
                );
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

struct RangeDef {
    min: u8,
    max: u8,
    step: u8,
    unit: &'static str,
}

static RANGE_DEFS: [RangeDef; 4] = [
    RangeDef {
        min: 0,
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
        min: 1,
        max: 10,
        step: 1,
        unit: "",
    },
    RangeDef {
        min: 0,
        max: 100,
        step: 5,
        unit: "%",
    },
];

#[inline(never)]
pub fn populate_defs(items: &mut [MenuItem], defs: &[SettingDef], ctx: &mut crate::SysContext) {
    let nav_layout = ctx.config.keys.nav_layout;
    for (i, def) in defs.iter().enumerate() {
        if i >= items.len() {
            break;
        }
        items[i].title = def.title;
        items[i].item_type = match def.kind {
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
                let idx = if (range_id as usize) < RANGE_DEFS.len() {
                    range_id as usize
                } else {
                    0
                };
                let r = &RANGE_DEFS[idx];
                let raw_byte = def.field.get_u8(&ctx.config);
                MenuItemType::Range {
                    val: raw_byte.clamp(r.min, r.max) as i16,
                    min: r.min as i16,
                    max: r.max as i16,
                    step: r.step as i16,
                    unit: r.unit,
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
            SettingKind::Info => MenuItemType::Info("ChaCha20"),
        };
    }
}

#[inline(never)]
pub fn update_single_def(item: &MenuItem, def: &SettingDef, ctx: &mut crate::SysContext) -> bool {
    if def.field == SettingField::None {
        return false;
    }
    let nav_layout = ctx.config.keys.nav_layout;
    let new_val: i32 = match def.kind {
        SettingKind::Choice(_) => {
            if let MenuItemType::Choice { selected, .. } = item.item_type {
                selected as i32
            } else {
                return false;
            }
        }
        SettingKind::BatteryType => {
            if let MenuItemType::Choice { selected, .. } = item.item_type {
                let (_, types) = battery_options_and_types(nav_layout);
                types.get(selected as usize).copied().unwrap_or(types[0]) as i32
            } else {
                return false;
            }
        }
        SettingKind::Range(_) | SettingKind::CalibTweak => {
            if let MenuItemType::Range { val, .. } = item.item_type {
                val as i32
            } else {
                return false;
            }
        }
        SettingKind::Toggle => {
            if let MenuItemType::Toggle(b) = item.item_type {
                b as i32
            } else {
                return false;
            }
        }
        _ => return false,
    };
    def.field.set_val(new_val, &mut ctx.config)
}

#[inline(never)]
pub fn update_defs(items: &[MenuItem], defs: &[SettingDef], ctx: &mut crate::SysContext) -> bool {
    let mut changed = false;
    for (item, def) in items.iter().zip(defs.iter()) {
        if update_single_def(item, def, ctx) {
            changed = true;
        }
    }
    changed
}

pub fn apply_page_changes(page_id: u8, ctx: &mut crate::SysContext) {
    match page_id {
        PAGE_DISPLAY => {
            ctx.config.mark_display_dirty();
            let cfg = ctx.config.display;
            if let Some(ref mut display) = ctx.display {
                display.set_contrast(cfg.contrast);
                display.set_inverted(cfg.inverted != 0);
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
            ctx.power.load_battery_config(&ctx.config.battery);
            if ctx.config.battery.cpu_clock == 1 {
                py32_hal::rcc::set_sysclk_72mhz();
            } else {
                py32_hal::rcc::set_sysclk_48mhz();
            }
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

pub static PAGES: &[MenuPageDef] = &[
    MenuPageDef {
        title: "Settings",
        defs: &ROOT_DEFS,
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
        defs: &super::apps::APPS_DEFS,
    },
    MenuPageDef {
        title: "Boot Splash",
        defs: &super::device::SPLASH_PAGE_DEFS,
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
    PAGES.get(page_id as usize)
}
