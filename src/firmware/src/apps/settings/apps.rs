//! Applications & Launcher Domain Settings Declarations

use super::page::{PageId, SettingDef, SettingField};
use crate::SysContext;
use crate::apps::launcher::catalog::get_catalog;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub static LAUNCHER_STYLE_OPTIONS: [&'static str; 2] = ["Grid", "List"];
pub static STARTUP_APP_OPTIONS: [&'static str; 2] = ["Home", "Radio"];
pub static VFO_DUAL_WATCH_OPTIONS: [&'static str; 3] = ["Off", "Dual", "Full"];

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

/// `"512 B"` below 1 KB, otherwise one-decimal KB (`"12.4 KB"`).
/// Tenths are computed as `(bytes * 10 + 512) >> 10`: shift-only, no divider, no `core::fmt`.
fn format_size(size_bytes: usize) -> ShortString {
    let bytes = size_bytes as u32;
    let mut s = ShortString::empty();
    if bytes < 1024 {
        s.push_int_fixed(bytes as i32, 0, "", " B");
    } else {
        let tenths = (bytes.saturating_mul(10).saturating_add(512)) >> 10;
        s.push_int_fixed(tenths as i32, 1, "", " KB");
    }
    s
}

fn populate_catalog_list(items: &mut [MenuItem], ctx: &mut SysContext, is_service: bool) -> usize {
    #[cfg(feature = "filesystem")]
    get_catalog().scan(ctx.storage);

    let cat = get_catalog();
    let mut count = 0;

    if is_service {
        for (i, srv) in cat.services().enumerate() {
            if i >= items.len() {
                break;
            }
            let tag = match srv.exec_type {
                kernel::proc::krx::KrxExecType::Service => "[SRV] ",
                kernel::proc::krx::KrxExecType::Daemon => "[DMN] ",
                kernel::proc::krx::KrxExecType::Driver => "[DRV] ",
                kernel::proc::krx::KrxExecType::Module => "[MOD] ",
                _ => "[PKG] ",
            };
            let mut val = ShortString::from_str(tag);
            val.push_str(srv.app_class.as_str());
            items[i] = MenuItem::new(srv.title.as_str(), MenuItemType::Value(val));
            count += 1;
        }
    } else {
        for (i, app) in cat.apps().enumerate() {
            if i >= items.len() {
                break;
            }
            let right_val = if app.is_valid {
                format_size(app.size_bytes)
            } else {
                ShortString::from_str("INVALID")
            };
            items[i] = MenuItem::new(app.title.as_str(), MenuItemType::Value(right_val));
            count += 1;
        }
    }

    if count == 0 && !items.is_empty() {
        items[0] = MenuItem::new(
            if is_service {
                "No Services"
            } else {
                "No KRX Apps"
            },
            MenuItemType::Value(ShortString::from_str("Staging /apps/")),
        );
        return 1;
    }

    count
}

#[inline(never)]
pub fn populate_manage_apps(items: &mut [MenuItem], ctx: &mut SysContext) -> usize {
    populate_catalog_list(items, ctx, false)
}

#[inline(never)]
pub fn populate_manage_services(items: &mut [MenuItem], ctx: &mut SysContext) -> usize {
    populate_catalog_list(items, ctx, true)
}

#[inline(never)]
fn push_val(items: &mut [MenuItem], cur: &mut usize, title: &'static str, val: ShortString) {
    if *cur < items.len() {
        items[*cur] = MenuItem::new(title, MenuItemType::Value(val));
        *cur += 1;
    }
}

fn push_str(items: &mut [MenuItem], cur: &mut usize, title: &'static str, val: &str) {
    push_val(items, cur, title, ShortString::from_str(val));
}

#[inline(never)]
fn push_action(items: &mut [MenuItem], cur: &mut usize, title: &'static str) {
    if *cur < items.len() {
        items[*cur] = MenuItem::new(title, MenuItemType::Action);
        *cur += 1;
    }
}

fn populate_package_detail(items: &mut [MenuItem], is_service: bool, idx: usize) -> usize {
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

    let mut cur = 0;
    if !pkg.is_valid {
        push_action(items, &mut cur, "Delete Invalid");
        push_str(items, &mut cur, "Status", "Invalid / Corrupt");
        push_val(items, &mut cur, "Size", format_size(pkg.size_bytes));
        push_str(items, &mut cur, "Path", pkg.path.as_str());
        return cur;
    }

    if !is_service {
        push_action(items, &mut cur, "Launch App");
    }
    push_str(
        items,
        &mut cur,
        if is_service { "Service" } else { "Name" },
        pkg.title.as_str(),
    );
    if is_service {
        push_str(items, &mut cur, "Role", pkg.exec_type.as_str());
    }
    push_str(items, &mut cur, "Class", pkg.app_class.as_str());
    push_val(items, &mut cur, "Size", format_size(pkg.size_bytes));
    push_str(items, &mut cur, "Path", pkg.path.as_str());
    if !is_service {
        push_action(items, &mut cur, "Delete App");
    }
    cur
}

#[inline(never)]
pub fn populate_app_detail(items: &mut [MenuItem], app_idx: usize, _ctx: &mut SysContext) -> usize {
    populate_package_detail(items, false, app_idx)
}

#[inline(never)]
pub fn populate_service_detail(
    items: &mut [MenuItem],
    srv_idx: usize,
    _ctx: &mut SysContext,
) -> usize {
    populate_package_detail(items, true, srv_idx)
}

#[inline(never)]
pub fn handle_apps_press(
    app: &mut super::app::SettingsApp,
    ctx: &mut SysContext,
    sel_idx: usize,
) -> bool {
    match app.current_page {
        PageId::ManageApps => {
            let cat = get_catalog();
            let count = cat.app_count();
            if count > 0 && sel_idx < count {
                app.selected_app_idx = sel_idx;
                let is_valid = cat.apps().nth(sel_idx).map_or(false, |a| a.is_valid);
                if !is_valid {
                    ctx.show_confirm(
                        super::dialog::TAG_APP_DELETE_CONFIRM,
                        "Invalid App",
                        "Invalid or corrupted package.\nDelete from /apps/?",
                        "Delete",
                        "Cancel",
                    );
                } else {
                    app.push_submenu(PageId::AppDetail, ctx);
                }
            } else {
                ctx.show_popup(
                    super::dialog::TAG_POPUP_INFO,
                    "Apps Storage",
                    "Stage .krx files into /apps/",
                );
            }
            true
        }
        PageId::ManageServices => {
            let count = get_catalog().service_count();
            if count > 0 && sel_idx < count {
                app.selected_service_idx = sel_idx;
                app.push_submenu(PageId::ServiceDetail, ctx);
            } else {
                ctx.show_popup(
                    super::dialog::TAG_POPUP_INFO,
                    "KRX Services",
                    "No background services in /apps/",
                );
            }
            true
        }
        PageId::AppDetail => {
            let cat = get_catalog();
            if let Some(pkg) = cat.apps().nth(app.selected_app_idx) {
                if !pkg.is_valid {
                    if sel_idx == 0 {
                        ctx.show_confirm(
                            super::dialog::TAG_APP_DELETE_CONFIRM,
                            "Delete Invalid",
                            "Delete invalid package from /apps/?",
                            "Delete",
                            "Cancel",
                        );
                    }
                    return true;
                }
                if sel_idx == 0 {
                    let path = pkg.path.as_str();
                    if let Err(err) = crate::apps::runtime::launch_krx_package(ctx, path) {
                        ctx.show_confirm(
                            super::dialog::TAG_POPUP_INFO,
                            err.title(),
                            err.message(),
                            "OK",
                            "EXIT",
                        );
                        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
                    }
                } else if sel_idx == 5 {
                    ctx.show_confirm(
                        super::dialog::TAG_APP_DELETE_CONFIRM,
                        "Delete App",
                        "Uninstall and delete application binary?",
                        "Delete",
                        "Cancel",
                    );
                }
            }
            true
        }
        PageId::ServiceDetail => {
            ctx.show_popup(
                super::dialog::TAG_POPUP_INFO,
                "Service Info",
                "KRX background package in /apps/",
            );
            true
        }
        _ => false,
    }
}
