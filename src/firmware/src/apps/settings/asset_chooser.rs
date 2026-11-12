//! Generalized Dynamic File & Asset Chooser for Kern OS
//!
//! Provides zero-alloc, stack-safe directory asset browsing across LittleFS,
//! dynamic name extraction (e.g. "01_roger.ton" -> "Roger"), and OptionSheet generation.
//! Completely decoupled from specific asset domains: takes directory paths and extensions dynamically.

use gfx::gui::modules::menu::{OptionSheet, ShortString};

pub const MAX_CHOOSER_ASSETS: usize = 8;
pub const TAG_ASSET_CHOOSER: u8 = 16;

#[derive(Clone, Copy)]
pub enum AssetKind {
    Splash,
    Roger,
    Jingle,
}

pub struct AssetDescriptor {
    pub title: &'static str,
    pub dir: &'static str,
    pub extensions: &'static [&'static str],
    pub none_label: &'static str,
    pub kind: AssetKind,
}

impl AssetDescriptor {
    fn get_path<'a>(&self, cfg: &'a crate::config::MasterConfig) -> (bool, &'a str) {
        match self.kind {
            AssetKind::Splash => (
                cfg.display.boot_splash_mode != 0,
                cfg.display.splash_path_str(),
            ),
            AssetKind::Roger => (cfg.audio.roger_beep != 0, cfg.audio.roger_path_str()),
            AssetKind::Jingle => (cfg.audio.boot_jingle != 0, cfg.audio.jingle_path_str()),
        }
    }

    fn set_path(&self, cfg: &mut crate::config::MasterConfig, opt: Option<&str>) {
        match self.kind {
            AssetKind::Splash => match opt {
                Some(path) => {
                    cfg.display.boot_splash_mode = 1;
                    cfg.display.set_splash_path(path);
                }
                None => cfg.display.boot_splash_mode = 0,
            },
            AssetKind::Roger => match opt {
                Some(path) => {
                    cfg.audio.roger_beep = 1;
                    cfg.audio.set_roger_path(path);
                }
                None => cfg.audio.roger_beep = 0,
            },
            AssetKind::Jingle => match opt {
                Some(path) => {
                    cfg.audio.boot_jingle = 1;
                    cfg.audio.set_jingle_path(path);
                }
                None => cfg.audio.boot_jingle = 0,
            },
        }
    }

    #[inline(never)]
    pub fn build_sheet(
        &self,
        config: &crate::config::MasterConfig,
        storage_fs: &mut crate::services::SystemStorage,
    ) -> OptionSheet {
        let (active, cur_path) = self.get_path(config);
        let storage = storage_mut();

        storage.count = 0;
        storage.labels[0] = ShortString::from_str(self.none_label);

        let scan_count = storage_fs
            .list_dir(self.dir, &mut storage.dir_entries)
            .unwrap_or(0);

        for i in 1..scan_count {
            let mut j = i;
            while j > 0
                && storage.dir_entries[j].path.as_str() < storage.dir_entries[j - 1].path.as_str()
            {
                storage.dir_entries.swap(j, j - 1);
                j -= 1;
            }
        }

        let mut current_sel = 0;

        for entry in &storage.dir_entries[..scan_count] {
            if entry.is_dir {
                continue;
            }
            let path = entry.path.as_str();
            if !has_extension(path, self.extensions) {
                continue;
            }

            let found = storage.count;
            storage.paths[found].clear();
            let _ = storage.paths[found].push_str(path);
            format_asset_name_into(path, &mut storage.labels[found + 1]);

            if active && current_sel == 0 && path == cur_path {
                current_sel = found + 1;
            }

            storage.count += 1;
            if storage.count >= MAX_CHOOSER_ASSETS {
                break;
            }
        }

        if active && current_sel == 0 && storage.count > 0 {
            current_sel = 1;
        }

        let total_options = storage.count + 1;
        let labels_slice: &'static [ShortString] =
            unsafe { core::slice::from_raw_parts(storage.labels.as_ptr(), total_options) };

        OptionSheet::new_dynamic(self.title, labels_slice, current_sel).with_radio(true)
    }

    pub fn apply_choice(&self, sel: usize, config: &mut crate::config::MasterConfig) -> bool {
        let is_active = self.get_path(config).0;
        if sel == 0 {
            if is_active {
                self.set_path(config, None);
                return true;
            }
            return false;
        }
        if let Some(path) = get_chosen_asset_path(sel) {
            self.set_path(config, Some(path));
            true
        } else {
            false
        }
    }

    pub fn format_status(&self, config: &crate::config::MasterConfig, out: &mut ShortString) {
        let (active, path) = self.get_path(config);
        if !active {
            out.set_str(self.none_label);
        } else {
            format_asset_name_into(path, out);
        }
    }
}

struct AssetChooserStorage {
    dir_entries: [crate::services::storage::DirEntryInfo; MAX_CHOOSER_ASSETS],
    paths: [heapless::String<32>; MAX_CHOOSER_ASSETS],
    labels: [ShortString; MAX_CHOOSER_ASSETS + 1],
    count: usize,
}

#[inline(always)]
fn storage_ref() -> &'static AssetChooserStorage {
    let arena = crate::data::arena::transient_arena_bytes_mut();
    unsafe {
        &*(arena[crate::data::arena::TRANSIENT_SCRATCH_OFFSET..].as_ptr()
            as *const AssetChooserStorage)
    }
}

#[inline(always)]
fn storage_mut() -> &'static mut AssetChooserStorage {
    let arena = crate::data::arena::transient_arena_bytes_mut();
    unsafe {
        &mut *(arena[crate::data::arena::TRANSIENT_SCRATCH_OFFSET..].as_mut_ptr()
            as *mut AssetChooserStorage)
    }
}

/// Universal dynamic display name formatter for filesystem assets.
/// Converts e.g. "/user/rogers/01_roger.ton" -> "Roger", "02_mini_kong.rle" -> "Mini Kong"
pub fn format_asset_name_into(path: &str, out: &mut ShortString) {
    let raw = path.as_bytes();
    let name_start = raw.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
    let file_part = &raw[name_start..];
    let dot_idx = file_part
        .iter()
        .rposition(|&b| b == b'.')
        .unwrap_or(file_part.len());
    let base = &file_part[..dot_idx];

    // Strip leading numerical index prefix if present (e.g. "01_", "02-", "1_")
    let mut start = 0;
    while start < base.len() && base[start].is_ascii_digit() {
        start += 1;
    }
    if start > 0 && start < base.len() && (base[start] == b'_' || base[start] == b'-') {
        start += 1;
    } else {
        start = 0;
    }
    let clean = &base[start..];
    if clean.is_empty() {
        out.set_str("Asset");
        return;
    }

    out.len = 0;
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

/// Retrieve the selected asset path from storage
pub fn get_chosen_asset_path(sel: usize) -> Option<&'static str> {
    if sel == 0 {
        return None;
    }
    let storage = storage_ref();
    let idx = sel - 1;
    if idx < storage.count && !storage.paths[idx].is_empty() {
        Some(storage.paths[idx].as_str())
    } else {
        None
    }
}
