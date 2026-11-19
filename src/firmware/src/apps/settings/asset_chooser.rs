//! Generalized Dynamic File & Asset Chooser for Kern OS
//!
//! Provides zero-alloc, stack-safe directory asset browsing across LittleFS,
//! dynamic name extraction (e.g. "01_roger.ton" -> "Roger"), and OptionSheet generation.
//! Completely decoupled from specific asset domains: takes directory paths and extensions dynamically.

use gfx::gui::modules::menu::{SheetOptions, ShortString};

pub const MAX_CHOOSER_ASSETS: usize = 8;
pub const TAG_ASSET_CHOOSER: u8 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AssetId {
    Splash = 0,
    Roger = 1,
    Jingle = 2,
}

#[repr(C)]
struct AssetMeta {
    dom: u8,
    flag_off: u8,
    path_off: u8,
    _pad: u8,
}

static ASSET_META: [AssetMeta; 3] = [
    AssetMeta {
        dom: 0,
        flag_off: core::mem::offset_of!(crate::config::DisplayConfig, boot_splash_mode) as u8,
        path_off: core::mem::offset_of!(crate::config::DisplayConfig, splash_path) as u8,
        _pad: 0,
    },
    AssetMeta {
        dom: 1,
        flag_off: core::mem::offset_of!(crate::config::AudioConfig, roger_beep) as u8,
        path_off: core::mem::offset_of!(crate::config::AudioConfig, roger_path) as u8,
        _pad: 0,
    },
    AssetMeta {
        dom: 1,
        flag_off: core::mem::offset_of!(crate::config::AudioConfig, boot_jingle) as u8,
        path_off: core::mem::offset_of!(crate::config::AudioConfig, jingle_path) as u8,
        _pad: 0,
    },
];

const _: () = {
    assert!(
        core::mem::offset_of!(crate::config::DisplayConfig, splash_path) + 32
            <= core::mem::size_of::<crate::config::DisplayConfig>()
    );
    assert!(
        core::mem::offset_of!(crate::config::AudioConfig, roger_path) + 32
            <= core::mem::size_of::<crate::config::AudioConfig>()
    );
    assert!(
        core::mem::offset_of!(crate::config::AudioConfig, jingle_path) + 32
            <= core::mem::size_of::<crate::config::AudioConfig>()
    );
};

utils::static_bytes!(
    static ASSET_STRS = b"\
Boot Splash\0/user/splash\0.rle|.img|.bin\0None\0\
Roger Beep\0/user/rogers\0.ton|.rgr\0Off\0\
Boot Jingle\0/user/jingles\0.ton\0Off\0";
);

impl AssetId {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Splash),
            1 => Some(Self::Roger),
            2 => Some(Self::Jingle),
            _ => None,
        }
    }

    #[inline(never)]
    fn str_field(self, field: u8) -> &'static str {
        utils::format::nth_cstr(&ASSET_STRS, (self as u8) * 4 + field)
    }

    #[inline(never)]
    fn get_path(self, cfg: &crate::config::MasterConfig) -> (bool, &str) {
        let m = &ASSET_META[self as usize];
        let bytes = cfg.config_bytes(m.dom as usize);
        let off = m.path_off as usize;
        // SAFETY: `ASSET_META` offsets are validated against `DisplayConfig`/`AudioConfig` lengths.
        let path_buf = unsafe { core::slice::from_raw_parts(bytes.as_ptr().add(off), 32) };
        (
            unsafe { *bytes.as_ptr().add(m.flag_off as usize) } != 0,
            crate::config::read_cstr(path_buf),
        )
    }

    #[inline(never)]
    fn set_path(self, cfg: &mut crate::config::MasterConfig, opt: Option<&str>) {
        let m = &ASSET_META[self as usize];
        let bytes = cfg.config_bytes_mut(m.dom as usize);
        unsafe {
            *bytes.as_mut_ptr().add(m.flag_off as usize) = opt.is_some() as u8;
        }
        if let Some(p) = opt {
            let off = m.path_off as usize;
            let path_buf =
                unsafe { core::slice::from_raw_parts_mut(bytes.as_mut_ptr().add(off), 32) };
            crate::config::write_cstr(path_buf, p);
        }
    }

    #[inline(never)]
    fn finalize_entries(
        self,
        cfg: &crate::config::MasterConfig,
        storage: &mut AssetChooserStorage,
    ) -> usize {
        let count = storage.count as usize;
        for i in 0..count {
            storage.order[i] = i as u8;
        }
        for i in 1..count {
            let mut j = i;
            while j > 0
                && storage.paths[storage.order[j] as usize].as_str()
                    < storage.paths[storage.order[j - 1] as usize].as_str()
            {
                storage.order.swap(j, j - 1);
                j -= 1;
            }
        }

        let (active, cur_path) = self.get_path(cfg);
        let cb = cur_path.as_bytes();
        let mut current_sel = 0;
        for idx in 0..count {
            let full_path = storage.paths[storage.order[idx] as usize].as_str();
            format_asset_name_into(full_path, &mut storage.labels[idx + 1]);
            let fb = full_path.as_bytes();
            if active
                && current_sel == 0
                && fb.len() >= cb.len()
                && (cb.first() != Some(&b'/') || fb.len() == cb.len())
                && &fb[fb.len() - cb.len()..] == cb
            {
                current_sel = idx + 1;
            }
        }
        if active && current_sel == 0 && count > 0 {
            current_sel = 1;
        }
        current_sel
    }

    #[inline(never)]
    pub fn open_sheet(self, ctx: &mut crate::SysContext) {
        let storage = storage_mut();
        storage.labels[0].set_str(self.str_field(3));
        storage.count = 0;
        let dir = self.str_field(1);
        let exts = self.str_field(2);

        #[cfg(feature = "filesystem")]
        let _ = ctx
            .storage
            .fs
            .list_dir(dir, |entry_name, _size, _perm, is_dir| {
                let c = storage.count as usize;
                if c < MAX_CHOOSER_ASSETS
                    && !is_dir
                    && has_extension(entry_name, exts)
                    && build_full_asset_path(dir, entry_name, &mut storage.paths[c])
                {
                    storage.count = (c + 1) as u8;
                }
            });

        let current_sel = self.finalize_entries(&ctx.config, storage);
        let count = storage.count as usize;
        let labels_slice: &'static [ShortString] =
            unsafe { core::slice::from_raw_parts(storage.labels.as_ptr(), count + 1) };
        super::dialog::show_radio_sheet(
            ctx,
            super::dialog::TAG_ASSET_CHOOSER,
            self.str_field(0),
            SheetOptions::Dynamic(labels_slice),
            current_sel,
        );
    }

    pub fn apply_choice(self, sel: usize, config: &mut crate::config::MasterConfig) -> bool {
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

    pub fn format_status(self, config: &crate::config::MasterConfig, out: &mut ShortString) {
        let (active, path) = self.get_path(config);
        if !active {
            out.set_str(self.str_field(3));
        } else {
            format_asset_name_into(path, out);
        }
    }
}

#[derive(Clone, Copy)]
struct PathBuf32 {
    bytes: [u8; 31],
    len: u8,
}

impl PathBuf32 {
    #[cfg(test)]
    const fn empty() -> Self {
        Self {
            bytes: [0; 31],
            len: 0,
        }
    }

    #[inline(always)]
    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    #[inline(always)]
    fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(self.as_bytes()) }
    }
}

const _: () = assert!(core::mem::size_of::<PathBuf32>() == 32);

struct AssetChooserStorage {
    count: u8,
    order: [u8; MAX_CHOOSER_ASSETS],
    labels: [ShortString; MAX_CHOOSER_ASSETS + 1],
    paths: [PathBuf32; MAX_CHOOSER_ASSETS],
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
#[inline(never)]
pub fn format_asset_name_into(path: &str, out: &mut ShortString) {
    let raw = path.as_bytes();
    let mut name_start = raw.len();
    while name_start > 0 && raw[name_start - 1] != b'/' {
        name_start -= 1;
    }
    let tail = &raw[name_start..];
    let mut dot = tail.len();
    while dot > 0 && tail[dot - 1] != b'.' {
        dot -= 1;
    }
    let base = if dot > 0 { &tail[..dot - 1] } else { tail };

    let mut digits = 0usize;
    while digits < base.len() && base[digits].is_ascii_digit() {
        digits += 1;
    }
    let clean = if digits > 0 && matches!(base.get(digits), Some(b'_' | b'-')) {
        &base[digits + 1..]
    } else {
        base
    };
    if clean.is_empty() {
        out.set_str("Asset");
        return;
    }

    out.len = 0;
    let mut cap = true;
    for &b in clean {
        if (out.len as usize) >= out.bytes.len() {
            break;
        }
        let ch = if b == b'_' || b == b'-' {
            cap = true;
            b' '
        } else if cap {
            cap = false;
            b.to_ascii_uppercase()
        } else {
            b
        };
        out.bytes[out.len as usize] = ch;
        out.len += 1;
    }
}

fn has_extension(path: &str, extensions: &str) -> bool {
    let eb_all = extensions.as_bytes();
    if eb_all.is_empty() {
        return true;
    }
    let b = path.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= eb_all.len() {
        if i == eb_all.len() || eb_all[i] == b'|' {
            let eb = &eb_all[start..i];
            if !eb.is_empty() && b.len() >= eb.len() && &b[b.len() - eb.len()..] == eb {
                return true;
            }
            start = i + 1;
        }
        i += 1;
    }
    false
}

#[inline(never)]
fn build_full_asset_path(dir: &str, entry_name: &str, out: &mut PathBuf32) -> bool {
    let name = entry_name.as_bytes();
    let need_slash = usize::from(name.first() != Some(&b'/'));
    let prefix = if need_slash == 0 {
        b"".as_slice()
    } else {
        utils::format::trim_trailing_slashes(dir).as_bytes()
    };
    let total = prefix.len() + need_slash + name.len();
    if total > 31 {
        return false;
    }
    out.bytes[..prefix.len()].copy_from_slice(prefix);
    if need_slash != 0 {
        out.bytes[prefix.len()] = b'/';
    }
    let off = prefix.len() + need_slash;
    out.bytes[off..total].copy_from_slice(name);
    out.len = total as u8;
    true
}

/// Retrieve the selected asset path from storage
pub fn get_chosen_asset_path(sel: usize) -> Option<&'static str> {
    if sel == 0 {
        return None;
    }
    let storage = storage_ref();
    let idx = sel - 1;
    if idx < (storage.count as usize).min(MAX_CHOOSER_ASSETS) {
        let slot = storage.order[idx] as usize;
        if slot < MAX_CHOOSER_ASSETS && storage.paths[slot].len > 0 {
            return Some(storage.paths[slot].as_str());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_full_asset_path() {
        let mut out = PathBuf32::empty();
        assert!(build_full_asset_path(
            "/user/splash",
            "01_quansheng.rle",
            &mut out
        ));
        assert_eq!(out.as_str(), "/user/splash/01_quansheng.rle");

        assert!(build_full_asset_path(
            "/user/rogers/",
            "03_mototrbo.ton",
            &mut out
        ));
        assert_eq!(out.as_str(), "/user/rogers/03_mototrbo.ton");

        assert!(build_full_asset_path(
            "/user/jingles",
            "/user/jingles/01_chime.ton",
            &mut out
        ));
        assert_eq!(out.as_str(), "/user/jingles/01_chime.ton");
    }
}
