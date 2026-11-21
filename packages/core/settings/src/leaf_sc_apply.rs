//! P0 Leaf 6: Apply Shortcut Binding / F-Drawer Toggle & Save `/pref/keys.cfg` + CRC32.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{
    CMD_SC_APPLY_PICK, CMD_SC_LOAD, CMD_SC_TOGGLE_DRAWER, MAX_APPS, SharedState, byte_at, set_byte,
    subslice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    if st.cmd == CMD_SC_LOAD {
        let _ = api.read_file(b"/pref/keys.cfg", &mut st.sc_mut().keys_cfg);
        st.app_count = api.list_dir(b"/apps", st.apps_mut()).min(MAX_APPS) as u8;
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
        if pick < (st.sc_target_count as usize).min(12) {
            let target = unsafe { *st.sc().target_ids.get_unchecked(pick) };
            let slot = st.sc_slot;
            let scope = st.sc_scope;
            let b = target.to_le_bytes();
            let cfg = &mut st.sc_mut().keys_cfg;
            if scope == 0 {
                let o = 10 + (slot as usize) * 2;
                set_byte(cfg, o, b[0]);
                set_byte(cfg, o + 1, b[1]);
            } else {
                let mut idx = 0usize;
                while idx < 3 {
                    let o = 48 + idx * 4;
                    if (byte_at(cfg, o) == slot && byte_at(cfg, o + 1) == scope)
                        || byte_at(cfg, o + 1) == 0
                    {
                        set_byte(cfg, o, slot);
                        set_byte(cfg, o + 1, if target == 0 { 0 } else { scope });
                        set_byte(cfg, o + 2, b[0]);
                        set_byte(cfg, o + 3, b[1]);
                        break;
                    }
                    idx += 1;
                }
            }
            changed = true;
        }
    }
    if changed {
        let cfg = &mut st.sc_mut().keys_cfg;
        let crc = api.crc32(subslice(cfg, 0, 60)).to_le_bytes();
        set_byte(cfg, 60, crc[0]);
        set_byte(cfg, 61, crc[1]);
        set_byte(cfg, 62, crc[2]);
        set_byte(cfg, 63, crc[3]);
        if api.write_file(b"/pref/keys.cfg", cfg) {
            let _ = api.reload_config();
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
