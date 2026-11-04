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
    if bytes < 1024 {
        s.push_int_fixed(bytes as i32, 0, "", " B");
    } else {
        s.push_int_fixed(
            ((bytes.saturating_mul(10).saturating_add(512)) >> 10) as i32,
            1,
            "",
            " KB",
        );
    }
    s
}

#[inline(never)]
pub fn populate_package_list(
    items: &mut [MenuItem],
    _ctx: &mut SysContext,
    is_service: bool,
) -> usize {
    #[cfg(feature = "filesystem")]
    get_catalog().scan(_ctx.storage);

    let cat = get_catalog();
    let mut count = 0;
    let max = items.len();

    if is_service {
        for srv in cat.services() {
            if count >= max {
                break;
            }
            items[count] = MenuItem::new(
                srv.title.as_str(),
                MenuItemType::Value(ShortString::from_str(srv.exec_type.as_str())),
            );
            count += 1;
        }
    } else {
        for app in cat.apps() {
            if count >= max {
                break;
            }
            let info = if app.is_valid {
                format_size(app.size_bytes)
            } else {
                ShortString::from_str("Invalid")
            };
            items[count] = MenuItem::new(app.title.as_str(), MenuItemType::Value(info));
            count += 1;
        }
    }

    if count == 0 && !items.is_empty() {
        items[0] = MenuItem::new(
            if is_service { "No Services" } else { "No Apps" },
            MenuItemType::Value(ShortString::from_str("In /apps/")),
        );
        return 1;
    }
    count
}

#[inline(never)]
pub fn populate_package_detail(items: &mut [MenuItem], is_service: bool, idx: usize) -> usize {
    let cat = get_catalog();
    let pkg = if is_service {
        cat.services().nth(idx)
    } else {
        cat.apps().nth(idx)
    };

    let Some(pkg) = pkg else {
        items[0] = MenuItem::new(
            "Status",
            MenuItemType::Value(ShortString::from_str("Empty")),
        );
        return 1;
    };

    let mut n = 0;
    if !is_service && pkg.is_valid {
        items[n] = MenuItem::new("Launch App", MenuItemType::Action);
        n += 1;
    }

    if n < items.len() {
        items[n] = MenuItem::new(
            "Name",
            MenuItemType::Value(ShortString::from_str(pkg.title.as_str())),
        );
        n += 1;
    }
    if is_service && n < items.len() {
        items[n] = MenuItem::new(
            "Role",
            MenuItemType::Value(ShortString::from_str(pkg.exec_type.as_str())),
        );
        n += 1;
    }
    if n < items.len() {
        items[n] = MenuItem::new(
            "Class",
            MenuItemType::Value(ShortString::from_str(pkg.app_class.as_str())),
        );
        n += 1;
    }
    if n < items.len() {
        items[n] = MenuItem::new("Size", MenuItemType::Value(format_size(pkg.size_bytes)));
        n += 1;
    }
    if n < items.len() {
        items[n] = MenuItem::new(
            "File",
            MenuItemType::Value(ShortString::from_str(pkg.filename())),
        );
        n += 1;
    }
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
        PageId::ManageApps => {
            let cat = get_catalog();
            if sel_idx < cat.app_count() {
                app.selected_pkg_idx = sel_idx;
                app.push_submenu(PageId::AppDetail, ctx);
            }
            true
        }
        PageId::ManageServices => {
            let cat = get_catalog();
            if sel_idx < cat.service_count() {
                app.selected_pkg_idx = sel_idx;
                app.push_submenu(PageId::ServiceDetail, ctx);
            }
            true
        }
        PageId::AppDetail | PageId::ServiceDetail => {
            let is_service = app.current_page == PageId::ServiceDetail;
            let cat = get_catalog();
            let pkg = if is_service {
                cat.services().nth(app.selected_pkg_idx)
            } else {
                cat.apps().nth(app.selected_pkg_idx)
            };

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
