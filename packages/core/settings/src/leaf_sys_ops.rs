//! P0 Leaf 15: System Recovery Confirmations (Reset Settings, Restart Radio).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry, prop};
use shared::{
    CMD_SYS_DO_RESET, CMD_SYS_REQ_REBOOT, CMD_SYS_REQ_RESET, CMD_SYS_TRY_DFU, CMD_SYS_TRY_INFO,
    MODAL_REBOOT_RADIO, MODAL_RESET_SETTINGS, SharedState,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    st.result = 0;
    match st.cmd {
        CMD_SYS_REQ_RESET => {
            api.show_confirm(MODAL_RESET_SETTINGS, b"Reset Settings?", b"Restore defaults?", b"Hold", b"Cancel", 1200);
        }
        CMD_SYS_REQ_REBOOT => {
            api.show_confirm(MODAL_REBOOT_RADIO, b"Restart Radio?", b"Save and restart?", b"Hold", b"Cancel", 800);
        }
        CMD_SYS_TRY_INFO => {
            let mut probe = [0u8; 4];
            if api.read_file_at(b"/apps/sysinfo.krx", 0, &mut probe) == 4 && api.launch_pkg(b"/apps/sysinfo.krx") {
                st.result = 1;
            }
        }
        CMD_SYS_TRY_DFU => {
            let mut probe = [0u8; 4];
            if api.read_file_at(b"/apps/dfu.mod", 0, &mut probe) == 4 {
                let _ = api.launch_pkg(b"/apps/dfu.mod");
            } else {
                api.show_toast(b"Install dfu.mod", 1200);
            }
        }
        CMD_SYS_DO_RESET => {
            const DEFS: [(u8, u8); 12] = [
                (prop::BACKLIGHT_TIME, 2), (prop::BACKLIGHT_MAX, 8), (prop::BACKLIGHT_MIN, 0),
                (prop::CONTRAST, 50), (prop::INVERT_PIXELS, 0), (prop::UI_ANIMATIONS, 1),
                (prop::VOLUME, 5), (prop::KEY_BEEP, 1), (prop::BEEP_GAIN, 5),
                (prop::AUTO_LOCK, 0), (prop::POWER_PROFILE, 1), (prop::CPU_CLOCK, 3),
            ];
            let mut i = 0usize;
            while i < DEFS.len() {
                let (id, val) = unsafe { *DEFS.get_unchecked(i) };
                let _ = api.pref_set(id, val);
                i += 1;
            }
            let _ = api.flush();
            api.show_toast(b"Defaults Restored", 1200);
        }
        _ => {}
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
