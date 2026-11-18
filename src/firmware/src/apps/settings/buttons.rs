//! Buttons Domain Settings, Key Tweaks & Dynamic Badge-Backed Shortcut Binder

use super::page::{ChoiceId, PageId, SettingDef, SettingField};
use crate::SysContext;
use crate::apps::shortcut::{
    CTX_OPTIONS, KEY_SHORTCUT_COUNT, SLOT_OPTIONS, ctx_label, describe, format_label, slot_label,
};
use gfx::gui::modules::menu::{MenuItem, MenuItemType, SheetOptions, ShortString};

pub static PTT_MODE_OPTIONS: &str = "Momentary\0Toggle / Latch\02x Press Latch\0";

pub static HOLD_DELAY_OPTIONS: &str = "250ms\0350ms\0500ms\0750ms\0";

pub static REPEAT_RATE_OPTIONS: &str = "30ms (Turbo)\050ms (Fast)\080ms (Norm)\0120ms (Slow)\0";

pub static F_KEY_TIMEOUT_OPTIONS: &str = "Toggle\01s\02s\03s\05s\0";

pub static AUTO_LOCK_OPTIONS: &str = "Off\015s\030s\01 min\02 min\0";

const RULE_BADGES: [&str; 3] = ["R1", "R2", "R3"];

pub static BUTTONS_ROOT: [SettingDef; 8] = [
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
    SettingDef::submenu("Context Rules", PageId::ShortcutRules),
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
    ctx: &mut SysContext,
) -> usize {
    let cat = crate::apps::launcher::catalog::get_catalog();
    cat.scan(ctx.storage);
    let count = KEY_SHORTCUT_COUNT.min(items.len()).min(icons.len());
    for i in 0..count {
        let (label, icon) = describe(ctx.config.keys.actions[i], cat);
        items[i] = MenuItem::new(slot_label(i), MenuItemType::Shortcut(label));
        icons[i] = icon;
    }
    count
}

#[inline(never)]
pub fn populate_shortcut_rules(
    items: &mut [MenuItem],
    icons: &mut [&'static [u8; 8]],
    ctx: &mut SysContext,
) -> usize {
    let cat = crate::apps::launcher::catalog::get_catalog();
    cat.scan(ctx.storage);
    let n = 3usize.min(items.len()).min(icons.len());
    for i in 0..n {
        let r = ctx.config.keys.ctx_rules[i];
        if r.ctx == 0 || (r.slot as usize) >= KEY_SHORTCUT_COUNT {
            items[i] = MenuItem::new(RULE_BADGES[i], MenuItemType::Shortcut(ShortString::from_str("Off")));
            icons[i] = gfx::icons::RADIO_OFF;
        } else {
            let (lbl, icon) = describe(r.action, cat);
            items[i] = MenuItem::new(
                slot_label(r.slot as usize),
                MenuItemType::Shortcut(format_label(ctx_label(r.ctx), lbl.as_str())),
            );
            icons[i] = icon;
        }
    }
    n
}

#[inline(never)]
pub fn populate_shortcut_rule_detail(
    items: &mut [MenuItem],
    icons: &mut [&'static [u8; 8]],
    rule_idx: u8,
    ctx: &mut SysContext,
) -> usize {
    let cat = crate::apps::launcher::catalog::get_catalog();
    cat.scan(ctx.storage);
    let r = ctx.config.keys.ctx_rules[rule_idx.min(2) as usize];
    let slot = (r.slot as usize).min(KEY_SHORTCUT_COUNT - 1);
    let (lbl, icon) = describe(r.action, cat);
    items[0] = MenuItem::new(
        "Context",
        MenuItemType::Choice {
            selected: (r.ctx.min(4)) as u16,
            options: SheetOptions::Packed(CTX_OPTIONS),
        },
    );
    items[1] = MenuItem::new(
        "Key",
        MenuItemType::Choice {
            selected: slot as u16,
            options: SheetOptions::Packed(SLOT_OPTIONS),
        },
    );
    items[2] = MenuItem::new(slot_label(slot), MenuItemType::Shortcut(lbl));
    icons[0] = gfx::icons::ICON_APPS;
    icons[1] = gfx::icons::ICON_BUTTONS;
    icons[2] = icon;
    3
}
