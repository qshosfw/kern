#[cfg(feature = "settings_calib")]
use crate::services::calibration::CalibrationService;
use crate::services::power::PowerServiceConfigExt;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub use super::page::*;

pub static ROOT_DEFS: &[SettingDef] = &[
    SettingDef::submenu("Display", PageId::Display),
    SettingDef::submenu("Audio", PageId::Audio),
    SettingDef::submenu("Buttons", PageId::Buttons),
    SettingDef::submenu("Battery", PageId::Battery),
    SettingDef::submenu("Device", PageId::Device),
    SettingDef::submenu("Apps", PageId::Apps),
    #[cfg(feature = "passcode")]
    SettingDef::submenu("Security", PageId::Security),
    SettingDef::submenu("System", PageId::System),
];

#[inline(never)]
fn get_action_value(action: ActionId, ctx: &mut crate::SysContext, out: &mut ShortString) {
    match action {
        ActionId::ChooseModel => out.set_str(ctx.config.system.model_str()),
        #[cfg(feature = "passcode")]
        ActionId::EditOwner => {
            super::metric::format_metric(super::page::MetricId::DeviceOwner, ctx, out)
        }
        #[cfg(feature = "passcode")]
        ActionId::EditTag => {
            super::metric::format_metric(super::page::MetricId::DeviceTag, ctx, out)
        }
        #[cfg(feature = "passcode")]
        ActionId::EditRadioId => {
            super::metric::format_metric(super::page::MetricId::DeviceRadioId, ctx, out)
        }
        _ => {}
    }
}

#[inline(never)]
fn build_item_type(def: &SettingDef, ctx: &mut crate::SysContext, nav_layout: u8) -> MenuItemType {
    match def.kind {
        SettingKind::Choice(opts) => {
            let raw_byte = def.field.get_u8(&ctx.config);
            let sel = (raw_byte as usize).min(opts.len().saturating_sub(1)) as u16;
            MenuItemType::Choice {
                selected: sel,
                options: opts,
            }
        }
        SettingKind::BatteryType => {
            let (opts, types) = super::battery::battery_options_and_types(nav_layout);
            let raw_byte = def.field.get_u8(&ctx.config);
            let sel = types.iter().position(|&t| t == raw_byte).unwrap_or(0) as u16;
            MenuItemType::Choice {
                selected: sel,
                options: opts,
            }
        }
        SettingKind::Range(range) => {
            let raw_val = if def.field == SettingField::CALIB_TWEAK_MV {
                def.field.get_i16(&ctx.config)
            } else {
                def.field.get_u8(&ctx.config) as i16
            };
            MenuItemType::Range {
                val: raw_val.clamp(range.min, range.max),
                min: range.min,
                max: range.max,
                step: range.step,
                unit: range.unit,
            }
        }
        SettingKind::Toggle => MenuItemType::Toggle(def.field.get_u8(&ctx.config) != 0),
        SettingKind::Submenu(_) => MenuItemType::Submenu,
        SettingKind::Info => MenuItemType::Info("Ascon-128"),
        SettingKind::Metric(metric) => {
            let mut s = ShortString::empty();
            super::metric::format_metric(metric, ctx, &mut s);
            MenuItemType::Value(s)
        }
        #[cfg(feature = "boot_assets")]
        SettingKind::Asset(desc) => {
            let mut s = ShortString::empty();
            desc.format_status(&ctx.config, &mut s);
            MenuItemType::Value(s)
        }
        SettingKind::Action(action) => {
            let mut s = ShortString::empty();
            get_action_value(action, ctx, &mut s);
            MenuItemType::Value(s)
        }
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

pub fn apply_page_changes(page: PageId, ctx: &mut crate::SysContext) {
    match page {
        PageId::Display => {
            ctx.config.mark_display_dirty();
            if let Some(ref mut display) = ctx.display {
                display.apply_config(&ctx.config.display);
            }
        }
        PageId::Audio => {
            ctx.config.mark_audio_dirty();
            ctx.apply_audio_config();
        }
        PageId::Buttons | PageId::ButtonsTweaks | PageId::ButtonsShortcuts => {
            ctx.config.mark_keys_dirty();
        }
        PageId::Battery | PageId::BatteryTweaks | PageId::System => {
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
        PageId::Device | PageId::BootSplash => {
            ctx.config.mark_system_dirty();
            ctx.config.mark_display_dirty();
        }
        PageId::Apps => {
            ctx.config.mark_apps_dirty();
        }
        _ => {}
    }
}

#[inline(never)]
pub fn get_page(page: PageId) -> (&'static str, &'static [SettingDef]) {
    match page {
        PageId::Root => ("Settings", ROOT_DEFS),
        PageId::Display => ("Display", super::display::DISPLAY_DEFS),
        PageId::Audio => ("Audio", &super::audio::AUDIO_DEFS),
        PageId::Buttons => ("Buttons", &super::buttons::BUTTONS_ROOT),
        PageId::Battery => ("Battery", &super::battery::BATTERY_ROOT),
        PageId::Device => ("Device", &super::device::DEVICE_ROOT),
        PageId::Security => ("Security", &[]),
        PageId::System => ("System", super::system::SYSTEM_ROOT),
        PageId::ButtonsTweaks => ("Tweaks", &super::buttons::BUTTONS_TWEAKS),
        PageId::ButtonsShortcuts => ("Shortcuts", &super::buttons::BUTTONS_SHORTCUTS),
        PageId::BatteryInfo => ("Battery Info", &super::battery::BATTERY_INFO),
        PageId::BatteryTweaks => ("Battery Tweaks", &super::battery::BATTERY_TWEAKS),
        PageId::SystemStorage => ("Storage", &super::system::SYSTEM_STORAGE),
        PageId::Apps => ("Apps", super::apps::APPS_DEFS),
        PageId::BootSplash => ("Boot Splash", super::device::SPLASH_PAGE_DEFS),
        #[cfg(feature = "settings_calib")]
        PageId::SystemCalibration => ("Calibration", &super::system::SYSTEM_CALIBRATION),
        #[cfg(feature = "settings_debug")]
        PageId::SystemDebug => ("Debug", &super::system::SYSTEM_DEBUG),
        PageId::ManageApps => ("Installed", &[]),
        PageId::AppDetail => ("App Info", &[]),
        PageId::ManageServices => ("Services", &[]),
        PageId::ServiceDetail => ("Service Info", &[]),
    }
}
