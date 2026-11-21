//! Live Localization & Per-App Language Pack Service (`LNG1`).
//!
//! Keeps only ~104 bytes of cached header/path state in SRAM (0 heap) and streams translations
//! directly from `/user/lang/<lang>.lng` (OS pack) and optional per-app `/user/lang/<app>_<cc>.lng`
//! packs. Switching language in Settings reloads immediately with zero reboot required.

#[cfg(not(test))]
use gfx::media::try_draw_font_cp_cached;
use gfx::media::{
    FontGlyphCache, FontPackHeader, LANG_FONT_CJK_JA, LANG_FONT_CJK_ZH, LangPackHeader,
    lookup_lang_short_streaming, lookup_lang_str_streaming,
};
#[cfg(feature = "text_input")]
use gfx::media::{LANG_KEY_KM_KEYS, LANG_KEY_KM_LABEL};

struct PackSlot {
    hdr: Option<LangPackHeader>,
    path: [u8; 32],
    path_len: u8,
}

impl PackSlot {
    const fn empty() -> Self {
        Self {
            hdr: None,
            path: [0; 32],
            path_len: 0,
        }
    }

    #[inline(always)]
    fn path_str(&self) -> &str {
        utils::format::ascii_str(&self.path[..self.path_len as usize]).unwrap_or("")
    }

    #[inline(never)]
    fn clear(&mut self) {
        self.hdr = None;
        self.path_len = 0;
    }

    #[inline(never)]
    fn load(&mut self, storage: &mut crate::services::SystemStorage, path: &str) -> bool {
        self.clear();
        let pb = path.as_bytes();
        if pb.is_empty() || pb.len() > self.path.len() {
            return false;
        }
        let mut hdr_buf = [0u8; LangPackHeader::SIZE];
        if storage.read_file_at(path, 0, &mut hdr_buf).unwrap_or(0) != LangPackHeader::SIZE {
            return false;
        }
        let hdr = LangPackHeader::from_bytes(&hdr_buf);
        if !hdr.is_valid() {
            return false;
        }
        self.path[..pb.len()].copy_from_slice(pb);
        self.path_len = pb.len() as u8;
        self.hdr = Some(hdr);
        true
    }
}

struct FontSlot {
    hdr: Option<FontPackHeader>,
    path: &'static str,
}

impl FontSlot {
    const fn empty() -> Self {
        Self {
            hdr: None,
            path: "",
        }
    }

    #[inline(never)]
    fn load(&mut self, storage: &mut crate::services::SystemStorage, path: &'static str) -> bool {
        self.hdr = None;
        self.path = "";
        let mut hdr_buf = [0u8; FontPackHeader::SIZE];
        if storage.read_file_at(path, 0, &mut hdr_buf).unwrap_or(0) != FontPackHeader::SIZE {
            return false;
        }
        let hdr = FontPackHeader::from_bytes(&hdr_buf);
        if !hdr.is_valid() {
            return false;
        }
        self.path = path;
        self.hdr = Some(hdr);
        true
    }
}

struct ActiveLang {
    os: PackSlot,
    app: PackSlot,
    base_font: FontSlot,
    cjk_font: FontSlot,
    glyph_cache: FontGlyphCache<4>,
    app_stem: [u8; 16],
    app_stem_len: u8,
}

impl ActiveLang {
    const fn new() -> Self {
        Self {
            os: PackSlot::empty(),
            app: PackSlot::empty(),
            base_font: FontSlot::empty(),
            cjk_font: FontSlot::empty(),
            glyph_cache: FontGlyphCache::new(),
            app_stem: [0; 16],
            app_stem_len: 0,
        }
    }
}

static mut ACTIVE_LANG: ActiveLang = ActiveLang::new();

#[inline(always)]
fn state_mut() -> &'static mut ActiveLang {
    unsafe { &mut *core::ptr::addr_of_mut!(ACTIVE_LANG) }
}

#[inline(always)]
pub fn is_active() -> bool {
    let st = unsafe { &*core::ptr::addr_of!(ACTIVE_LANG) };
    st.os.hdr.is_some() || st.app.hdr.is_some()
}

/// Extract bare stem from `/apps/settings.krx` -> `"settings"`.
fn extract_stem(path: &str) -> &str {
    let b = path.as_bytes();
    let mut start = b.len();
    while start > 0 && b[start - 1] != b'/' {
        start -= 1;
    }
    let tail = &b[start..];
    let mut end = tail.len();
    while end > 0 && tail[end - 1] != b'.' {
        end -= 1;
    }
    let stem = if end > 0 { &tail[..end - 1] } else { tail };
    utils::format::ascii_str(stem).unwrap_or("")
}

/// Build `/user/lang/<stem>_<cc>.lng` into `out`, returning the slice length (0 if too long).
pub fn build_app_lang_path(stem: &str, lang_code: &[u8; 4], out: &mut [u8; 32]) -> usize {
    let mut code_len = 0usize;
    while code_len < 4 && lang_code[code_len] != 0 {
        code_len += 1;
    }
    if stem.is_empty() || code_len == 0 {
        return 0;
    }
    const PREFIX: &[u8] = b"/user/lang/";
    const SUFFIX: &[u8] = b".lng";
    let total = PREFIX.len() + stem.len() + 1 + code_len + SUFFIX.len();
    if total > out.len() {
        return 0;
    }
    let mut p = 0;
    out[p..p + PREFIX.len()].copy_from_slice(PREFIX);
    p += PREFIX.len();
    out[p..p + stem.len()].copy_from_slice(stem.as_bytes());
    p += stem.len();
    out[p] = b'_';
    p += 1;
    out[p..p + code_len].copy_from_slice(&lang_code[..code_len]);
    p += code_len;
    out[p..p + SUFFIX.len()].copy_from_slice(SUFFIX);
    p + SUFFIX.len()
}

/// Reload the active OS language pack immediately (zero reboot), mount companion external `.fnt`
/// packs (`/user/fonts/tiny5.fnt` + optional CJK pack), and re-bind any active app pack.
#[inline(never)]
pub fn reload_os(storage: &mut crate::services::SystemStorage, sys: &crate::config::SystemConfig) {
    let st = state_mut();
    st.glyph_cache.clear();
    st.base_font.hdr = None;
    st.cjk_font.hdr = None;
    if sys.lang_active == 0 {
        st.os.clear();
        st.app.clear();
        gfx::text::set_ext_font_hooks(None, None);
        gfx::text::set_localize_hook(None);
        return;
    }
    let path = sys.lang_path_str();
    if !st.os.load(storage, path) {
        st.app.clear();
        gfx::text::set_ext_font_hooks(None, None);
        gfx::text::set_localize_hook(None);
        return;
    }
    let hint = st.os.hdr.map_or(1, |h| h.font_hint);
    let _ = st.base_font.load(storage, "/user/fonts/tiny5.fnt");
    if (hint & LANG_FONT_CJK_JA) != 0 {
        let _ = st.cjk_font.load(storage, "/user/fonts/cjk_ja.fnt");
    } else if (hint & LANG_FONT_CJK_ZH) != 0 {
        let _ = st.cjk_font.load(storage, "/user/fonts/cjk_zh.fnt");
    }
    if st.base_font.hdr.is_some() || st.cjk_font.hdr.is_some() {
        gfx::text::set_ext_font_hooks(Some(ext_glyph_draw_hook), Some(ext_glyph_width_hook));
    } else {
        gfx::text::set_ext_font_hooks(None, None);
    }
    gfx::text::set_localize_hook(Some(os_localize_hook));
    reload_bound_app(storage);
}

fn os_localize_hook<'a>(key: &str, prefer_short: bool, out: &'a mut [u8]) -> Option<&'a str> {
    #[cfg(not(test))]
    {
        return translate_opt(crate::kern_fs(), key, prefer_short, out);
    }
    #[cfg(test)]
    {
        let _ = (key, prefer_short, out);
        None
    }
}

fn ext_glyph_draw_hook(
    canvas: &mut gfx::Canvas,
    x: i16,
    y: i16,
    cp: u16,
    min_x: i16,
    max_x: i16,
    color: gfx::Color,
) -> Option<i16> {
    #[cfg(not(test))]
    {
        let st = state_mut();
        let fs = crate::kern_fs();
        if let Some(hdr) = st.base_font.hdr {
            let p = st.base_font.path;
            if let Some(adv) = try_draw_font_cp_cached(
                canvas,
                x,
                y + 1,
                cp,
                0,
                &hdr,
                |off, dst| fs.read_file_at(p, off, dst).unwrap_or(0),
                &mut st.glyph_cache,
                min_x,
                max_x,
                color,
            ) {
                return Some(adv);
            }
        }
        if let Some(hdr) = st.cjk_font.hdr {
            let p = st.cjk_font.path;
            if let Some(adv) = try_draw_font_cp_cached(
                canvas,
                x,
                y + 1,
                cp,
                1,
                &hdr,
                |off, dst| fs.read_file_at(p, off, dst).unwrap_or(0),
                &mut st.glyph_cache,
                min_x,
                max_x,
                color,
            ) {
                return Some(adv);
            }
        }
    }
    #[cfg(test)]
    let _ = (canvas, x, y, cp, min_x, max_x, color);
    None
}

fn ext_glyph_width_hook(cp: u16) -> Option<i16> {
    #[cfg(not(test))]
    {
        let st = state_mut();
        let fs = crate::kern_fs();
        if let Some(hdr) = st.base_font.hdr {
            let p = st.base_font.path;
            if let Some(slot) = st.glyph_cache.fetch(
                0,
                &hdr,
                |off, dst| fs.read_file_at(p, off, dst).unwrap_or(0),
                cp,
            ) {
                return Some(slot.entry.x_advance as i16);
            }
        }
        if let Some(hdr) = st.cjk_font.hdr {
            let p = st.cjk_font.path;
            if let Some(slot) = st.glyph_cache.fetch(
                1,
                &hdr,
                |off, dst| fs.read_file_at(p, off, dst).unwrap_or(0),
                cp,
            ) {
                return Some(slot.entry.x_advance as i16);
            }
        }
    }
    #[cfg(test)]
    let _ = cp;
    None
}

#[inline(never)]
fn reload_bound_app(storage: &mut crate::services::SystemStorage) {
    let st = state_mut();
    st.app.clear();
    let Some(os_hdr) = st.os.hdr else {
        return;
    };
    if st.app_stem_len == 0 {
        return;
    }
    let stem = utils::format::ascii_str(&st.app_stem[..st.app_stem_len as usize]).unwrap_or("");
    let mut pbuf = [0u8; 32];
    let n = build_app_lang_path(stem, &os_hdr.lang_code, &mut pbuf);
    if n > 0 {
        if let Some(p) = utils::format::ascii_str(&pbuf[..n]) {
            st.app.load(storage, p);
        }
    }
}

/// Bind an optional per-app localized pack (`/user/lang/<stem>_<cc>.lng`) on KRX app launch.
#[inline(never)]
pub fn bind_app(storage: &mut crate::services::SystemStorage, app_path: &str) {
    let st = state_mut();
    let stem = extract_stem(app_path);
    let n = stem.len().min(st.app_stem.len());
    st.app_stem[..n].copy_from_slice(&stem.as_bytes()[..n]);
    st.app_stem_len = n as u8;
    reload_bound_app(storage);
}

/// Clear any active per-app localization binding on KRX app exit.
#[inline(always)]
pub fn unbind_app() {
    let st = state_mut();
    st.app_stem_len = 0;
    st.app.clear();
}

/// Look up `key` via `app pack -> OS pack`, returning `None` on miss or when inactive.
#[inline(never)]
pub fn translate_opt<'a>(
    storage: &mut crate::services::SystemStorage,
    key: &str,
    prefer_short: bool,
    out: &'a mut [u8],
) -> Option<&'a str> {
    let st = unsafe { &*core::ptr::addr_of!(ACTIVE_LANG) };
    if st.os.hdr.is_none() && st.app.hdr.is_none() {
        return None;
    }
    for slot in [&st.app, &st.os] {
        if let Some(hdr) = slot.hdr {
            let path = slot.path_str();
            let res = if prefer_short {
                lookup_lang_short_streaming(
                    &hdr,
                    |off, dst| storage.read_file_at(path, off, dst).unwrap_or(0),
                    key,
                    out,
                )
            } else {
                lookup_lang_str_streaming(
                    &hdr,
                    |off, dst| storage.read_file_at(path, off, dst).unwrap_or(0),
                    key,
                    out,
                )
            };
            if res.is_some() {
                return res;
            }
        }
    }
    None
}

/// Translate `key` via `app pack -> OS pack -> English literal` with 0 SPI I/O when inactive.
#[inline(always)]
pub fn translate<'a>(
    storage: &mut crate::services::SystemStorage,
    key: &'a str,
    prefer_short: bool,
    out: &'a mut [u8],
) -> &'a str {
    translate_opt(storage, key, prefer_short, out).unwrap_or(key)
}

/// Populate `km` from the active OS `.lng` pack's optional `[keymap]` entries (`__KM_LABEL__`, `__KM_0__..__KM_9__`).
#[cfg(feature = "text_input")]
#[inline(never)]
pub fn load_locale_keymap(
    storage: &mut crate::services::SystemStorage,
    km: &mut gfx::gui::modules::text_input::LocaleKeymap,
) {
    *km = gfx::gui::modules::text_input::LocaleKeymap::empty();
    let st = unsafe { &*core::ptr::addr_of!(ACTIVE_LANG) };
    let Some(hdr) = st.os.hdr else {
        return;
    };
    let path = st.os.path_str();
    let mut buf = [0u8; 24];
    let Some(lbl) = lookup_lang_str_streaming(
        &hdr,
        |off, dst| storage.read_file_at(path, off, dst).unwrap_or(0),
        LANG_KEY_KM_LABEL,
        &mut buf,
    ) else {
        return;
    };
    let _ = km.label.push_str(lbl);
    let mut any_key = false;
    for (i, &k) in LANG_KEY_KM_KEYS.iter().enumerate() {
        if let Some(seq) = lookup_lang_str_streaming(
            &hdr,
            |off, dst| storage.read_file_at(path, off, dst).unwrap_or(0),
            k,
            &mut buf,
        ) {
            let _ = km.keys[i].push_str(seq);
            any_key = true;
        }
    }
    km.active = any_key;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_lang_path_builds_cleanly_within_32_bytes() {
        let mut buf = [0u8; 32];
        let n = build_app_lang_path("settings", b"ja\0\0", &mut buf);
        assert_eq!(&buf[..n], b"/user/lang/settings_ja.lng");

        let n2 = build_app_lang_path("beacon", b"zhCN", &mut buf);
        assert_eq!(&buf[..n2], b"/user/lang/beacon_zhCN.lng");

        assert_eq!(extract_stem("/apps/settings.krx"), "settings");
        assert_eq!(extract_stem("/apps/calib.mod"), "calib");
    }
}
