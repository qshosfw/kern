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

pub fn app_icons(title: &str) -> (&'static [u8; 8], &'static [u8; 32]) {
    static MAP: [(u8, &'static [u8; 8], &'static [u8; 32]); 5] = [
        (
            b'T',
            gfx::icons::ICON_APP_TORCH,
            &gfx::icons::ICON_16_FLASHLIGHT,
        ),
        (b'S', gfx::icons::ICON_APP_ABOUT, &gfx::icons::ICON_16_INFO),
        (b'R', gfx::icons::ICON_APP_RADIO, &gfx::icons::ICON_16_RADIO),
        (b'C', &gfx::icons::ICON_KEYPAD, &gfx::icons::ICON_16_APPS),
        (
            b'B',
            gfx::icons::ICON_APP_DIAG,
            &gfx::icons::ICON_16_DIAGNOSTICS,
        ),
    ];
    let b = title.as_bytes();
    let c0 = b.first().copied().unwrap_or(0) & !0x20;
    if c0 == b'B' && (b.get(2).copied().unwrap_or(0) & !0x20) == b'A' {
        return (&gfx::icons::ICON_ANTENNA, &gfx::icons::ICON_16_SCANNER);
    }
    for &(k, i8, i16) in &MAP {
        if c0 == k {
            return (i8, i16);
        }
    }
    (gfx::icons::ICON_APPS, &gfx::icons::ICON_16_APPS)
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
        icons[count] = app_icons(entry.title.as_str()).0;
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
