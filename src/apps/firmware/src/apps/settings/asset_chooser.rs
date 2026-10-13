//! Shared Dynamic Asset Chooser for Settings (Roger Beep & Boot Splash)
//!
//! Unifies scanning, static path/label storage, and OptionSheet generation,
//! saving ~1.1 KB of static RAM and ~550 bytes of flash.

use crate::SysContext;
use gfx::gui::modules::menu::{OptionSheet, ShortString};

pub static mut ASSET_PATHS: [heapless::String<32>; 12] = [const { heapless::String::new() }; 12];
pub static mut ASSET_LABELS: [ShortString; 16] = [const { ShortString::empty() }; 16];

pub fn build_asset_chooser(
    title: &'static str,
    dir: &str,
    ext_match: fn(&str) -> bool,
    get_label: fn(&mut SysContext, &str) -> ShortString,
    is_active: bool,
    cur_path: &str,
    ctx: &mut SysContext,
) -> OptionSheet {
    let (paths, labels) = unsafe {
        (
            &mut *core::ptr::addr_of_mut!(ASSET_PATHS),
            &mut *core::ptr::addr_of_mut!(ASSET_LABELS),
        )
    };

    let mut entries = [const { crate::services::storage::DirEntryInfo::empty() }; 12];
    let count = ctx.storage.list_dir(dir, &mut entries).unwrap_or(0);
    let mut found = 0;
    let mut current_sel = 0;

    labels[0] = ShortString::from_str(if title.starts_with("Boot") {
        "None"
    } else {
        "Off"
    });

    for entry in &entries[..count] {
        if entry.is_dir {
            continue;
        }
        let path = entry.path.as_str();
        if !ext_match(path) {
            continue;
        }

        paths[found].clear();
        let _ = paths[found].push_str(path);

        labels[found + 1] = get_label(ctx, path);

        if is_active && current_sel == 0 && path == cur_path {
            current_sel = found + 1;
        }

        found += 1;
        if found >= 12 {
            break;
        }
    }

    if is_active && current_sel == 0 && found > 0 {
        current_sel = 1;
    }

    OptionSheet::new_dynamic(title, unsafe { &ASSET_LABELS[..found + 1] }, current_sel)
        .with_radio(true)
}

pub fn get_chosen_asset_path(sel: usize) -> Option<&'static str> {
    if sel == 0 {
        return None;
    }
    let paths = unsafe { &*core::ptr::addr_of!(ASSET_PATHS) };
    if sel - 1 < paths.len() && !paths[sel - 1].is_empty() {
        Some(paths[sel - 1].as_str())
    } else {
        None
    }
}
