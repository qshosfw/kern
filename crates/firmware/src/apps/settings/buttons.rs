//! Buttons Domain Settings, Key Tweaks & Dynamic Badge-Backed Shortcut Binder

use super::page::{ChoiceId, PageId, SettingDef, SettingField, setting_page};
use crate::SysContext;
use crate::apps::shortcut::{
    CTX_OPTIONS, KEY_SHORTCUT_COUNT, describe_into, resolve_slot_override, slot_label,
};
use gfx::gui::modules::menu::{MenuItem, MenuItemType, SheetOptions, ShortString};

pub use ::abi::prop::options::{
    AUTO_LOCK as AUTO_LOCK_OPTIONS, F_KEY_TIMEOUT as F_KEY_TIMEOUT_OPTIONS,
    HOLD_DELAY as HOLD_DELAY_OPTIONS, PTT_MODE as PTT_MODE_OPTIONS,
    REPEAT_RATE as REPEAT_RATE_OPTIONS,
};

setting_page!(pub BUTTONS_ROOT_PAGE = [
    ("Shortcuts", SettingDef::submenu(PageId::ButtonsShortcuts)),
    ("PTT Mode", SettingDef::choice(ChoiceId::PttMode, SettingField::PTT_MODE)),
    ("Auto Lock", SettingDef::choice(ChoiceId::AutoLock, SettingField::AUTO_LOCK)),
    ("Sticky F-Key", SettingDef::choice(ChoiceId::FKeyTimeout, SettingField::F_KEY_TIMEOUT)),
    ("Fast Scroll", SettingDef::toggle(SettingField::FAST_SCROLL)),
    ("Tweaks", SettingDef::submenu(PageId::ButtonsTweaks)),
]);

setting_page!(pub BUTTONS_TWEAKS_PAGE = [
    ("Hold Delay", SettingDef::choice(ChoiceId::HoldDelay, SettingField::HOLD_DELAY)),
    ("Repeat Rate", SettingDef::choice(ChoiceId::RepeatRate, SettingField::REPEAT_RATE)),
]);

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
    icons[0] = &gfx::icons::ICON_NONE;
    items[1].title = "Show Drawer";
    items[1].item_type = MenuItemType::Toggle(ctx.config.keys.f_drawer != 0);
    icons[1] = &gfx::icons::ICON_NONE;
    let count = KEY_SHORTCUT_COUNT
        .min(items.len().saturating_sub(2))
        .min(icons.len().saturating_sub(2));
    for i in 0..count {
        let (target_id, overridden) = resolve_slot_override(&ctx.config.keys, i, cur_ctx);
        let mut label = ShortString::empty();
        icons[2 + i] = describe_into(target_id, cat, overridden, &mut label);
        items[2 + i].title = slot_label(i);
        items[2 + i].item_type = MenuItemType::Shortcut(label);
    }
    count + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_is_first_in_buttons_root_and_f_drawer_moved() {
        assert_eq!(BUTTONS_ROOT_PAGE.title(0), "Shortcuts");
        assert!(
            BUTTONS_ROOT_PAGE
                .defs
                .iter()
                .all(|d| d.field != SettingField::F_DRAWER),
            "F-Drawer should live inside Shortcuts, not Buttons root"
        );
    }
}
