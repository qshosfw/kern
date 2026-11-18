//! Buttons Domain Settings, Key Tweaks & Dynamic Badge-Backed Shortcut Binder

use super::page::{ChoiceId, PageId, SettingDef, SettingField};
use crate::SysContext;
use crate::apps::shortcut::{
    CTX_OPTIONS, KEY_SHORTCUT_COUNT, describe, format_label, resolve_slot, slot_label,
};
use gfx::gui::modules::menu::{MenuItem, MenuItemType, SheetOptions};

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
    items[0] = MenuItem::new(
        "Scope",
        MenuItemType::Choice {
            selected: cur_ctx.min(4) as u16,
            options: SheetOptions::Packed(CTX_OPTIONS),
        },
    );
    icons[0] = gfx::icons::ICON_APPS;
    let count = KEY_SHORTCUT_COUNT
        .min(items.len().saturating_sub(1))
        .min(icons.len().saturating_sub(1));
    for i in 0..count {
        let target_id = resolve_slot(&ctx.config.keys, i, cur_ctx);
        let (mut label, icon) = describe(target_id, cat);
        if cur_ctx != 0
            && ctx
                .config
                .keys
                .ctx_rules
                .iter()
                .any(|r| r.ctx == cur_ctx && r.slot as usize == i)
        {
            label = format_label("*", label.as_str());
        }
        items[1 + i] = MenuItem::new(slot_label(i), MenuItemType::Shortcut(label));
        icons[1 + i] = icon;
    }
    count + 1
}
