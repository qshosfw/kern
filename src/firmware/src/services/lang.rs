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
        assert!(N.is_power_of_two() && N <= 256);
        Self {
            slots: [StrCacheSlot {
                key_hash: 0,
                len: 0,
                bytes: [0; 27],
            }; N],
            next_evict: 0,
        }
    }

    #[inline(never)]
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
                    unsafe {
                        core::ptr::copy_nonoverlapping(s.bytes.as_ptr(), out.as_mut_ptr(), n);
                        return Some(Some(core::str::from_utf8_unchecked(out.get_unchecked(..n))));
                    }
                }
            }
        }
        None
    }

    #[inline(never)]
    fn insert(&mut self, h: u32, val: Option<&str>) {
        let idx = (self.next_evict as usize) & (N - 1);
        self.next_evict = self.next_evict.wrapping_add(1) & ((N - 1) as u8);
        let slot = unsafe { self.slots.get_unchecked_mut(idx) };
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
fn open_hdr_16(
    storage: &mut crate::services::SystemStorage,
    path: &str,
    out: &mut [u8; 16],
) -> u16 {
    if !path.is_empty()
        && let Ok(ino) = storage.lookup_ino(path)
        && ino >= 2
        && storage.read_ino_at(ino, 0, out).unwrap_or(0) == 16
    {
        return ino;
    }
    0
}

#[inline(never)]
fn load_lang_hdr(
    storage: &mut crate::services::SystemStorage,
    path: &str,
    out_hdr: &mut Option<LangPackHeader>,
) -> u16 {
    *out_hdr = None;
    let mut b = [0u8; 16];
    let ino = open_hdr_16(storage, path, &mut b);
    if ino != 0 {
        let h = LangPackHeader::from_bytes(&b);
        if h.is_valid() {
            *out_hdr = Some(h);
            return ino;
        }
    }
    0
}

#[inline(never)]
fn build_lang_path(is_app: bool, stem: &[u8], code: &[u8], out: &mut [u8]) -> usize {
    let clen = gfx::text::find_byte(code, 0).unwrap_or(code.len());
    if clen == 0 {
        return 0;
    }
    let (prefix, ext): (&[u8], &[u8]) = if is_app {
        (b"/user/lang/", b".lng")
    } else {
        (b"/user/fonts/", b".fnt")
    };
    let sep = usize::from(!stem.is_empty());
    let total = prefix.len() + stem.len() + sep + clen + ext.len();
    if total > out.len() {
        return 0;
    }
    let dst = out.as_mut_ptr();
    let mut p = prefix.len();
    unsafe {
        core::ptr::copy_nonoverlapping(prefix.as_ptr(), dst, p);
        if sep != 0 {
            core::ptr::copy_nonoverlapping(stem.as_ptr(), dst.add(p), stem.len());
            p += stem.len();
            *dst.add(p) = b'_';
            p += 1;
        }
        core::ptr::copy_nonoverlapping(code.as_ptr(), dst.add(p), clen);
        core::ptr::copy_nonoverlapping(ext.as_ptr(), dst.add(p + clen), ext.len());
    }
    total
}

#[inline(never)]
fn load_font_stem(
    storage: &mut crate::services::SystemStorage,
    stem: &[u8],
    out_hdr: &mut Option<FontPackHeader>,
) -> u16 {
    *out_hdr = None;
    let mut fbuf = [0u8; 24];
    let n = build_lang_path(false, b"", stem, &mut fbuf);
    if n > 0
        && let Some(fp) = utils::format::ascii_str(&fbuf[..n])
    {
        let mut b = [0u8; 16];
        let ino = open_hdr_16(storage, fp, &mut b);
        if ino != 0 {
            let h = FontPackHeader::from_bytes(&b);
            if h.is_valid() {
                *out_hdr = Some(h);
                return ino;
            }
        }
    }
    0
}

struct ActiveLang {
    os_hdr: Option<LangPackHeader>,
    app_hdr: Option<LangPackHeader>,
    base_font: Option<FontPackHeader>,
    extra_font_hdr: Option<FontPackHeader>,
    os_ino: u16,
    app_ino: u16,
    base_font_ino: u16,
    extra_font_ino: u16,
    glyph_cache: FontGlyphCache<24>,
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
    if stem.is_empty() {
        return 0;
    }
    build_lang_path(true, stem.as_bytes(), lang_code, out)
}

/// Reload the active OS language pack immediately (zero reboot), mount companion external `.fnt`
/// packs (`/user/fonts/tiny5.fnt` + data-driven script pack), and re-bind any active app pack.
#[inline(never)]
pub fn reload_os(storage: &mut crate::services::SystemStorage, sys: &crate::config::SystemConfig) {
    let st = state_mut();
    st.glyph_cache.clear();
    st.os_ino = if sys.lang_active != 0 {
        load_lang_hdr(storage, sys.lang_path_str(), &mut st.os_hdr)
    } else {
        st.os_hdr = None;
        0
    };
    let Some(os_hdr) = st.os_hdr else {
        st.str_cache.clear();
        st.app_hdr = None;
        st.app_ino = 0;
        st.base_font = None;
        st.extra_font_hdr = None;
        gfx::text::set_ext_font_hooks(None, None);
        gfx::text::set_localize_hook(None);
        return;
    };
    st.base_font_ino = load_font_stem(storage, b"tiny5", &mut st.base_font);
    st.extra_font_ino = load_font_stem(storage, &os_hdr.lang_code, &mut st.extra_font_hdr);
    if st.extra_font_hdr.is_none() {
        let stem: &[u8] = if (os_hdr.font_hint & LANG_FONT_CJK_JA) != 0 {
            b"cjk_ja"
        } else if (os_hdr.font_hint & LANG_FONT_CJK_ZH) != 0 {
            b"cjk_zh"
        } else {
            b""
        };
        st.extra_font_ino = load_font_stem(storage, stem, &mut st.extra_font_hdr);
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
            st.app_ino = load_lang_hdr(storage, p, &mut st.app_hdr);
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
#[inline(never)]
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
    for (hdr_opt, ino) in [(st.app_hdr, st.app_ino), (st.os_hdr, st.os_ino)] {
        if let Some(hdr) = hdr_opt
            && ino >= 2
        {
            found = lookup_lang_slot(storage, &hdr, ino, primary_hash, fallback_hash, out);
            if found.is_some() {
                break;
            }
        }
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
    let mut any_key = false;
    for i in 0..11usize {
        let kb = lang_km_key_bytes(i.saturating_sub(1) as u8);
        let key: &[u8] = if i == 0 {
            LANG_KEY_KM_LABEL.as_bytes()
        } else {
            &kb
        };
        let res = lookup_lang_slot(storage, &hdr, ino, fnv1a_32(key), None, &mut buf);
        if i == 0 {
            let Some(lbl) = res else { return };
            km.set_label(lbl);
        } else if let Some(seq) = res {
            km.set_key(i - 1, seq);
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
    fn associative_lang_str_cache_avoids_conflicts_and_evicts_ring() {
        let mut cache = LangStrCache::<4>::new();
        let mut out = [0u8; 32];
        assert!(cache.lookup(0x1001, &mut out).is_none());

        // Same low bits (`0x1001` and `0x2001`) must coexist without conflict eviction
        cache.insert(0x1001, Some("Settings"));
        cache.insert(0x2001, Some("Radio"));
        cache.insert(0x1002, None);
        cache.insert(0x1003, Some("Audio"));
        assert_eq!(cache.lookup(0x1001, &mut out), Some(Some("Settings")));
        assert_eq!(cache.lookup(0x2001, &mut out), Some(Some("Radio")));
        assert_eq!(cache.lookup(0x1002, &mut out), Some(None));
        assert_eq!(cache.lookup(0x1003, &mut out), Some(Some("Audio")));

        // 5th insert evicts oldest ring slot (`0x1001`)
        cache.insert(0x1004, Some("Display"));
        assert!(cache.lookup(0x1001, &mut out).is_none());
        assert_eq!(cache.lookup(0x1004, &mut out), Some(Some("Display")));

        cache.clear();
        assert!(cache.lookup(0x1004, &mut out).is_none());
    }
}
