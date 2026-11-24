//! P0 Leaf 6: Shortcuts Load & Apply (`/pref/keys.cfg`).
//! Zero stack buffers: uses `st.sheet_buf` as scratch.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    CMD_SC_APPLY_PICK, CMD_SC_LOAD, CMD_SC_TOGGLE_DRAWER, SharedState, byte_at, copy_bytes,
    cstr_slice, fill_bytes, hash_app_path, is_mod_name, set_byte, strip_apps_prefix, subslice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    if st.cmd == CMD_SC_LOAD {
        let _ = api.read_file(b"/pref/keys.cfg", &mut st.sc_mut().keys_cfg);
        let base = st.sheet_buf.as_mut_ptr();
        let ent = unsafe { &mut *(base as *mut KrxDirEntry) };
        let pbuf = unsafe { core::slice::from_raw_parts_mut(base.add(32), 32) };
        let mut n = 0usize;
        let mut off = 0u32;
        while n < 6 {
            if api.list_dir_page(b"/apps", off, core::slice::from_mut(ent)) == 0 {
                break;
            }
            off += 1;
            let short = strip_apps_prefix(cstr_slice(&ent.name));
            if is_mod_name(short) {
                continue;
            }
            let pl = copy_bytes(pbuf, 0, b"/apps/");
            let pl = copy_bytes(pbuf, pl, short);
            let h = hash_app_path(subslice(pbuf, 0, pl));
            let stem_len = if short.len() > 4 && byte_at(short, short.len() - 4) == b'.' {
                short.len() - 4
            } else {
                short.len()
            };
            let sc = st.sc_mut();
            sc.app_hashes[n] = h;
            fill_bytes(&mut sc.app_names[n], 0);
            copy_bytes(&mut sc.app_names[n], 0, subslice(short, 0, stem_len));
            n += 1;
        }
        st.sc_mut().app_n = n as u8;
        ctx.continuation.kind = KRX_LEAF_KIND_DONE;
        return 0;
    }

    let mut changed = false;
    if st.cmd == CMD_SC_TOGGLE_DRAWER {
        let next = u8::from(byte_at(&st.sc().keys_cfg, 8) == 0);
        set_byte(&mut st.sc_mut().keys_cfg, 8, next);
        changed = true;
    } else if st.cmd == CMD_SC_APPLY_PICK {
        let pick = st.arg0 as usize;
        let target_opt = if pick < 6 {
            Some(pick as u16)
        } else if pick - 6 < (st.sc().app_n as usize).min(6) {
            Some(st.sc().app_hashes[pick - 6])
        } else {
            None
        };
        if let Some(target) = target_opt {
            let slot = st.sc_slot;
            let scope = st.sc_scope;
            let b = target.to_le_bytes();
            let cfg = &mut st.sc_mut().keys_cfg;
            if scope == 0 {
                let o = 10 + (slot as usize) * 2;
                set_byte(cfg, o, b[0]);
                set_byte(cfg, o + 1, b[1]);
                changed = true;
            } else {
                let mut idx = 0usize;
                let mut placed = false;
                while idx < 3 {
                    let o = 48 + idx * 4;
                    if (byte_at(cfg, o) == slot && byte_at(cfg, o + 1) == scope)
                        || byte_at(cfg, o + 1) == 0
                    {
                        set_byte(cfg, o, slot);
                        set_byte(cfg, o + 1, if target == 0 { 0 } else { scope });
                        set_byte(cfg, o + 2, b[0]);
                        set_byte(cfg, o + 3, b[1]);
                        placed = true;
                        break;
                    }
                    idx += 1;
                }
                if placed {
                    changed = true;
                } else {
                    api.show_toast(b"Max 3 rules", 1200);
                }
            }
        }
    }
    if changed {
        let cfg = &mut st.sc_mut().keys_cfg;
        let crc = api.crc32(subslice(cfg, 0, 60)).to_le_bytes();
        copy_bytes(cfg, 60, &crc);
        if api.write_file(b"/pref/keys.cfg", cfg) {
            let _ = api.reload_config();
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
