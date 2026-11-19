use crate::services::power::PowerServiceConfigExt;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub use super::page::*;

setting_page!(pub ROOT_PAGE = [
    ("Display", SettingDef::submenu(PageId::Display)),
    ("Audio", SettingDef::submenu(PageId::Audio)),
    ("Buttons", SettingDef::submenu(PageId::Buttons)),
    ("Battery", SettingDef::submenu(PageId::Battery)),
    ("Device", SettingDef::submenu(PageId::Device)),
    ("Apps", SettingDef::submenu(PageId::Apps)),
    #[cfg(feature = "passcode")]
    ("Security", SettingDef::submenu(PageId::Security)),
    ("System", SettingDef::submenu(PageId::System)),
]);

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
        #[allow(unreachable_patterns)]
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
fn populate_item(item: &mut MenuItem, spec: &PageSpec, idx: usize, ctx: &mut crate::SysContext) {
    let def = spec.defs[idx];
    item.title = spec.title(idx);
    match def.tag {
        TAG_CHOICE | TAG_BATTERY_TYPE | TAG_TOGGLE => {
            let raw_byte = def.field.get_u8(&ctx.config);
            if def.tag == TAG_TOGGLE {
                item.item_type = MenuItemType::Toggle(raw_byte != 0);
            } else if def.tag == TAG_CHOICE {
                if let Some(opts) = choice_options(def.aux) {
                    item.item_type = make_choice(opts, raw_byte);
                }
            } else {
                let (opts, types) =
                    super::battery::battery_options_and_types(ctx.config.keys.nav_layout);
                let mut sel = 0u16;
                for (i, &t) in types.iter().enumerate() {
                    if t == raw_byte {
                        sel = i as u16;
                        break;
                    }
                }
                item.item_type = MenuItemType::Choice {
                    selected: sel,
                    options: gfx::gui::modules::menu::SheetOptions::Packed(opts),
                };
            }
        }
        TAG_RANGE => {
            if let Some(range) = RANGES.get(def.aux as usize) {
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
        }
        TAG_SUBMENU => {
            item.item_type = MenuItemType::Submenu;
        }
        TAG_METRIC | TAG_ACTION | TAG_ASSET => {
            item.item_type = MenuItemType::Value(ShortString::empty());
            let MenuItemType::Value(s) = &mut item.item_type else {
                return;
            };
            if def.tag == TAG_METRIC {
                if let Some(metric) = MetricId::from_u8(def.aux) {
                    super::metric::format_metric(metric, ctx, s);
                }
            } else if def.tag == TAG_ACTION {
                if let Some(action) = ActionId::from_u8(def.aux) {
                    get_action_value(action, ctx, s);
                }
            } else {
                #[cfg(feature = "filesystem")]
                if let Some(&desc) = ASSETS.get(def.aux as usize) {
                    desc.format_status(&ctx.config, s);
                }
            }
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

#[inline(never)]
pub fn populate_spec_range(
    items: &mut [MenuItem],
    spec: &PageSpec,
    start: usize,
    end: usize,
    ctx: &mut crate::SysContext,
) -> usize {
    let end = end.min(spec.defs.len());
    let n = end.saturating_sub(start).min(items.len());
    for i in 0..n {
        populate_item(&mut items[i], spec, start + i, ctx);
    }
    n
}

static PAGE_DIRTY_MASK: [u8; PageId::COUNT] = [
    0,                   // Root
    1 << 0,              // Display
    1 << 1,              // Audio
    1 << 2,              // Buttons
    1 << 4,              // Battery
    (1 << 3) | (1 << 0), // Device
    #[cfg(feature = "passcode")]
    0, // Security
    1 << 3,              // System
    1 << 2,              // ButtonsTweaks
    1 << 2,              // ButtonsShortcuts
    0,                   // BatteryInfo
    1 << 4,              // BatteryTweaks
    #[cfg(feature = "settings_info")]
    0, // SystemInfo
    #[cfg(feature = "filesystem")]
    0, // SystemStorage
    1 << 5,              // Apps
    (1 << 3) | (1 << 0), // BootSplash
    #[cfg(feature = "settings_calib")]
    0, // SystemCalibration
    #[cfg(feature = "settings_debug")]
    0, // SystemDebug
    0,                   // AppPrefs
];

#[inline(never)]
pub fn apply_page_changes(page: PageId, ctx: &mut crate::SysContext) {
    let mask = PAGE_DIRTY_MASK[page as usize];
    if mask == 0 {
        return;
    }
    ctx.config.mark_dirty_mask(mask);
    if page == PageId::Display {
        ctx.apply_display_config();
    } else if page == PageId::Audio {
        ctx.apply_audio_config();
    } else if (mask & (1 << 4)) != 0 {
        ctx.power.load_battery_config(&ctx.config.battery);
    }
}

const PAGE_TITLE_LIST: &[&str] = &[
    "Settings",
    "Display",
    "Audio",
    "Buttons",
    "Battery",
    "Device",
    #[cfg(feature = "passcode")]
    "Security",
    "System",
    "Tweaks",
    "Shortcuts",
    "Battery Info",
    "Battery Tweaks",
    #[cfg(feature = "settings_info")]
    "System Info",
    #[cfg(feature = "filesystem")]
    "Storage",
    "Apps",
    "Boot Splash",
    #[cfg(feature = "settings_calib")]
    "Calibration",
    #[cfg(feature = "settings_debug")]
    "Debug",
    "App Settings",
];

const _: () = assert!(PAGE_TITLE_LIST.len() == PageId::COUNT);

const PAGE_TITLES_LEN: usize = {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < PAGE_TITLE_LIST.len() {
        n += PAGE_TITLE_LIST[i].len() + 1;
        i += 1;
    }
    n
};

static PAGE_TITLES: [u8; PAGE_TITLES_LEN] = {
    let mut buf = [0u8; PAGE_TITLES_LEN];
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < PAGE_TITLE_LIST.len() {
        let s = PAGE_TITLE_LIST[i].as_bytes();
        let mut j = 0usize;
        while j < s.len() {
            buf[pos] = s[j];
            pos += 1;
            j += 1;
        }
        pos += 1;
        i += 1;
    }
    buf
};

#[inline(never)]
pub fn page_title(page: PageId) -> &'static str {
    utils::format::nth_cstr(&PAGE_TITLES, page as u8)
}

static EMPTY_PAGE: PageSpec = PageSpec::EMPTY;

static PAGE_SPECS: [&'static PageSpec; PageId::COUNT] = [
    &ROOT_PAGE,
    &super::display::DISPLAY_PAGE,
    &super::audio::AUDIO_PAGE,
    &super::buttons::BUTTONS_ROOT_PAGE,
    &super::battery::BATTERY_ROOT_PAGE,
    &super::device::DEVICE_ROOT_PAGE,
    #[cfg(feature = "passcode")]
    &EMPTY_PAGE,
    &super::system::SYSTEM_ROOT_PAGE,
    &super::buttons::BUTTONS_TWEAKS_PAGE,
    &EMPTY_PAGE,
    &super::battery::BATTERY_INFO_PAGE,
    &super::battery::BATTERY_TWEAKS_PAGE,
    #[cfg(feature = "settings_info")]
    &EMPTY_PAGE,
    #[cfg(feature = "filesystem")]
    &super::system::SYSTEM_STORAGE_PAGE,
    &super::apps::APPS_PAGE,
    &super::device::SPLASH_PAGE,
    #[cfg(feature = "settings_calib")]
    &super::system::SYSTEM_CALIBRATION_PAGE,
    #[cfg(feature = "settings_debug")]
    &super::system::SYSTEM_DEBUG_PAGE,
    &EMPTY_PAGE,
];

#[inline(always)]
pub fn page_spec(page: PageId) -> &'static PageSpec {
    PAGE_SPECS[page as usize]
}

#[inline(always)]
pub fn page_defs(page: PageId) -> &'static [SettingDef] {
    page_spec(page).defs
}
