//! Declarative KRX Application Preferences Engine (`SECTION_CONFIG = 0x0007`).
//!
//! Renders native `Toggle`, `Choice`, and `Range` rows directly from a package's
//! KRX header without hardcoded per-app Rust UI code or static `.bss` storage.

use crate::SysContext;
#[cfg(feature = "filesystem")]
use crate::apps::launcher::catalog::get_catalog;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, SheetOptions};
use kernel::proc::krx::{CFG_KIND_CHOICE, CFG_KIND_RANGE, CFG_KIND_TOGGLE, KrxPreamble};
#[cfg(feature = "filesystem")]
use kernel::proc::primitives::StoragePrimitive;

#[repr(C)]
struct AppPrefsScratch {
    hdr: [u8; KrxPreamble::MAX_HEADER_SIZE],
    cfg: [u8; 64],
    hdr_len: usize,
    sec_off: usize,
    sec_len: usize,
    cfg_len: usize,
    pkg_name: [u8; 16],
    pkg_name_len: u8,
}

const _: () = assert!(
    core::mem::size_of::<AppPrefsScratch>()
        <= crate::data::arena::TRANSIENT_ARENA_BYTES - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
);

#[inline(always)]
fn scratch() -> &'static mut AppPrefsScratch {
    unsafe {
        &mut *(crate::data::arena::scratch_tail_bytes_mut().as_mut_ptr() as *mut AppPrefsScratch)
    }
}

#[inline(always)]
fn le16(s: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([s[off], s[off + 1]])
}

#[inline(never)]
pub fn read_cfg_val(buf: &[u8], item: &[u8]) -> i16 {
    let off = item[0] as usize;
    let min_v = le16(item, 6);
    let max_v = le16(item, 8);
    let raw = if off < buf.len() {
        if min_v < 0 {
            (buf[off] as i8) as i16
        } else {
            buf[off] as i16
        }
    } else {
        le16(item, 4)
    };
    raw.clamp(min_v, max_v)
}

#[inline(never)]
pub fn write_cfg_val(buf: &mut [u8], item: &[u8], val: i16) {
    let off = item[0] as usize;
    if off < buf.len() {
        buf[off] = val.clamp(le16(item, 6), le16(item, 8)) as i8 as u8;
    }
}

#[inline(never)]
pub fn seed_defaults(cfg: &mut [u8], sec: &[u8], ver: u8) {
    for b in cfg.iter_mut() {
        *b = 0;
    }
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
        let item = &sec[off..off + 16];
        write_cfg_val(cfg, item, le16(item, 4));
    }
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn populate_app_prefs(items: &mut [MenuItem], pkg_idx: usize, ctx: &mut SysContext) -> usize {
    let Some(pkg) = get_catalog().get_entry(false, pkg_idx) else {
        return 0;
    };
    let mut path_buf = [0u8; 24];
    let plen = pkg.path.len().min(24);
    path_buf[..plen].copy_from_slice(&pkg.path.as_bytes()[..plen]);
    let tlen = pkg.title.len().min(16);
    let mut title_buf = [0u8; 16];
    title_buf[..tlen].copy_from_slice(&pkg.title.as_bytes()[..tlen]);

    let sc = scratch();
    sc.pkg_name = title_buf;
    sc.pkg_name_len = tlen as u8;
    sc.cfg_len = 0;
    let hdr_base = sc.hdr.as_ptr() as usize;

    let path = utils::format::ascii_str(&path_buf[..plen]).unwrap_or("");
    let mut summary_uninit = core::mem::MaybeUninit::uninit();
    let Ok(summary) =
        crate::runtime::loader::read_krx_summary(ctx.storage, path, &mut sc.hdr, &mut summary_uninit)
    else {
        return 0;
    };
    let Some(sec) = summary.aux.config else {
        return 0;
    };
    let sec_len = sec.len();
    if sec_len < 16 {
        return 0;
    }
    let sec_off = sec.as_ptr() as usize - hdr_base;
    let hdr_len = (summary.preamble.header_len as usize).min(KrxPreamble::MAX_HEADER_SIZE);
    let ver = sec[0];
    let item_count = sec[2] as usize;
    let cfg_len = (sec[3] as usize).clamp(1, sc.cfg.len());
    sc.hdr_len = hdr_len;
    sc.sec_off = sec_off;
    sc.sec_len = sec_len;
    sc.cfg_len = cfg_len;

    let name = utils::format::ascii_str(&sc.pkg_name[..tlen]).unwrap_or("");
    let loaded = StoragePrimitive::load_config(ctx, name, &mut sc.cfg[..cfg_len])
        .is_ok_and(|n| n >= cfg_len && sc.cfg[0] == ver);
    if !loaded {
        seed_defaults(&mut sc.cfg[..cfg_len], &sc.hdr[sec_off..sec_off + sec_len], ver);
    }

    let hdr_slice = &sc.hdr[..hdr_len];
    let sec_slice = &sc.hdr[sec_off..sec_off + sec_len];
    let count = item_count.min(items.len());
    let mut n = 0usize;
    for i in 0..count {
        let off = (i + 1) << 4;
        if off + 16 > sec_slice.len() {
            break;
        }
        let item = &sec_slice[off..off + 16];
        let l_off = le16(item, 10) as u16;
        let a_off = le16(item, 12) as u16;
        let title = KrxPreamble::read_hdr_str(hdr_slice, l_off).unwrap_or("Setting");
        let cur = read_cfg_val(&sc.cfg[..cfg_len], item);
        let ty = match item[1] {
            CFG_KIND_TOGGLE => MenuItemType::Toggle(cur != 0),
            CFG_KIND_CHOICE => {
                let opts = KrxPreamble::read_hdr_packed(hdr_slice, a_off).unwrap_or("Off\0");
                MenuItemType::Choice {
                    selected: (cur.max(0)) as u16,
                    options: SheetOptions::Packed(opts),
                }
            }
            CFG_KIND_RANGE => MenuItemType::Range {
                val: cur,
                min: le16(item, 6),
                max: le16(item, 8),
                step: (item[3].max(1)) as i16,
                unit: KrxPreamble::read_hdr_str(hdr_slice, a_off).unwrap_or(""),
            },
            _ => continue,
        };
        items[n] = MenuItem::new(title, ty);
        n += 1;
    }
    n
}

#[cfg(not(feature = "filesystem"))]
pub fn populate_app_prefs(_items: &mut [MenuItem], _pkg_idx: usize, _ctx: &mut SysContext) -> usize {
    0
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn commit_app_pref(idx: usize, val: i32, ctx: &mut SysContext) {
    let sc = scratch();
    let off = (idx + 1) << 4;
    if sc.cfg_len == 0 || off + 16 > sc.sec_len {
        return;
    }
    let item = &sc.hdr[sc.sec_off + off..sc.sec_off + off + 16];
    write_cfg_val(&mut sc.cfg[..sc.cfg_len], item, val as i16);
    let name = utils::format::ascii_str(&sc.pkg_name[..sc.pkg_name_len as usize]).unwrap_or("");
    let _ = StoragePrimitive::save_config(ctx, name, &sc.cfg[..sc.cfg_len]);
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
