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
    #[cfg(feature = "filesystem")]
    ("Language", SettingDef::asset(AssetId::Language)),
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

fn populate_item(item: &mut MenuItem, spec: &PageSpec, idx: usize, ctx: &mut crate::SysContext) {
    let def = spec.defs[idx];
    item.title = spec.title(idx);
    match def.tag {
        TAG_TOGGLE => {
            item.item_type = MenuItemType::Toggle(def.field.get_u8(&ctx.config) != 0);
        }
        TAG_CHOICE | TAG_BATTERY_TYPE => {
            let raw_byte = def.field.get_u8(&ctx.config);
            let (packed, sel) = if def.tag == TAG_CHOICE {
                let Some(opts) = choice_options(def.aux) else {
                    return;
                };
                let max_sel = gfx::gui::modules::menu::SheetOptions::Packed(opts)
                    .len()
                    .saturating_sub(1);
                (opts, (raw_byte as usize).min(max_sel) as u16)
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
                (opts, sel)
            };
            item.item_type = MenuItemType::Choice {
                selected: sel,
                options: gfx::gui::modules::menu::SheetOptions::Packed(packed),
            };
        }
        TAG_RANGE => {
            let (min, max, step) = range_of(def.field);
            item.item_type = MenuItemType::Range {
                val: def.field.get_i16(&ctx.config).clamp(min, max),
                min,
                max,
                step,
                unit: range_unit(def.aux),
            };
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
                if let Some(asset) = AssetId::from_u8(def.aux) {
                    asset.format_status(&ctx.config, s);
                }
            }
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

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
