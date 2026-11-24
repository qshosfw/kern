//! P0 Leaf 15: System Recovery Confirmations & Passcode Option Toggles / Max Tries.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{
    CMD_SYS_DO_RESET, CMD_SYS_LAUNCH_MOD, CMD_SYS_REQ_REBOOT, CMD_SYS_REQ_RESET,
    CMD_SYS_SEC_TOGGLE, CMD_SYS_SEC_TRIES_OPEN, CMD_SYS_SEC_TRIES_PICK, MODAL_REBOOT_RADIO,
    MODAL_RESET_SETTINGS, MODAL_SEC_TRIES, SharedState, copy_bytes, subslice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    match st.cmd {
        CMD_SYS_REQ_RESET => {
            api.show_confirm(
                MODAL_RESET_SETTINGS,
                b"Reset Settings?",
                b"Restore defaults?",
                b"Hold",
                b"Cancel",
                1200,
            );
        }
        CMD_SYS_REQ_REBOOT => {
            api.show_confirm(
                MODAL_REBOOT_RADIO,
                b"Restart Radio?",
                b"Save and restart?",
                b"Hold",
                b"Cancel",
                800,
            );
        }
        CMD_SYS_DO_RESET => {
            let ok = api.reset_all_prefs();
            api.show_toast(if ok { b"Defaults Restored" } else { b"Reset Failed" }, 1200);
        }
        CMD_SYS_SEC_TOGGLE => {
            let flags = ((st.sec_status as u8) & 0x0E) ^ (1u8 << st.arg0);
            let tries = ((st.sec_status >> 16) & 0xFF) as u8;
            let _ = api.sec_set_opts(flags, tries);
            st.sec_status = api.sec_status();
        }
        CMD_SYS_SEC_TRIES_OPEN => {
            let tries = ((st.sec_status >> 16) & 0xFF) as u8;
            let sel = if tries <= 3 { 0 } else if tries <= 5 { 1 } else { 2 };
            let tlen = copy_bytes(&mut st.sheet_title, 0, b"Max Tries");
            let olen = copy_bytes(&mut st.sheet_buf, 0, b"3\05\010\0");
            api.show_sheet(
                MODAL_SEC_TRIES,
                subslice(&st.sheet_title, 0, tlen),
                subslice(&st.sheet_buf, 0, olen),
                sel,
                true,
            );
        }
        CMD_SYS_SEC_TRIES_PICK => {
            let tries = match st.arg0 {
                0 => 3u8,
                1 => 5u8,
                _ => 10u8,
            };
            let _ = api.sec_set_opts((st.sec_status as u8) & 0x0E, tries);
            st.sec_status = api.sec_status();
        }
        CMD_SYS_LAUNCH_MOD => {
            let path: &[u8] = match st.arg0 {
                0 => b"/apps/sysinfo.krx",
                1 => b"/apps/storage.mod",
                2 => b"/apps/calib.mod",
                _ => b"/apps/dfu.mod",
            };
            let ok = api.launch_pkg(path);
            st.result = u8::from(ok);
            if !ok && st.arg0 != 0 {
                api.show_toast(b"Not Installed", 1200);
            }
        }
        _ => {}
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
