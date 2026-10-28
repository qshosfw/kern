//! Generalized Dynamic Asset & File Chooser for Kern OS
//!
//! Provides zero-alloc, stack-safe, zero-I/O directory asset browsing
//! with custom extensions, display name formatting, and instant OptionSheet generation.
//! Designed for use across core Settings (Roger beeps, Boot Splash) and installable KRX apps.

use crate::SysContext;
use gfx::gui::modules::menu::{OptionSheet, ShortString};

pub const MAX_CHOOSER_ASSETS: usize = 8;

pub struct AssetChooserParams {
    pub title: &'static str,
    pub dir: &'static str,
    pub cur_path: heapless::String<32>,
    pub is_active: bool,
    pub none_label: Option<&'static str>,
    pub extensions: &'static [&'static str],
    pub name_parser: Option<fn(&str) -> ShortString>,
}

struct AssetChooserStorage {
    entries: [crate::services::storage::DirEntryInfo; MAX_CHOOSER_ASSETS],
    paths: [heapless::String<32>; MAX_CHOOSER_ASSETS],
    labels: [ShortString; MAX_CHOOSER_ASSETS + 1],
    has_none: bool,
    count: usize,
}

static mut CHOOSER_STORAGE: AssetChooserStorage = AssetChooserStorage {
    entries: [const { crate::services::storage::DirEntryInfo::empty() }; MAX_CHOOSER_ASSETS],
    paths: [const { heapless::String::new() }; MAX_CHOOSER_ASSETS],
    labels: [const { ShortString::empty() }; MAX_CHOOSER_ASSETS + 1],
    has_none: false,
    count: 0,
};

/// Default name parser: extracts basename and strips leading numerical tags (e.g. "01_")
pub fn default_name_parser(path: &str) -> ShortString {
    let name_start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
    let file_part = &path[name_start..];
    let base = if let Some(dot_idx) = gfx::text::rfind_byte(file_part, b'.') {
        &file_part[..dot_idx]
    } else {
        file_part
    };
    let clean = if base.len() > 3
        && base.as_bytes()[0].is_ascii_digit()
        && base.as_bytes()[1].is_ascii_digit()
        && base.as_bytes()[2] == b'_'
    {
        &base[3..]
    } else {
        base
    };
    ShortString::from_str(clean)
}

fn has_extension(path: &str, extensions: &[&str]) -> bool {
    if extensions.is_empty() {
        return true;
    }
    let b = path.as_bytes();
    for &ext in extensions {
        let eb = ext.as_bytes();
        if b.len() >= eb.len() && &b[b.len() - eb.len()..] == eb {
            return true;
        }
    }
    false
}

/// Generalized asset chooser builder
#[inline(never)]
pub fn build_asset_chooser(params: &AssetChooserParams, ctx: &mut SysContext) -> OptionSheet {
    let storage = unsafe { &mut *core::ptr::addr_of_mut!(CHOOSER_STORAGE) };

    storage.count = 0;
    storage.has_none = params.none_label.is_some();

    let mut label_offset = 0;
    if let Some(none_str) = params.none_label {
        storage.labels[0] = ShortString::from_str(none_str);
        label_offset = 1;
    }

    let scan_count = ctx
        .storage
        .list_dir(params.dir, &mut storage.entries)
        .unwrap_or(0);

    let mut current_sel = 0;
    let name_fn = params.name_parser.unwrap_or(default_name_parser);

    for entry in &storage.entries[..scan_count] {
        if entry.is_dir {
            continue;
        }
        let path = entry.path.as_str();
        if !has_extension(path, params.extensions) {
            continue;
        }

        let found = storage.count;
        storage.paths[found].clear();
        let _ = storage.paths[found].push_str(path);
        storage.labels[found + label_offset] = name_fn(path);

        if params.is_active && current_sel == 0 && path == params.cur_path.as_str() {
            current_sel = found + label_offset;
        }

        storage.count += 1;
        if storage.count >= MAX_CHOOSER_ASSETS {
            break;
        }
    }

    if params.is_active && current_sel == 0 && storage.count > 0 && params.none_label.is_some() {
        current_sel = 1;
    }

    let total_options = storage.count + label_offset;
    let labels_slice: &'static [ShortString] = unsafe {
        core::slice::from_raw_parts(storage.labels.as_ptr(), total_options)
    };

    OptionSheet::new_dynamic(params.title, labels_slice, current_sel).with_radio(true)
}

/// Retrieve the selected asset path from static storage
pub fn get_chosen_asset_path(sel: usize) -> Option<&'static str> {
    unsafe {
        let storage = &*core::ptr::addr_of!(CHOOSER_STORAGE);
        let idx = if storage.has_none {
            if sel == 0 {
                return None;
            }
            sel - 1
        } else {
            sel
        };
        if idx < storage.count && !storage.paths[idx].is_empty() {
            Some(storage.paths[idx].as_str())
        } else {
            None
        }
    }
}

/// Built-in Boot Splash chooser preset
#[inline(never)]
pub fn build_splash_chooser(ctx: &mut SysContext) -> OptionSheet {
    let mut cur = heapless::String::<32>::new();
    let _ = cur.push_str(ctx.config.display.splash_path_str());
    let active = ctx.config.display.boot_splash_mode != 0;
    let params = AssetChooserParams {
        title: "Boot Image",
        dir: "/user/splash",
        cur_path: cur,
        is_active: active,
        none_label: Some("None"),
        extensions: &[".rle", ".img", ".bin"],
        name_parser: Some(crate::boot::splash::get_splash_name),
    };
    build_asset_chooser(&params, ctx)
}

/// Built-in Roger Beep chooser preset
#[inline(never)]
pub fn build_roger_chooser(ctx: &mut SysContext) -> OptionSheet {
    let mut cur = heapless::String::<32>::new();
    let _ = cur.push_str(ctx.config.audio.roger_path_str());
    let active = ctx.config.audio.roger_beep != 0;
    let params = AssetChooserParams {
        title: "Roger Beep",
        dir: "/user/rogers",
        cur_path: cur,
        is_active: active,
        none_label: Some("Off"),
        extensions: &[".ton", ".rgr"],
        name_parser: Some(crate::services::melody::get_roger_name),
    };
    build_asset_chooser(&params, ctx)
}
