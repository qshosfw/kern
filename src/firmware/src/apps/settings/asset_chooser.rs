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
        storage.labels[0].set_str(self.none_label);

        let scan_count = storage_fs
            .list_dir(self.dir, &mut storage.dir_entries)
            .unwrap_or(0)
            .min(MAX_CHOOSER_ASSETS);

        let mut order = [0u8; MAX_CHOOSER_ASSETS];
        for i in 0..scan_count {
            order[i] = i as u8;
            let mut j = i;
            while j > 0 {
                let cur = (order[j] as usize) & (MAX_CHOOSER_ASSETS - 1);
                let prev = (order[j - 1] as usize) & (MAX_CHOOSER_ASSETS - 1);
                if storage.dir_entries[cur].path.as_str() >= storage.dir_entries[prev].path.as_str()
                {
                    break;
                }
                order.swap(j, j - 1);
                j -= 1;
            }
        }

        let mut current_sel = 0;

        for &idx in &order[..scan_count] {
            let found = storage.count;
            if found >= MAX_CHOOSER_ASSETS {
                break;
            }
            let entry = &storage.dir_entries[(idx as usize) & (MAX_CHOOSER_ASSETS - 1)];
            if entry.is_dir {
                continue;
            }
            let entry_name = entry.path.as_str();
            if !has_extension(entry_name, self.extensions) {
                continue;
            }

            if build_full_asset_path(self.dir, entry_name, &mut storage.paths[found]).is_err() {
                continue;
            }
            let full_path = storage.paths[found].as_str();
            format_asset_name_into(entry_name, &mut storage.labels[found + 1]);

            if active
                && current_sel == 0
                && (full_path == cur_path || (!cur_path.starts_with("/") && entry_name == cur_path))
            {
                current_sel = found + 1;
            }

            storage.count = found + 1;
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

const _: () = {
    assert!(
        core::mem::size_of::<AssetChooserStorage>()
            <= crate::data::arena::TRANSIENT_ARENA_BYTES
                - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
    );
    assert!(
        crate::data::arena::TRANSIENT_SCRATCH_OFFSET % core::mem::align_of::<AssetChooserStorage>()
            == 0
    );
    assert!(
        core::mem::align_of::<AssetChooserStorage>()
            <= core::mem::align_of::<crate::data::arena::AlignedArena>()
    );
};

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
    let mut name_start = 0usize;
    let mut dot_idx = raw.len();
    let mut i = raw.len();
    while i > 0 {
        i -= 1;
        if raw[i] == b'/' {
            name_start = i + 1;
            break;
        }
        if raw[i] == b'.' && dot_idx == raw.len() {
            dot_idx = i;
        }
    }
    let base = &raw[name_start..dot_idx.max(name_start)];

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

fn build_full_asset_path(
    dir: &str,
    entry_name: &str,
    out: &mut heapless::String<32>,
) -> Result<(), ()> {
    out.clear();
    if entry_name.starts_with("/") {
        if entry_name.len() > 31 {
            return Err(());
        }
        return out.push_str(entry_name);
    }
    let clean_dir = utils::format::trim_trailing_slashes(dir);
    let dir_prefix = if clean_dir.is_empty() {
        0
    } else {
        clean_dir.len()
    };
    if dir_prefix + 1 + entry_name.len() > 31 {
        return Err(());
    }
    if !clean_dir.is_empty() {
        out.push_str(clean_dir)?;
    }
    out.push('/')?;
    out.push_str(entry_name)
}

/// Retrieve the selected asset path from storage
pub fn get_chosen_asset_path(sel: usize) -> Option<&'static str> {
    if sel == 0 {
        return None;
    }
    let storage = storage_ref();
    let idx = sel - 1;
    if idx < storage.count.min(MAX_CHOOSER_ASSETS) && !storage.paths[idx].is_empty() {
        Some(storage.paths[idx].as_str())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_full_asset_path() {
        let mut out = heapless::String::<32>::new();
        assert!(build_full_asset_path("/user/splash", "01_quansheng.rle", &mut out).is_ok());
        assert_eq!(out.as_str(), "/user/splash/01_quansheng.rle");

        assert!(build_full_asset_path("/user/rogers/", "03_mototrbo.ton", &mut out).is_ok());
        assert_eq!(out.as_str(), "/user/rogers/03_mototrbo.ton");

        assert!(
            build_full_asset_path("/user/jingles", "/user/jingles/01_chime.ton", &mut out).is_ok()
        );
        assert_eq!(out.as_str(), "/user/jingles/01_chime.ton");
    }
}
