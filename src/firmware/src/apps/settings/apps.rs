//! Dynamic Application & Service Manager for OS Settings.
//!
//! Provides zero-alloc, lightweight package inspection and management,
//! directly querying the system `PackageCatalog` without code duplication.

use super::page::{PageId, SettingDef, SettingField};
use crate::SysContext;
use crate::apps::launcher::catalog::get_catalog;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub static LAUNCHER_STYLE_OPTIONS: [&'static str; 2] = ["Grid", "List"];
pub static STARTUP_APP_OPTIONS: [&'static str; 2] = ["Home", "Radio"];

pub static APPS_DEFS: &[SettingDef] = &[
    SettingDef::submenu("Installed", PageId::ManageApps),
    SettingDef::submenu("Services", PageId::ManageServices),
    SettingDef::choice(
        "Style",
        &LAUNCHER_STYLE_OPTIONS,
        SettingField::LAUNCHER_STYLE,
    ),
    SettingDef::toggle("Key Badges", SettingField::SHOW_NUMBERS),
    SettingDef::toggle("App Titles", SettingField::SHOW_LABELS),
    SettingDef::choice(
        "Startup App",
        &STARTUP_APP_OPTIONS,
        SettingField::STARTUP_APP,
    ),
];

fn format_size(bytes: usize) -> ShortString {
    let mut s = ShortString::empty();
    let (val, dec, unit) = if bytes < 1024 {
        (bytes as i32, 0, " B")
    } else {
        (((bytes * 10 + 512) >> 10) as i32, 1, " KB")
    };
    s.push_int_fixed(val, dec, "", unit);
    s
}

#[inline(always)]
pub fn app_icon_8(title: &str) -> &'static [u8; 8] {
    let b = title.as_bytes();
    match b.first().copied().unwrap_or(0) {
        b'B' | b'b' => {
            if b.get(2).copied().unwrap_or(0) == b'a' || b.get(2).copied().unwrap_or(0) == b'A' {
                &gfx::icons::ICON_ANTENNA
            } else {
                &gfx::icons::ICON_APP_DIAG
            }
        }
        b'T' | b't' => &gfx::icons::ICON_APP_TORCH,
        b'S' | b's' => &gfx::icons::ICON_APP_ABOUT,
        b'R' | b'r' => &gfx::icons::ICON_APP_RADIO,
        b'C' | b'c' => &gfx::icons::ICON_KEYPAD,
        _ => &gfx::icons::ICON_APPS,
    }
}

#[inline(always)]
pub fn app_icon_16(title: &str) -> &'static [u8; 32] {
    let b = title.as_bytes();
    match b.first().copied().unwrap_or(0) {
        b'B' | b'b' => {
            if b.get(2).copied().unwrap_or(0) == b'a' || b.get(2).copied().unwrap_or(0) == b'A' {
                &gfx::icons::ICON_16_SCANNER
            } else {
                &gfx::icons::ICON_16_DIAGNOSTICS
            }
        }
        b'T' | b't' => &gfx::icons::ICON_16_FLASHLIGHT,
        b'S' | b's' => &gfx::icons::ICON_16_INFO,
        b'R' | b'r' => &gfx::icons::ICON_16_RADIO,
        _ => &gfx::icons::ICON_16_APPS,
    }
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
        let matches = if is_service {
            entry.is_valid && entry.exec_type != kernel::proc::krx::KrxExecType::App
        } else {
            !entry.is_valid || entry.exec_type == kernel::proc::krx::KrxExecType::App
        };
        if !matches {
            continue;
        }
        let info = if is_service {
            ShortString::from_str(entry.exec_type.as_str())
        } else if entry.is_valid {
            format_size(entry.size_bytes)
        } else {
            ShortString::from_str("Invalid")
        };
        items[count] = MenuItem::new(entry.title.as_str(), MenuItemType::Value(info));
        icons[count] = app_icon_8(entry.title.as_str());
        count += 1;
    }

    if count == 0 && !items.is_empty() {
        items[0] = MenuItem::new(
            if is_service { "No Services" } else { "No Apps" },
            MenuItemType::Value(ShortString::from_str("In /apps/")),
        );
        if !icons.is_empty() {
            icons[0] = &gfx::icons::ICON_APPS;
        }
        return 1;
    }
    count
}

#[inline(never)]
fn add_detail_row(items: &mut [MenuItem], n: &mut usize, title: &'static str, val: &str) {
    if *n < items.len() {
        items[*n] = MenuItem::new(title, MenuItemType::Value(ShortString::from_str(val)));
        *n += 1;
    }
}

#[inline(never)]
pub fn populate_package_detail(items: &mut [MenuItem], is_service: bool, idx: usize) -> usize {
    let cat = get_catalog();
    let pkg = cat.get_entry(is_service, idx);

    let Some(pkg) = pkg else {
        items[0] = MenuItem::new(
            "Status",
            MenuItemType::Value(ShortString::from_str("Empty")),
        );
        return 1;
    };

    let mut n = 0;
    if !is_service && pkg.is_valid {
        items[0] = MenuItem::new("Launch App", MenuItemType::Action);
        n = 1;
    }

    add_detail_row(items, &mut n, "Name", pkg.title.as_str());
    if is_service {
        add_detail_row(items, &mut n, "Role", pkg.exec_type.as_str());
    }
    add_detail_row(items, &mut n, "Class", pkg.app_class.as_str());
    if n < items.len() {
        items[n] = MenuItem::new("Size", MenuItemType::Value(format_size(pkg.size_bytes)));
        n += 1;
    }
    add_detail_row(items, &mut n, "File", pkg.filename());

    if n < items.len() {
        items[n] = MenuItem::new("Delete", MenuItemType::Action);
        n += 1;
    }
    n
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
                if let Err(err) = crate::apps::runtime::launch_krx_package(ctx, path) {
                    ctx.show_popup(super::dialog::TAG_POPUP_INFO, err.title(), err.message());
                }
            } else if (!is_service && pkg.is_valid && sel_idx == 5)
                || ((is_service || !pkg.is_valid) && sel_idx == if is_service { 5 } else { 4 })
            {
                let path = pkg.path.clone();
                let _ = ctx.storage.remove(path.as_str());
                cat.mark_dirty();
                cat.scan(ctx.storage);
                crate::apps::launcher::reload_launcher(ctx.storage);
                ctx.show_popup(
                    super::dialog::TAG_POPUP_INFO,
                    "Package",
                    "Deleted from /apps/",
                );
                app.pop_submenu(ctx);
            }
            true
        }
        _ => false,
    }
}
