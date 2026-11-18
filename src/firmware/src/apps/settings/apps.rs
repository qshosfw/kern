//! Dynamic Application & Service Manager for OS Settings.
//!
//! Provides zero-alloc, lightweight package inspection and management,
//! directly querying the system `PackageCatalog` without code duplication.

use super::page::{ChoiceId, PageId, SettingDef, SettingField};
use crate::SysContext;
use crate::apps::launcher::catalog::get_catalog;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub static LAUNCHER_STYLE_OPTIONS: &str = "Grid\0List\0";
pub static STARTUP_APP_OPTIONS: &str = "Home\0Radio\0";

pub static APPS_DEFS: &[SettingDef] = &[
    SettingDef::submenu("Installed", PageId::ManageApps),
    SettingDef::submenu("Services", PageId::ManageServices),
    SettingDef::choice(
        "Style",
        ChoiceId::LauncherStyle,
        SettingField::LAUNCHER_STYLE,
    ),
    SettingDef::toggle("Key Badges", SettingField::SHOW_NUMBERS),
    SettingDef::toggle("App Titles", SettingField::SHOW_LABELS),
    SettingDef::choice(
        "Startup App",
        ChoiceId::StartupApp,
        SettingField::STARTUP_APP,
    ),
];

fn format_size_into(bytes: usize, s: &mut ShortString) {
    let (val, dec, unit) = if bytes < 1024 {
        (bytes as i32, 0, " B")
    } else {
        (((bytes * 10 + 512) >> 10) as i32, 1, " KB")
    };
    s.push_int_fixed(val, dec, "", unit);
}

#[inline(never)]
fn resolve_icon_id(title: &str, icon_id: u16) -> u16 {
    if icon_id >= 22 {
        return icon_id;
    }
    let b = title.as_bytes();
    let c0 = b.first().copied().unwrap_or(0) & !0x20;
    match c0 {
        b'B' if (b.get(2).copied().unwrap_or(0) & !0x20) == b'A' => 0xFF,
        b'T' => 28,
        b'S' => 30,
        b'R' => 25,
        b'C' => 1,
        b'B' => 29,
        _ => 46,
    }
}

/// Resolves only the 8x8 monochrome icon bitmap without referencing 16x16 atlases.
#[inline(never)]
pub fn package_icon_8(title: &str, icon_id: u16) -> &'static [u8; 8] {
    let eff = resolve_icon_id(title, icon_id);
    if eff == 0xFF {
        return gfx::icons::ICON_ANTENNA;
    }
    if eff == 1 {
        return gfx::icons::ICON_KEYPAD;
    }
    if eff >= 22 {
        if let Some(bmp) = gfx::icons::SYSTEM_ICONS_8.get((eff - 22) as usize) {
            return bmp;
        }
    }
    gfx::icons::ICON_APPS
}

/// Resolves 8x8 and 16x16 icons for a packaged `.krx` entry using its embedded
/// `icon_id` (`SYSTEM_ICONS_8` offset `22 + idx`, e.g. `30` = `ICON_APP_ABOUT`),
/// falling back to title matching for packages built with the default `icon_id <= 1`.
pub fn package_icons(title: &str, icon_id: u16) -> (&'static [u8; 8], &'static [u8; 32]) {
    let eff = resolve_icon_id(title, icon_id);
    let icon_8 = package_icon_8(title, icon_id);
    let icon_16 = match eff {
        0xFF => &gfx::icons::ICON_16_SCANNER,
        25 => &gfx::icons::ICON_16_RADIO,
        26 | 38 => &gfx::icons::ICON_16_SETTINGS,
        28 => &gfx::icons::ICON_16_FLASHLIGHT,
        29 => &gfx::icons::ICON_16_DIAGNOSTICS,
        30 => &gfx::icons::ICON_16_INFO,
        _ => &gfx::icons::ICON_16_APPS,
    };
    (icon_8, icon_16)
}

pub fn app_icons(title: &str) -> (&'static [u8; 8], &'static [u8; 32]) {
    package_icons(title, 0)
}

#[inline(never)]
pub fn populate_package_list(
    items: &mut [MenuItem],
    icons: &mut [&'static [u8; 8]],
    _ctx: &mut SysContext,
    is_service: bool,
) -> usize {
    #[cfg(feature = "filesystem")]
    get_catalog().scan(_ctx.storage);

    let cat = get_catalog();
    let mut count = 0;
    let max = items.len().min(icons.len());

    for entry in cat.entries.iter() {
        if count >= max {
            break;
        }
        if !crate::apps::launcher::catalog::PackageCatalog::matches_kind(entry, is_service) {
            continue;
        }
        let item = &mut items[count];
        item.title = entry.title.as_str();
        item.item_type = MenuItemType::Value(ShortString::empty());
        if let MenuItemType::Value(ref mut s) = item.item_type {
            if is_service {
                s.set_str(entry.exec_type.as_str());
            } else if entry.is_valid {
                format_size_into(entry.size_bytes, s);
            } else {
                s.set_str("Invalid");
            }
        }
        icons[count] = package_icons(entry.title.as_str(), entry.icon_id).0;
        count += 1;
    }

    if count == 0 && !items.is_empty() {
        add_detail_row(
            items,
            &mut count,
            if is_service { "No Services" } else { "No Apps" },
            "In /apps/",
        );
        if !icons.is_empty() {
            icons[0] = gfx::icons::ICON_APPS;
        }
        return 1;
    }
    count
}

#[inline(never)]
fn push_item(items: &mut [MenuItem], n: &mut usize, title: &'static str, ty: MenuItemType) {
    if *n < items.len() {
        items[*n].title = title;
        items[*n].item_type = ty;
        *n += 1;
    }
}

#[inline(never)]
fn add_detail_row(items: &mut [MenuItem], n: &mut usize, title: &'static str, val: &str) {
    push_item(
        items,
        n,
        title,
        MenuItemType::Value(ShortString::from_str(val)),
    );
}

#[inline(never)]
pub fn populate_package_detail(items: &mut [MenuItem], is_service: bool, idx: usize) -> usize {
    let cat = get_catalog();
    let pkg = cat.get_entry(is_service, idx);

    let mut n = 0;
    let Some(pkg) = pkg else {
        add_detail_row(items, &mut n, "Status", "Empty");
        return n;
    };

    if !is_service && pkg.is_valid {
        push_item(items, &mut n, "Launch App", MenuItemType::Action);
        if pkg.has_config {
            push_item(items, &mut n, "App Settings", MenuItemType::Submenu);
        }
    }

    add_detail_row(items, &mut n, "Name", pkg.title.as_str());
    if is_service {
        add_detail_row(items, &mut n, "Role", pkg.exec_type.as_str());
    }
    add_detail_row(items, &mut n, "Class", pkg.app_class.as_str());
    let mut size_s = ShortString::empty();
    format_size_into(pkg.size_bytes, &mut size_s);
    add_detail_row(items, &mut n, "Size", size_s.as_str());
    add_detail_row(items, &mut n, "File", pkg.filename());
    push_item(items, &mut n, "Delete", MenuItemType::Action);
    n
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn prompt_delete_package(app: &super::app::SettingsApp, ctx: &mut SysContext) {
    let is_service = app.current_page == PageId::ServiceDetail;
    let cat = get_catalog();
    if let Some(pkg) = cat.get_entry(is_service, app.selected_pkg_idx) {
        let mut msg = heapless::String::<48>::new();
        let _ = msg.push_str(pkg.filename());
        let _ = msg.push_str("\nCannot be undone.");
        super::dialog::show_destructive_hold_confirm(
            ctx,
            super::dialog::TAG_DELETE_PACKAGE,
            "Delete Package?",
            msg.as_str(),
            15,
        );
    }
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn delete_selected_package(app: &mut super::app::SettingsApp, ctx: &mut SysContext) {
    let is_service = app.current_page == PageId::ServiceDetail;
    let cat = get_catalog();
    if let Some(pkg) = cat.get_entry(is_service, app.selected_pkg_idx) {
        let path = pkg.path.clone();
        let _ = ctx.storage.remove(path.as_str());
        crate::apps::launcher::reload_launcher(ctx.storage);
        ctx.show_popup(super::dialog::TAG_POPUP_INFO, "Package", "Deleted");
        app.pop_submenu(ctx);
    }
}

pub fn handle_package_press(
    app: &mut super::app::SettingsApp,
    ctx: &mut SysContext,
    sel_idx: usize,
) -> bool {
    match app.current_page {
        PageId::ManageApps | PageId::ManageServices => {
            let is_service = app.current_page == PageId::ManageServices;
            let cat = get_catalog();
            if sel_idx < cat.count_entries(is_service) {
                app.selected_pkg_idx = sel_idx;
                app.push_submenu(
                    if is_service {
                        PageId::ServiceDetail
                    } else {
                        PageId::AppDetail
                    },
                    ctx,
                );
            }
            true
        }
        PageId::AppDetail | PageId::ServiceDetail => {
            let is_service = app.current_page == PageId::ServiceDetail;
            let cat = get_catalog();
            let pkg = cat.get_entry(is_service, app.selected_pkg_idx);

            let Some(pkg) = pkg else {
                return true;
            };

            if !is_service && pkg.is_valid && sel_idx == 0 {
                let path = pkg.path.as_str();
                app.resume_nav = true;
                if let Err(err) = crate::apps::runtime::launch_krx_package(ctx, path) {
                    app.resume_nav = false;
                    crate::runtime::loader::show_launch_error_dialog(ctx, path, err);
                }
            } else if let Some(item) = app.items.get(sel_idx) {
                if item.title == "App Settings" {
                    app.push_submenu(PageId::AppPrefs, ctx);
                } else if item.title == "Delete" {
                    #[cfg(feature = "filesystem")]
                    prompt_delete_package(app, ctx);
                }
            }
            true
        }
        _ => false,
    }
}
