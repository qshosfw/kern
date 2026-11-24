//! Live Localization & Per-App Language Pack Service (`LNG1`).
//!
//! Keeps only ~104 bytes of cached header/path state in SRAM (0 heap) and streams translations
//! directly from `/user/lang/<lang>.lng` (OS pack) and optional per-app `/user/lang/<app>_<cc>.lng`
//! packs. Switching language in Settings reloads immediately with zero reboot required.

#[cfg(not(test))]
use gfx::media::draw_cached_glyph;
use gfx::media::{
    FontGlyphCache, FontPackHeader, LANG_FONT_CJK_JA, LANG_FONT_CJK_ZH, LangPackHeader, fnv1a_32,
    fnv1a_32_short, lookup_lang_hash_streaming,
};
#[cfg(feature = "text_input")]
use gfx::media::{LANG_KEY_KM_LABEL, lang_km_key_bytes};

/// Power-of-two 32-byte RAM cache slot for hot UI translations (eliminates repeated SPI Flash
/// binary searches during 10 Hz marquee and kinetic scroll frames).
/// `len == 0xFF` marks a cached negative miss; `len == 0` marks an empty slot.
#[derive(Clone, Copy)]
#[repr(C)]
struct StrCacheSlot {
    key_hash: u32,
    len: u8,
    bytes: [u8; 27],
}

const _: () = assert!(core::mem::size_of::<StrCacheSlot>() == 32);

struct LangStrCache<const N: usize> {
    slots: [StrCacheSlot; N],
}

impl<const N: usize> LangStrCache<N> {
    const fn new() -> Self {
        assert!(N.is_power_of_two());
        Self {
            slots: [StrCacheSlot {
                key_hash: 0,
                len: 0,
                bytes: [0; 27],
            }; N],
        }
    }

    #[inline(always)]
    fn clear(&mut self) {
        for s in &mut self.slots {
            s.len = 0;
        }
    }

    #[inline(never)]
    fn lookup<'a>(&self, h: u32, out: &'a mut [u8]) -> Option<Option<&'a str>> {
        let s = unsafe { self.slots.get_unchecked((h as usize) & (N - 1)) };
        if s.len != 0 && s.key_hash == h {
            if s.len == 0xFF {
                return Some(None);
            }
            let n = s.len as usize;
            if n <= out.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(s.bytes.as_ptr(), out.as_mut_ptr(), n);
                    return Some(Some(core::str::from_utf8_unchecked(
                        out.get_unchecked(..n),
                    )));
                }
            }
        }
        None
    }

    #[inline(never)]
    fn insert(&mut self, h: u32, val: Option<&str>) {
        let slot = unsafe { self.slots.get_unchecked_mut((h as usize) & (N - 1)) };
        slot.key_hash = h;
        match val {
            Some(s) if !s.is_empty() && s.len() <= slot.bytes.len() => {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        s.as_bytes().as_ptr(),
                        slot.bytes.as_mut_ptr(),
                        s.len(),
                    );
                }
                slot.len = s.len() as u8;
            }
            None => slot.len = 0xFF,
            _ => slot.len = 0,
        }
    }
}

#[inline(never)]
fn open_hdr_16(storage: &mut crate::services::SystemStorage, path: &str, out: &mut [u8; 16]) -> u16 {
    if path.is_empty() {
        return 0;
    }
    let Ok(ino) = storage.lookup_ino(path) else {
        return 0;
    };
    if storage.read_ino_at(ino, 0, out).unwrap_or(0) == 16 {
        ino
    } else {
        0
    }
}

#[inline(never)]
fn load_lang_hdr(
    storage: &mut crate::services::SystemStorage,
    path: &str,
) -> (Option<LangPackHeader>, u16) {
    let mut b = [0u8; 16];
    let ino = open_hdr_16(storage, path, &mut b);
    if ino >= 2 {
        let h = LangPackHeader::from_bytes(&b);
        if h.is_valid() {
            return (Some(h), ino);
        }
    }
    (None, 0)
}

#[inline(never)]
fn load_font_hdr(
    storage: &mut crate::services::SystemStorage,
    path: &str,
) -> (Option<FontPackHeader>, u16) {
    let mut b = [0u8; 16];
    let ino = open_hdr_16(storage, path, &mut b);
    if ino >= 2 {
        let h = FontPackHeader::from_bytes(&b);
        if h.is_valid() {
            return (Some(h), ino);
        }
    }
    (None, 0)
}

#[inline(never)]
fn join_bytes(out: &mut [u8], parts: &[&[u8]]) -> usize {
    let mut p = 0usize;
    for &part in parts {
        if p + part.len() > out.len() {
            return 0;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(part.as_ptr(), out.as_mut_ptr().add(p), part.len());
        }
        p += part.len();
    }
    p
}

/// Build `/user/fonts/<cc>.fnt` for data-driven companion script packs (e.g. `ko.fnt`, `he.fnt`).
fn build_lang_font_path(lang_code: &[u8; 4], out: &mut [u8; 24]) -> usize {
    let code_len = gfx::text::find_byte(lang_code, 0).unwrap_or(4);
    if code_len == 0 {
        return 0;
    }
    join_bytes(out, &[b"/user/fonts/", &lang_code[..code_len], b".fnt"])
}

const TINY5_FONT_PATH: &str = "/user/fonts/tiny5.fnt";

struct ActiveLang {
    os_hdr: Option<LangPackHeader>,
    app_hdr: Option<LangPackHeader>,
    base_font: Option<FontPackHeader>,
    extra_font_hdr: Option<FontPackHeader>,
    os_ino: u16,
    app_ino: u16,
    base_font_ino: u16,
    extra_font_ino: u16,
    glyph_cache: FontGlyphCache<16>,
    str_cache: LangStrCache<16>,
    app_stem: utils::format::InlineStr<12>,
}

impl ActiveLang {
    const fn new() -> Self {
        Self {
            os_hdr: None,
            app_hdr: None,
            base_font: None,
            extra_font_hdr: None,
            os_ino: 0,
            app_ino: 0,
            base_font_ino: 0,
            extra_font_ino: 0,
            glyph_cache: FontGlyphCache::new(),
            str_cache: LangStrCache::new(),
            app_stem: utils::format::InlineStr::new(),
        }
    }
}

static mut ACTIVE_LANG: ActiveLang = ActiveLang::new();

#[inline(never)]
fn state_mut() -> &'static mut ActiveLang {
    unsafe { &mut *core::ptr::addr_of_mut!(ACTIVE_LANG) }
}

#[inline(always)]
pub fn is_active() -> bool {
    let st = state_mut();
    st.os_hdr.is_some() || st.app_hdr.is_some()
}

/// Extract bare stem from `/apps/settings.krx` -> `"settings"`.
fn extract_stem(path: &str) -> &str {
    let fname = crate::apps::launcher::catalog::extract_filename(path);
    let end = gfx::text::rfind_byte(fname, b'.').unwrap_or(fname.len());
    &fname[..end]
}

/// Build `/user/lang/<stem>_<cc>.lng` into `out`, returning the slice length (0 if too long).
pub fn build_app_lang_path(stem: &str, lang_code: &[u8; 4], out: &mut [u8; 32]) -> usize {
    let code_len = gfx::text::find_byte(lang_code, 0).unwrap_or(4);
    if stem.is_empty() || code_len == 0 {
        return 0;
    }
    join_bytes(
        out,
        &[b"/user/lang/", stem.as_bytes(), b"_", &lang_code[..code_len], b".lng"],
    )
}

#[inline(never)]
fn try_load_extra_font(
    storage: &mut crate::services::SystemStorage,
    st: &mut ActiveLang,
    path: &str,
) -> bool {
    let (hdr, ino) = load_font_hdr(storage, path);
    if hdr.is_some() {
        st.extra_font_hdr = hdr;
        st.extra_font_ino = ino;
        true
    } else {
        false
    }
}

/// Reload the active OS language pack immediately (zero reboot), mount companion external `.fnt`
/// packs (`/user/fonts/tiny5.fnt` + data-driven script pack), and re-bind any active app pack.
#[inline(never)]
pub fn reload_os(storage: &mut crate::services::SystemStorage, sys: &crate::config::SystemConfig) {
    let st = state_mut();
    st.glyph_cache.clear();
    st.str_cache.clear();
    st.base_font = None;
    st.base_font_ino = 0;
    st.extra_font_hdr = None;
    st.extra_font_ino = 0;
    let (os_hdr, os_ino) = if sys.lang_active != 0 {
        load_lang_hdr(storage, sys.lang_path_str())
    } else {
        (None, 0)
    };
    st.os_hdr = os_hdr;
    st.os_ino = os_ino;
    let Some(os_hdr) = st.os_hdr else {
        st.app_hdr = None;
        st.app_ino = 0;
        gfx::text::set_ext_font_hooks(None, None);
        gfx::text::set_localize_hook(None);
        return;
    };
    let (bf, bf_ino) = load_font_hdr(storage, TINY5_FONT_PATH);
    st.base_font = bf;
    st.base_font_ino = bf_ino;
    let mut fbuf = [0u8; 24];
    let fn_len = build_lang_font_path(&os_hdr.lang_code, &mut fbuf);
    let loaded = fn_len > 0
        && utils::format::ascii_str(&fbuf[..fn_len])
            .is_some_and(|fp| try_load_extra_font(storage, st, fp));
    if !loaded {
        let path = if (os_hdr.font_hint & LANG_FONT_CJK_JA) != 0 {
            "/user/fonts/cjk_ja.fnt"
        } else if (os_hdr.font_hint & LANG_FONT_CJK_ZH) != 0 {
            "/user/fonts/cjk_zh.fnt"
        } else {
            ""
        };
        try_load_extra_font(storage, st, path);
    }
    if st.base_font.is_some() || st.extra_font_hdr.is_some() {
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

#[cfg(not(test))]
#[inline(never)]
fn fetch_ext_glyph(cp: u16) -> Option<&'static gfx::media::CachedGlyphSlot> {
    let st = state_mut();
    if let Some(hit) = st.glyph_cache.peek(0, cp) {
        return hit;
    }
    let fs = crate::kern_fs();
    let base = (st.base_font.as_ref(), st.base_font_ino);
    let extra = (st.extra_font_hdr.as_ref(), st.extra_font_ino);
    let sources = if cp >= 0x1100 {
        [extra, base]
    } else {
        [base, extra]
    };
    for (hdr_opt, ino) in sources {
        if let Some(hdr) = hdr_opt
            && ino >= 2
            && let Some(slot) = st.glyph_cache.fetch(
                0,
                hdr,
                |off, dst| fs.read_ino_at(ino, off, dst).unwrap_or(0),
                cp,
            )
        {
            return Some(slot);
        }
    }
    st.glyph_cache.put_miss(0, cp);
    None
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
        let slot = fetch_ext_glyph(cp)?;
        return Some(draw_cached_glyph(
            canvas,
            x,
            y + 1,
            slot,
            min_x,
            max_x,
            color,
        ));
    }
    #[cfg(test)]
    {
        let _ = (canvas, x, y, cp, min_x, max_x, color);
        None
    }
}

fn ext_glyph_width_hook(cp: u16) -> Option<i16> {
    #[cfg(not(test))]
    {
        return fetch_ext_glyph(cp).map(|slot| slot.entry.x_advance as i16);
    }
    #[cfg(test)]
    {
        let _ = cp;
        None
    }
}

#[inline(never)]
fn reload_bound_app(storage: &mut crate::services::SystemStorage) {
    let st = state_mut();
    st.app_hdr = None;
    st.app_ino = 0;
    st.str_cache.clear();
    if let Some(os_hdr) = st.os_hdr
        && !st.app_stem.is_empty()
    {
        let mut pbuf = [0u8; 32];
        let n = build_app_lang_path(st.app_stem.as_str(), &os_hdr.lang_code, &mut pbuf);
        if n > 0
            && let Some(p) = utils::format::ascii_str(&pbuf[..n])
        {
            let (hdr, ino) = load_lang_hdr(storage, p);
            st.app_hdr = hdr;
            st.app_ino = ino;
        }
    }
}

/// Bind an optional per-app localized pack (`/user/lang/<stem>_<cc>.lng`) on KRX app launch.
#[inline(never)]
pub fn bind_app(storage: &mut crate::services::SystemStorage, app_path: &str) {
    state_mut().app_stem.set(extract_stem(app_path));
    reload_bound_app(storage);
}

/// Clear any active per-app localization binding on KRX app exit.
#[inline(always)]
pub fn unbind_app() {
    let st = state_mut();
    st.app_stem.clear();
    st.app_hdr = None;
    st.app_ino = 0;
    st.str_cache.clear();
}

#[inline(never)]
fn lookup_lang_slot<'a>(
    storage: &mut crate::services::SystemStorage,
    hdr: &LangPackHeader,
    ino: u16,
    primary_hash: u32,
    fallback_hash: Option<u32>,
    out: &'a mut [u8],
) -> Option<&'a str> {
    let mut read_fn = |off: usize, dst: &mut [u8]| storage.read_ino_at(ino, off, dst).unwrap_or(0);
    if let Some(s) = lookup_lang_hash_streaming(hdr, &mut read_fn, primary_hash, out) {
        return Some(s);
    }
    if let Some(fb) = fallback_hash {
        return lookup_lang_hash_streaming(hdr, &mut read_fn, fb, out);
    }
    None
}

/// Look up `key` via `RAM cache -> app pack -> OS pack`, returning `None` on miss or when inactive.
#[inline(never)]
pub fn translate_opt<'a>(
    storage: &mut crate::services::SystemStorage,
    key: &str,
    prefer_short: bool,
    out: &'a mut [u8],
) -> Option<&'a str> {
    let st = state_mut();
    if st.os_hdr.is_none() && st.app_hdr.is_none() {
        return None;
    }
    let full_hash = fnv1a_32(key.as_bytes());
    let (primary_hash, fallback_hash) = if prefer_short {
        (fnv1a_32_short(key.as_bytes()), Some(full_hash))
    } else {
        (full_hash, None)
    };
    if let Some(hit) = st.str_cache.lookup(primary_hash, out) {
        return hit;
    }
    let mut found = None;
    if let Some(app_hdr) = st.app_hdr
        && st.app_ino >= 2
    {
        found = lookup_lang_slot(
            storage,
            &app_hdr,
            st.app_ino,
            primary_hash,
            fallback_hash,
            out,
        );
    }
    if found.is_none()
        && let Some(os_hdr) = st.os_hdr
        && st.os_ino >= 2
    {
        found = lookup_lang_slot(
            storage,
            &os_hdr,
            st.os_ino,
            primary_hash,
            fallback_hash,
            out,
        );
    }
    st.str_cache.insert(primary_hash, found);
    found
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
    unsafe { core::ptr::write_bytes(km as *mut _ as *mut u8, 0, core::mem::size_of_val(km)) };
    let st = state_mut();
    let Some(hdr) = st.os_hdr else {
        return;
    };
    let ino = st.os_ino;
    if ino < 2 {
        return;
    }
    let mut buf = [0u8; 24];
    let Some(lbl) = lookup_lang_slot(
        storage,
        &hdr,
        ino,
        fnv1a_32(LANG_KEY_KM_LABEL.as_bytes()),
        None,
        &mut buf,
    ) else {
        return;
    };
    km.set_label(lbl);
    let mut any_key = false;
    for i in 0..10 {
        let key_bytes = lang_km_key_bytes(i as u8);
        if let Some(seq) =
            lookup_lang_slot(storage, &hdr, ino, fnv1a_32(&key_bytes), None, &mut buf)
        {
            km.set_key(i, seq);
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

    #[test]
    fn direct_mapped_lang_str_cache_hits_misses_and_evicts() {
        let mut cache = LangStrCache::<8>::new();
        let mut out = [0u8; 32];
        assert!(cache.lookup(0x1001, &mut out).is_none());

        cache.insert(0x1001, Some("Settings"));
        cache.insert(0x1002, None);
        assert_eq!(cache.lookup(0x1001, &mut out), Some(Some("Settings")));
        assert_eq!(cache.lookup(0x1002, &mut out), Some(None));

        // Same low 3 bits (`1`) replaces slot 1 cleanly in O(1)
        cache.insert(0x2009, Some("Radio"));
        assert!(cache.lookup(0x1001, &mut out).is_none());
        assert_eq!(cache.lookup(0x2009, &mut out), Some(Some("Radio")));

        cache.clear();
        assert!(cache.lookup(0x2009, &mut out).is_none());
    }
}
