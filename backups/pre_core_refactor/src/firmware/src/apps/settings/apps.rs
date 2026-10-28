//! Applications & Launcher Domain Settings Declarations

use super::options::{
    CHOICE_LAUNCHER_STYLE, CHOICE_STARTUP_APP, PAGE_MANAGE_APPS, PAGE_MANAGE_SERVICES, SettingDef,
    SettingField, SettingKind,
};
use crate::SysContext;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub static LAUNCHER_STYLE_OPTIONS: [&'static str; 2] = ["Grid", "List"];
pub static STARTUP_APP_OPTIONS: [&'static str; 2] = ["Launcher", "Radio"];
pub static VFO_DUAL_WATCH_OPTIONS: [&'static str; 3] = ["Off", "Dual", "Full"];

pub static APPS_DEFS: [SettingDef; 6] = [
    SettingDef {
        title: "Installed",
        kind: SettingKind::Submenu(PAGE_MANAGE_APPS),
        field: SettingField::None,
    },
    SettingDef {
        title: "Services",
        kind: SettingKind::Submenu(PAGE_MANAGE_SERVICES),
        field: SettingField::None,
    },
    SettingDef {
        title: "Style",
        kind: SettingKind::Choice(CHOICE_LAUNCHER_STYLE),
        field: SettingField::LAUNCHER_STYLE,
    },
    SettingDef {
        title: "Key Badges",
        kind: SettingKind::Toggle,
        field: SettingField::SHOW_NUMBERS,
    },
    SettingDef {
        title: "App Titles",
        kind: SettingKind::Toggle,
        field: SettingField::SHOW_LABELS,
    },
    SettingDef {
        title: "Startup App",
        kind: SettingKind::Choice(CHOICE_STARTUP_APP),
        field: SettingField::STARTUP_APP,
    },
];

use crate::apps::launcher::catalog::get_catalog;

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

/// Absolute VFS path of a staged bundle (`/apps/<file>`).
fn app_path(filename: &str) -> ShortString {
    let mut s = ShortString::from_str("/apps/");
    s.push_str(filename);
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
                crate::data::app_bundle::ExecType::Service => "[SRV] ",
                crate::data::app_bundle::ExecType::Daemon => "[DMN] ",
                crate::data::app_bundle::ExecType::Driver => "[DRV] ",
                crate::data::app_bundle::ExecType::Module => "[MOD] ",
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
            items[i] = MenuItem::new(
                app.title.as_str(),
                MenuItemType::Value(format_size(app.size_bytes)),
            );
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
    push_val(items, &mut cur, "Path", app_path(pkg.filename.as_str()));
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
