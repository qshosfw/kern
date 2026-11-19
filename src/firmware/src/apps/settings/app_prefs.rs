//! Declarative KRX Application Preferences Engine (`SECTION_CONFIG = 0x0007`).
//!
//! Renders native `Toggle`, `Choice`, and `Range` rows directly from a package's
//! KRX header without hardcoded per-app Rust UI code or static `.bss` storage.

use crate::SysContext;
#[cfg(feature = "filesystem")]
use crate::apps::launcher::catalog::get_catalog;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, SheetOptions};
use kernel::proc::krx::{CFG_KIND_CHOICE, CFG_KIND_RANGE, CFG_KIND_TOGGLE, KrxPreamble};

#[repr(C)]
struct AppPrefsScratch {
    pkg_idx: u8,
    cfg_len: u8,
    hdr_len: u16,
    sec_off: u16,
    sec_len: u16,
    cfg: [u8; 64],
    hdr: [u8; KrxPreamble::MAX_HEADER_SIZE],
}

const SUMMARY_OFFSET: usize = (core::mem::size_of::<AppPrefsScratch>() + 7) & !7;
const _: () = assert!(
    SUMMARY_OFFSET + core::mem::size_of::<kernel::proc::krx::KrxSummary<'static>>()
        <= crate::data::arena::TRANSIENT_ARENA_BYTES - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
);

#[inline(always)]
fn scratch() -> &'static mut AppPrefsScratch {
    unsafe {
        &mut *(crate::data::arena::scratch_tail_bytes_mut().as_mut_ptr() as *mut AppPrefsScratch)
    }
}

#[inline(never)]
fn unpack_item(raw: &[u8]) -> kernel::proc::krx::KrxConfigItem {
    unsafe { core::ptr::read_unaligned(raw.as_ptr() as *const kernel::proc::krx::KrxConfigItem) }
}

#[inline(never)]
fn read_cfg_it(buf: &[u8], it: &kernel::proc::krx::KrxConfigItem) -> i16 {
    let off = it.offset as usize;
    let raw = if off < buf.len() {
        if it.min_v < 0 {
            (buf[off] as i8) as i16
        } else {
            buf[off] as i16
        }
    } else {
        it.default_v
    };
    raw.clamp(it.min_v, it.max_v)
}

#[inline(never)]
fn write_cfg_it(buf: &mut [u8], it: &kernel::proc::krx::KrxConfigItem, val: i16) {
    let off = it.offset as usize;
    if off < buf.len() {
        buf[off] = val.clamp(it.min_v, it.max_v) as i8 as u8;
    }
}

#[cfg(test)]
pub fn read_cfg_val(buf: &[u8], item: &[u8]) -> i16 {
    read_cfg_it(buf, &unpack_item(item))
}

#[cfg(test)]
pub fn write_cfg_val(buf: &mut [u8], item: &[u8], val: i16) {
    write_cfg_it(buf, &unpack_item(item), val)
}

#[cfg(test)]
#[inline(never)]
pub fn seed_defaults(cfg: &mut [u8], sec: &[u8], ver: u8) {
    cfg.fill(0);
    if !cfg.is_empty() {
        cfg[0] = ver;
    }
    if sec.len() < 16 {
        return;
    }
    let count = sec[2] as usize;
    for i in 0..count {
        let off = (i + 1) << 4;
        if off + 16 > sec.len() {
            break;
        }
        let it = unpack_item(&sec[off..off + 16]);
        write_cfg_it(cfg, &it, it.default_v);
    }
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn build_app_pref_item(dst: Option<&mut MenuItem>, off: usize, loaded: bool) -> bool {
    let sc = scratch();
    let sec_off = sc.sec_off as usize;
    let cfg_len = sc.cfg_len as usize;
    let it = unpack_item(&sc.hdr[sec_off + off..sec_off + off + 16]);
    if !loaded {
        write_cfg_it(&mut sc.cfg[..cfg_len], &it, it.default_v);
    }
    let Some(dst) = dst else {
        return false;
    };
    let hdr_slice = &sc.hdr[..sc.hdr_len as usize];
    let cur = read_cfg_it(&sc.cfg[..cfg_len], &it);
    dst.item_type = match it.kind {
        CFG_KIND_TOGGLE => MenuItemType::Toggle(cur != 0),
        CFG_KIND_CHOICE => MenuItemType::Choice {
            selected: cur.max(0) as u16,
            options: SheetOptions::Packed(
                KrxPreamble::read_hdr_packed(hdr_slice, it.aux_off).unwrap_or("Off\0"),
            ),
        },
        CFG_KIND_RANGE => MenuItemType::Range {
            val: cur,
            min: it.min_v,
            max: it.max_v,
            step: it.step.max(1) as i16,
            unit: KrxPreamble::read_hdr_str(hdr_slice, it.aux_off).unwrap_or(""),
        },
        _ => return false,
    };
    dst.title = KrxPreamble::read_hdr_str(hdr_slice, it.label_off).unwrap_or("Setting");
    true
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn populate_app_prefs(items: &mut [MenuItem], pkg_idx: usize, ctx: &mut SysContext) -> usize {
    let Some(pkg) = get_catalog().get_entry(false, pkg_idx) else {
        return 0;
    };
    let sc = scratch();
    sc.pkg_idx = pkg_idx as u8;
    sc.cfg_len = 0;
    let hdr_base = sc.hdr.as_ptr() as usize;

    // Place `KrxSummary` immediately after `AppPrefsScratch` in the scratch tail so the two
    // never alias while keeping `populate_app_prefs` off the main runtime stack (-180 B peak stack).
    let summary_slot = unsafe {
        &mut *(crate::data::arena::scratch_tail_bytes_mut()
            .as_mut_ptr()
            .add(SUMMARY_OFFSET)
            as *mut core::mem::MaybeUninit<kernel::proc::krx::KrxSummary<'_>>)
    };
    let Ok(summary) = crate::runtime::loader::read_krx_summary(
        ctx.storage,
        pkg.path.as_str(),
        &mut sc.hdr,
        summary_slot,
    ) else {
        return 0;
    };
    let Some(sec) = summary.aux.config else {
        return 0;
    };
    let sec_len = sec.len();
    if sec_len < 16 {
        return 0;
    }
    let ver = sec[0];
    let item_count = sec[2] as usize;
    let cfg_len = (sec[3] as usize).clamp(1, sc.cfg.len());
    sc.hdr_len = (summary.preamble.header_len as usize).min(KrxPreamble::MAX_HEADER_SIZE) as u16;
    sc.sec_off = (sec.as_ptr() as usize - hdr_base) as u16;
    sc.sec_len = sec_len as u16;
    sc.cfg_len = cfg_len as u8;

    let loaded = ctx
        .load_config(pkg.title.as_str(), &mut sc.cfg[..cfg_len])
        .is_ok_and(|n| n >= cfg_len && sc.cfg[0] == ver);
    if !loaded {
        sc.cfg[..cfg_len].fill(0);
        sc.cfg[0] = ver;
    }

    let mut n = 0usize;
    for i in 0..item_count {
        let off = (i + 1) << 4;
        if off + 16 > sc.sec_len as usize {
            break;
        }
        if build_app_pref_item(items.get_mut(n), off, loaded) {
            n += 1;
        }
    }
    n
}

#[cfg(not(feature = "filesystem"))]
pub fn populate_app_prefs(
    _items: &mut [MenuItem],
    _pkg_idx: usize,
    _ctx: &mut SysContext,
) -> usize {
    0
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn commit_app_pref(idx: usize, val: i32, ctx: &mut SysContext) {
    let sc = scratch();
    let off = (idx + 1) << 4;
    let cfg_len = sc.cfg_len as usize;
    let sec_off = sc.sec_off as usize;
    if cfg_len == 0 || off + 16 > sc.sec_len as usize {
        return;
    }
    let Some(pkg) = get_catalog().get_entry(false, sc.pkg_idx as usize) else {
        return;
    };
    let it = unpack_item(&sc.hdr[sec_off + off..sec_off + off + 16]);
    write_cfg_it(&mut sc.cfg[..cfg_len], &it, val as i16);
    let _ = ctx.save_config(pkg.title.as_str(), &sc.cfg[..cfg_len]);
}

#[cfg(not(feature = "filesystem"))]
pub fn commit_app_pref(_idx: usize, _val: i32, _ctx: &mut SysContext) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_seed_clamp_and_roundtrip() {
        let mut body = [0u8; 48];
        body[0..2].copy_from_slice(&2u16.to_le_bytes()); // version 2
        body[2] = 2; // 2 items
        body[3] = 16; // cfg_bytes
        // Item 0: toggle at offset 1, default=1
        body[16] = 1;
        body[17] = CFG_KIND_TOGGLE;
        body[20..22].copy_from_slice(&1i16.to_le_bytes());
        body[24..26].copy_from_slice(&1i16.to_le_bytes());
        // Item 1: signed range at offset 2, [-50, 50], default 20
        body[32] = 2;
        body[33] = CFG_KIND_RANGE;
        body[36..38].copy_from_slice(&20i16.to_le_bytes());
        body[38..40].copy_from_slice(&(-50i16).to_le_bytes());
        body[40..42].copy_from_slice(&50i16.to_le_bytes());

        let mut cfg = [0xFFu8; 16];
        seed_defaults(&mut cfg, &body, 2);
        assert_eq!(cfg[0], 2);
        assert_eq!(cfg[1], 1);
        assert_eq!(cfg[2], 20);

        let item1 = &body[32..48];
        write_cfg_val(&mut cfg, item1, -999);
        assert_eq!(read_cfg_val(&cfg, item1), -50);
    }
}
