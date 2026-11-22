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
    next_evict: u8,
}

impl<const N: usize> LangStrCache<N> {
    const fn new() -> Self {
        Self {
            slots: [StrCacheSlot {
                key_hash: 0,
                len: 0,
                bytes: [0; 27],
            }; N],
            next_evict: 0,
        }
    }

    #[inline(always)]
    fn clear(&mut self) {
        for s in &mut self.slots {
            s.len = 0;
        }
        self.next_evict = 0;
    }

    #[inline(never)]
    fn lookup<'a>(&self, h: u32, out: &'a mut [u8]) -> Option<Option<&'a str>> {
        for s in &self.slots {
            if s.len != 0 && s.key_hash == h {
                if s.len == 0xFF {
                    return Some(None);
                }
                let n = s.len as usize;
                if n <= out.len() {
                    out[..n].copy_from_slice(&s.bytes[..n]);
                    return Some(Some(unsafe { core::str::from_utf8_unchecked(&out[..n]) }));
                }
            }
        }
        None
    }

    #[inline(never)]
    fn insert(&mut self, h: u32, val: Option<&str>) {
        let idx = (self.next_evict as usize) % N;
        self.next_evict = self.next_evict.wrapping_add(1);
        let slot = &mut self.slots[idx];
        slot.key_hash = h;
        match val {
            Some(s) if !s.is_empty() && s.len() <= slot.bytes.len() => {
                slot.bytes[..s.len()].copy_from_slice(s.as_bytes());
                slot.len = s.len() as u8;
            }
            None => slot.len = 0xFF,
            _ => slot.len = 0,
        }
    }
}

#[inline(never)]
fn read_hdr_16(storage: &mut crate::services::SystemStorage, path: &str, out: &mut [u8; 16]) -> bool {
    !path.is_empty() && storage.read_file_at(path, 0, out).unwrap_or(0) == 16
}

#[inline(never)]
fn load_lang_hdr(storage: &mut crate::services::SystemStorage, path: &str) -> Option<LangPackHeader> {
    let mut b = [0u8; 16];
    read_hdr_16(storage, path, &mut b)
        .then(|| LangPackHeader::from_bytes(&b))
        .filter(|h| h.is_valid())
}

#[inline(never)]
fn load_font_hdr(storage: &mut crate::services::SystemStorage, path: &str) -> Option<FontPackHeader> {
    let mut b = [0u8; 16];
    read_hdr_16(storage, path, &mut b)
        .then(|| FontPackHeader::from_bytes(&b))
        .filter(|h| h.is_valid())
}

/// Build `/user/fonts/<cc>.fnt` for data-driven companion script packs (e.g. `ko.fnt`, `he.fnt`).
fn build_lang_font_path(lang_code: &[u8; 4], out: &mut [u8; 24]) -> usize {
    let code_len = gfx::text::find_byte(lang_code, 0).unwrap_or(4);
    if code_len == 0 {
        return 0;
    }
    const PREFIX: &[u8] = b"/user/fonts/";
    const SUFFIX: &[u8] = b".fnt";
    let total = PREFIX.len() + code_len + SUFFIX.len();
    if total > out.len() {
        return 0;
    }
    out[..PREFIX.len()].copy_from_slice(PREFIX);
    out[PREFIX.len()..PREFIX.len() + code_len].copy_from_slice(&lang_code[..code_len]);
    out[PREFIX.len() + code_len..total].copy_from_slice(SUFFIX);
    total
}

const EXTRA_FONT_NONE: u8 = 0;
const EXTRA_FONT_LANG_CODE: u8 = 1;
const EXTRA_FONT_CJK_JA: u8 = 2;
const EXTRA_FONT_CJK_ZH: u8 = 3;

struct ActiveLang {
    os_hdr: Option<LangPackHeader>,
    app_hdr: Option<LangPackHeader>,
    base_font: Option<FontPackHeader>,
    extra_font_hdr: Option<FontPackHeader>,
    glyph_cache: FontGlyphCache<1>,
    str_cache: LangStrCache<1>,
    app_stem: [u8; 12],
    app_stem_len: u8,
    extra_font_kind: u8,
}

impl ActiveLang {
    const fn new() -> Self {
        Self {
            os_hdr: None,
            app_hdr: None,
            base_font: None,
            extra_font_hdr: None,
            glyph_cache: FontGlyphCache::new(),
            str_cache: LangStrCache::new(),
            app_stem: [0; 12],
            app_stem_len: 0,
            extra_font_kind: EXTRA_FONT_NONE,
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
    st.os_hdr.is_some() || st.app_hdr.is_some()
}

#[inline(always)]
fn active_os_lang_path() -> &'static str {
    #[cfg(not(test))]
    {
        crate::master_config_ref().system.lang_path_str()
    }
    #[cfg(test)]
    {
        ""
    }
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
/// packs (`/user/fonts/tiny5.fnt` + data-driven script pack), and re-bind any active app pack.
#[inline(never)]
pub fn reload_os(storage: &mut crate::services::SystemStorage, sys: &crate::config::SystemConfig) {
    let st = state_mut();
    st.glyph_cache.clear();
    st.str_cache.clear();
    st.base_font = None;
    st.extra_font_hdr = None;
    st.extra_font_kind = EXTRA_FONT_NONE;
    st.os_hdr = if sys.lang_active != 0 {
        load_lang_hdr(storage, sys.lang_path_str())
    } else {
        None
    };
    let Some(os_hdr) = st.os_hdr else {
        st.app_hdr = None;
        gfx::text::set_ext_font_hooks(None, None);
        gfx::text::set_localize_hook(None);
        return;
    };
    st.base_font = load_font_hdr(storage, "/user/fonts/tiny5.fnt");
    let mut fbuf = [0u8; 24];
    let fn_len = build_lang_font_path(&os_hdr.lang_code, &mut fbuf);
    if fn_len > 0
        && let Some(fp) = utils::format::ascii_str(&fbuf[..fn_len])
        && let Some(h) = load_font_hdr(storage, fp)
    {
        st.extra_font_hdr = Some(h);
        st.extra_font_kind = EXTRA_FONT_LANG_CODE;
    } else {
        let (path, kind) = if (os_hdr.font_hint & LANG_FONT_CJK_JA) != 0 {
            ("/user/fonts/cjk_ja.fnt", EXTRA_FONT_CJK_JA)
        } else if (os_hdr.font_hint & LANG_FONT_CJK_ZH) != 0 {
            ("/user/fonts/cjk_zh.fnt", EXTRA_FONT_CJK_ZH)
        } else {
            ("", EXTRA_FONT_NONE)
        };
        if let Some(h) = load_font_hdr(storage, path) {
            st.extra_font_hdr = Some(h);
            st.extra_font_kind = kind;
        }
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
    let fs = crate::kern_fs();
    let mut fbuf = [0u8; 24];
    let extra_path = match st.extra_font_kind {
        EXTRA_FONT_LANG_CODE => st.os_hdr.and_then(|oh| {
            let n = build_lang_font_path(&oh.lang_code, &mut fbuf);
            (n > 0)
                .then(|| utils::format::ascii_str(&fbuf[..n]))
                .flatten()
        }),
        EXTRA_FONT_CJK_JA => Some("/user/fonts/cjk_ja.fnt"),
        EXTRA_FONT_CJK_ZH => Some("/user/fonts/cjk_zh.fnt"),
        _ => None,
    };
    let sources = [
        (0u8, st.base_font, Some("/user/fonts/tiny5.fnt")),
        (1u8, st.extra_font_hdr, extra_path),
    ];
    for (id, hdr_opt, path_opt) in sources {
        if let (Some(hdr), Some(p)) = (hdr_opt, path_opt)
            && let Some(slot) = st.glyph_cache.fetch(
                id,
                &hdr,
                |off, dst| fs.read_file_at(p, off, dst).unwrap_or(0),
                cp,
            )
        {
            return Some(slot);
        }
    }
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
fn resolve_app_lang_path<'a>(st: &ActiveLang, pbuf: &'a mut [u8; 32]) -> Option<&'a str> {
    let os_hdr = st.os_hdr?;
    if st.app_stem_len == 0 {
        return None;
    }
    let stem = utils::format::ascii_str(&st.app_stem[..st.app_stem_len as usize])?;
    let n = build_app_lang_path(stem, &os_hdr.lang_code, pbuf);
    (n > 0).then(|| utils::format::ascii_str(&pbuf[..n])).flatten()
}

#[inline(never)]
fn reload_bound_app(storage: &mut crate::services::SystemStorage) {
    let st = state_mut();
    st.app_hdr = None;
    st.str_cache.clear();
    let mut pbuf = [0u8; 32];
    if let Some(p) = resolve_app_lang_path(st, &mut pbuf) {
        st.app_hdr = load_lang_hdr(storage, p);
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
    st.app_hdr = None;
    st.str_cache.clear();
}

#[inline(never)]
fn lookup_lang_slot<'a>(
    storage: &mut crate::services::SystemStorage,
    hdr: &LangPackHeader,
    path: &str,
    primary_hash: u32,
    fallback_hash: Option<u32>,
    out: &'a mut [u8],
) -> Option<&'a str> {
    let mut read_fn =
        |off: usize, dst: &mut [u8]| storage.read_file_at(path, off, dst).unwrap_or(0);
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
    if let Some(app_hdr) = st.app_hdr {
        let mut pbuf = [0u8; 32];
        if let Some(p) = resolve_app_lang_path(st, &mut pbuf) {
            found = lookup_lang_slot(storage, &app_hdr, p, primary_hash, fallback_hash, out);
        }
    }
    if found.is_none()
        && let Some(os_hdr) = st.os_hdr
    {
        found = lookup_lang_slot(
            storage,
            &os_hdr,
            active_os_lang_path(),
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
    *km = gfx::gui::modules::text_input::LocaleKeymap::empty();
    let st = unsafe { &*core::ptr::addr_of!(ACTIVE_LANG) };
    let Some(hdr) = st.os_hdr else {
        return;
    };
    let path = active_os_lang_path();
    let mut buf = [0u8; 24];
    let Some(lbl) = lookup_lang_slot(
        storage,
        &hdr,
        path,
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
            lookup_lang_slot(storage, &hdr, path, fnv1a_32(&key_bytes), None, &mut buf)
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
}
