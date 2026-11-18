//! Buttons Domain Settings, Key Tweaks & Dynamic Badge-Backed Shortcut Binder

use super::page::{ChoiceId, PageId, SettingDef, SettingField};
use crate::SysContext;
use crate::apps::shortcut::{
    CTX_OPTIONS, KEY_SHORTCUT_COUNT, describe_into, resolve_slot_override, slot_label,
};
use gfx::gui::modules::menu::{MenuItem, MenuItemType, SheetOptions, ShortString};

pub static PTT_MODE_OPTIONS: &str = "Momentary\0Toggle / Latch\02x Press Latch\0";

pub static HOLD_DELAY_OPTIONS: &str = "250ms\0350ms\0500ms\0750ms\0";

pub static REPEAT_RATE_OPTIONS: &str = "30ms (Turbo)\050ms (Fast)\080ms (Norm)\0120ms (Slow)\0";

pub static F_KEY_TIMEOUT_OPTIONS: &str = "Toggle\01s\02s\03s\05s\0";

pub static AUTO_LOCK_OPTIONS: &str = "Off\015s\030s\01 min\02 min\0";

pub static BUTTONS_ROOT: [SettingDef; 7] = [
    SettingDef::choice("PTT Mode", ChoiceId::PttMode, SettingField::PTT_MODE),
    SettingDef::choice("Auto Lock", ChoiceId::AutoLock, SettingField::AUTO_LOCK),
    SettingDef::choice(
        "Sticky F-Key",
        ChoiceId::FKeyTimeout,
        SettingField::F_KEY_TIMEOUT,
    ),
    SettingDef::toggle("F-Drawer", SettingField::F_DRAWER),
    SettingDef::toggle("Fast Scroll", SettingField::FAST_SCROLL),
    SettingDef::submenu("Tweaks", PageId::ButtonsTweaks),
    SettingDef::submenu("Shortcuts", PageId::ButtonsShortcuts),
];

pub static BUTTONS_TWEAKS: [SettingDef; 2] = [
    SettingDef::choice("Hold Delay", ChoiceId::HoldDelay, SettingField::HOLD_DELAY),
    SettingDef::choice(
        "Repeat Rate",
        ChoiceId::RepeatRate,
        SettingField::REPEAT_RATE,
    ),
];

pub static BUTTONS_SHORTCUTS: [SettingDef; 0] = [];

#[inline(never)]
pub fn populate_shortcuts(
    items: &mut [MenuItem],
    icons: &mut [&'static [u8; 8]],
    cur_ctx: u8,
    ctx: &mut SysContext,
) -> usize {
    let cat = crate::apps::launcher::catalog::get_catalog();
    cat.scan(ctx.storage);
    items[0].title = "Scope";
    items[0].item_type = MenuItemType::Choice {
        selected: cur_ctx.min(4) as u16,
        options: SheetOptions::Packed(CTX_OPTIONS),
    };
    icons[0] = gfx::icons::ICON_APPS;
    let count = KEY_SHORTCUT_COUNT
        .min(items.len().saturating_sub(1))
        .min(icons.len().saturating_sub(1));
    for i in 0..count {
        let (target_id, overridden) = resolve_slot_override(&ctx.config.keys, i, cur_ctx);
        let mut label = ShortString::empty();
        icons[1 + i] = describe_into(target_id, cat, overridden, &mut label);
        items[1 + i].title = slot_label(i);
        items[1 + i].item_type = MenuItemType::Shortcut(label);
    }
    count + 1
}
