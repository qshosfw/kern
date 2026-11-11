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

fn get_action_value(action: ActionId, ctx: &mut crate::SysContext, out: &mut ShortString) {
    match action {
        ActionId::ChooseModel => out.set_str(ctx.config.system.model_str()),
        #[cfg(feature = "text_input")]
        ActionId::EditOwner => {
            super::metric::format_metric(super::page::MetricId::DeviceOwner, ctx, out)
        }
        #[cfg(feature = "text_input")]
        ActionId::EditTag => {
            super::metric::format_metric(super::page::MetricId::DeviceTag, ctx, out)
        }
        #[cfg(feature = "text_input")]
        ActionId::EditRadioId => {
            super::metric::format_metric(super::page::MetricId::DeviceRadioId, ctx, out)
        }
        _ => {}
    }
}

#[inline(always)]
fn make_choice(packed: &'static str, raw_byte: u8) -> MenuItemType {
    let opts = gfx::gui::modules::menu::SheetOptions::Packed(packed);
    let sel = (raw_byte as usize).min(opts.len().saturating_sub(1)) as u16;
    MenuItemType::Choice {
        selected: sel,
        options: opts,
    }
}

#[inline(never)]
fn populate_item(
    item: &mut MenuItem,
    def: &SettingDef,
    ctx: &mut crate::SysContext,
    nav_layout: u8,
) {
    item.title = def.title;
    match def.kind() {
        SettingKind::Choice(opts) => {
            item.item_type = make_choice(opts, def.field.get_u8(&ctx.config));
        }
        SettingKind::BatteryType => {
            let (opts, types) = super::battery::battery_options_and_types(nav_layout);
            let raw_byte = def.field.get_u8(&ctx.config);
            let mut sel = 0u16;
            for (i, &t) in types.iter().enumerate() {
                if t == raw_byte {
                    sel = i as u16;
                    break;
                }
            }
            item.item_type = MenuItemType::Choice {
                selected: sel,
                options: gfx::gui::modules::menu::SheetOptions::Static(opts),
            };
        }
        SettingKind::Range(range) => {
            let raw_val = if def.field == SettingField::CALIB_TWEAK_MV {
                def.field.get_i16(&ctx.config)
            } else {
                def.field.get_u8(&ctx.config) as i16
            };
            item.item_type = MenuItemType::Range {
                val: raw_val.clamp(range.min, range.max),
                min: range.min,
                max: range.max,
                step: range.step,
                unit: range.unit,
            };
        }
        SettingKind::Toggle => {
            item.item_type = MenuItemType::Toggle(def.field.get_u8(&ctx.config) != 0);
        }
        SettingKind::Submenu(_) => {
            item.item_type = MenuItemType::Submenu;
        }
        SettingKind::Info => {
            item.item_type = MenuItemType::Info("Ascon-128");
        }
        SettingKind::Metric(metric) => {
            let mut s = ShortString::empty();
            super::metric::format_metric(metric, ctx, &mut s);
            item.item_type = MenuItemType::Value(s);
        }
        #[cfg(feature = "boot_assets")]
        SettingKind::Asset(desc) => {
            let mut s = ShortString::empty();
            desc.format_status(&ctx.config, &mut s);
            item.item_type = MenuItemType::Value(s);
        }
        SettingKind::Action(action) => {
            let mut s = ShortString::empty();
            get_action_value(action, ctx, &mut s);
            item.item_type = MenuItemType::Value(s);
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
        populate_item(&mut items[i], def, ctx, nav_layout);
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
        PageId::Battery | PageId::BatteryTweaks => {
            ctx.config.mark_battery_dirty();
            ctx.power.load_battery_config(&ctx.config.battery);
        }
        PageId::System => {
            ctx.config.mark_system_dirty();
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
pub fn page_title(page: PageId) -> &'static str {
    match page {
        PageId::Root => "Settings",
        PageId::Display => "Display",
        PageId::Audio => "Audio",
        PageId::Buttons => "Buttons",
        PageId::Battery => "Battery",
        PageId::Device => "Device",
        PageId::Security => "Security",
        PageId::System => "System",
        PageId::ButtonsTweaks => "Tweaks",
        PageId::ButtonsShortcuts => "Shortcuts",
        PageId::BatteryInfo => "Battery Info",
        PageId::BatteryTweaks => "Battery Tweaks",
        PageId::SystemStorage => "Storage",
        PageId::Apps => "Apps",
        PageId::BootSplash => "Boot Splash",
        #[cfg(feature = "settings_calib")]
        PageId::SystemCalibration => "Calibration",
        #[cfg(feature = "settings_debug")]
        PageId::SystemDebug => "Debug",
        PageId::ManageApps => "Installed",
        PageId::AppDetail => "App Info",
        PageId::ManageServices => "Services",
        PageId::ServiceDetail => "Service Info",
    }
}

#[inline(never)]
pub fn page_defs(page: PageId) -> &'static [SettingDef] {
    match page {
        PageId::Root => ROOT_DEFS,
        PageId::Display => super::display::DISPLAY_DEFS,
        PageId::Audio => &super::audio::AUDIO_DEFS,
        PageId::Buttons => &super::buttons::BUTTONS_ROOT,
        PageId::Battery => &super::battery::BATTERY_ROOT,
        PageId::Device => &super::device::DEVICE_ROOT,
        PageId::System => super::system::SYSTEM_ROOT,
        PageId::ButtonsTweaks => &super::buttons::BUTTONS_TWEAKS,
        PageId::ButtonsShortcuts => &super::buttons::BUTTONS_SHORTCUTS,
        PageId::BatteryInfo => &super::battery::BATTERY_INFO,
        PageId::BatteryTweaks => &super::battery::BATTERY_TWEAKS,
        PageId::SystemStorage => &super::system::SYSTEM_STORAGE,
        PageId::Apps => super::apps::APPS_DEFS,
        PageId::BootSplash => super::device::SPLASH_PAGE_DEFS,
        #[cfg(feature = "settings_calib")]
        PageId::SystemCalibration => &super::system::SYSTEM_CALIBRATION,
        #[cfg(feature = "settings_debug")]
        PageId::SystemDebug => &super::system::SYSTEM_DEBUG,
        _ => &[],
    }
}


