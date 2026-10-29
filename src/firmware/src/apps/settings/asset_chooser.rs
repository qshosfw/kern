//! Generalized Dynamic File & Asset Chooser for Kern OS
//!
//! Provides zero-alloc, stack-safe directory asset browsing across LittleFS,
//! dynamic name extraction (e.g. "01_roger.ton" -> "Roger"), and OptionSheet generation.
//! Completely decoupled from specific asset domains: takes directory paths and extensions dynamically.

use crate::SysContext;
use gfx::gui::modules::menu::{OptionSheet, ShortString};

pub const MAX_CHOOSER_ASSETS: usize = 8;
pub const TAG_ASSET_CHOOSER: u8 = 16;

pub struct AssetChooserParams {
    pub title: &'static str,
    pub dir: &'static str,
    pub cur_path: heapless::String<32>,
    pub is_active: bool,
    pub none_label: Option<&'static str>,
    pub fallback: Option<(&'static str, &'static str)>,
    pub extensions: &'static [&'static str],
}

struct AssetChooserStorage {
    paths: [heapless::String<32>; MAX_CHOOSER_ASSETS],
    labels: [ShortString; MAX_CHOOSER_ASSETS + 1],
    has_none: bool,
    count: usize,
}

#[inline(always)]
fn storage_ref() -> &'static AssetChooserStorage {
    let arena = crate::runtime::native_runner::transient_arena_bytes_mut();
    unsafe { &*(arena[5120..].as_ptr() as *const AssetChooserStorage) }
}

#[inline(always)]
fn storage_mut() -> &'static mut AssetChooserStorage {
    let arena = crate::runtime::native_runner::transient_arena_bytes_mut();
    unsafe { &mut *(arena[5120..].as_mut_ptr() as *mut AssetChooserStorage) }
}

/// Universal dynamic display name formatter for filesystem assets.
/// Converts e.g. "/user/rogers/01_roger.ton" -> "Roger", "02_mini_kong.rle" -> "Mini Kong"
pub fn format_asset_name(path: &str) -> ShortString {
    let name_start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
    let file_part = &path[name_start..];
    let base = if let Some(dot_idx) = gfx::text::rfind_byte(file_part, b'.') {
        &file_part[..dot_idx]
    } else {
        file_part
    };

    // Strip leading numerical index prefix if present (e.g. "01_", "02-", "1_")
    let bytes = base.as_bytes();
    let mut start = 0;
    while start < bytes.len() && bytes[start].is_ascii_digit() {
        start += 1;
    }
    if start > 0 && start < bytes.len() && (bytes[start] == b'_' || bytes[start] == b'-') {
        start += 1;
    } else {
        start = 0;
    }
    let clean = &bytes[start..];
    if clean.is_empty() {
        return ShortString::from_str("Asset");
    }

    let mut out = ShortString::empty();
    let mut capitalize_next = true;
    for &b in clean {
        if (out.len as usize) >= out.bytes.len() {
            break;
        }
        let ch = if b == b'_' || b == b'-' {
            capitalize_next = true;
            b' '
        } else if capitalize_next {
            capitalize_next = false;
            b.to_ascii_uppercase()
        } else {
            b
        };
        out.bytes[out.len as usize] = ch;
        out.len += 1;
    }
    out
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
    let storage = storage_mut();

    storage.count = 0;
    storage.has_none = params.none_label.is_some();

    let mut label_offset = 0;
    if let Some(none_str) = params.none_label {
        storage.labels[0] = ShortString::from_str(none_str);
        label_offset = 1;
    }

    let mut dir_entries =
        [const { crate::services::storage::DirEntryInfo::empty() }; MAX_CHOOSER_ASSETS];
    let scan_count = ctx
        .storage
        .list_dir(params.dir, &mut dir_entries)
        .unwrap_or(0);

    for i in 1..scan_count {
        let mut j = i;
        while j > 0 && dir_entries[j].path.as_str() < dir_entries[j - 1].path.as_str() {
            dir_entries.swap(j, j - 1);
            j -= 1;
        }
    }

    let mut current_sel = 0;

    for entry in &dir_entries[..scan_count] {
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
        storage.labels[found + label_offset] = format_asset_name(path);

        if params.is_active && current_sel == 0 && path == params.cur_path.as_str() {
            current_sel = found + label_offset;
        }

        storage.count += 1;
        if storage.count >= MAX_CHOOSER_ASSETS {
            break;
        }
    }

    if storage.count == 0 {
        if let Some((fb_path, fb_label)) = params.fallback {
            storage.paths[0].clear();
            let _ = storage.paths[0].push_str(fb_path);
            storage.labels[label_offset] = ShortString::from_str(fb_label);
            storage.count = 1;
            if params.is_active {
                current_sel = label_offset;
            }
        }
    }

    if params.is_active && current_sel == 0 && storage.count > 0 && params.none_label.is_some() {
        current_sel = 1;
    }

    let total_options = storage.count + label_offset;
    let labels_slice: &'static [ShortString] =
        unsafe { core::slice::from_raw_parts(storage.labels.as_ptr(), total_options) };

    OptionSheet::new_dynamic(params.title, labels_slice, current_sel).with_radio(true)
}

/// Retrieve the selected asset path from storage
pub fn get_chosen_asset_path(sel: usize) -> Option<&'static str> {
    let storage = storage_ref();
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
