//! Kern OS Public Rust Package SDK (`kern-sdk`).
//!
//! Provides zero-heap `#![no_std]` types, position-independent string table helpers,
//! entry-point macros, and inline syscall wrappers over [`abi`] for authoring
//! relocatable `.krx` applications, services, and system modules.
#![no_std]

pub use abi::{
    ALIGN_CENTER, ALIGN_LEFT, ALIGN_RIGHT, COLOR_CLEAR, COLOR_DARK_GRAY, COLOR_FILL, COLOR_INVERT,
    COLOR_LIGHT_GRAY, Cap, CapabilitySet, FONT_MUMATRIX, FONT_TOMTHUMB, INTERFACE_SVC_AX25,
    INTERFACE_SVC_CW, INTERFACE_SVC_DATA, INTERFACE_SVC_DTMF, INTERFACE_SVC_SPECTRUM,
    INTERFACE_SVC_TRIGGER, KRX_ABI_VERSION, KRX_CUSTOM_TRAY_ACTIVE, KRX_CUSTOM_TRAY_PERSIST,
    KRX_EV_BUS, KRX_EV_ENCODER, KRX_EV_IPC, KRX_EV_KEY, KRX_EV_MODAL, KRX_EV_NONE, KRX_EV_POWER,
    KRX_EV_TICK, KRX_EV_USB, KRX_ICON_CUSTOM_0, KRX_ICON_CUSTOM_1, KRX_ICON_NONE,
    KRX_IPC_ACTION_FLAG, KRX_OP_EVENT, KRX_OP_EXIT, KRX_OP_RENDER, KRX_OP_START,
    KRX_PROC_FLAG_FOREGROUND, KRX_PROC_FLAG_HAS_LEASE, KRX_PROC_FLAG_KILLABLE,
    KRX_PROC_FLAG_PERSISTENT, KRX_PROC_FLAG_SUSPENDED, KRX_QS1_EMPTY, KRX_QS1_INVALID,
    KRX_QS1_NONE, KRX_QS1_UNVERIFIED, KRX_QS1_VERIFIED, KRX_STATUS_CUSTOM_TITLE,
    KRX_STATUS_DIVIDER, KRX_STATUS_HAS_SIGNAL, KRX_STATUS_PARTIAL, KRX_STATUS_VISIBLE,
    KRX_SYS_INFO_FLAG_CALIB_VALID, KRX_SYS_INFO_FLAG_HAS_CERT, KRX_SYS_INFO_FLAG_RDP_PROTECTED,
    KRX_WIDGET_BATTERY, KRX_WIDGET_BIG_DIGIT, KRX_WIDGET_CAPSULE_BUTTON, KRX_WIDGET_KEY_BADGE,
    KRX_WIDGET_MENU_ITEM, KRX_WIDGET_MODAL_BOX, KRX_WIDGET_NEGATED_HEADER,
    KRX_WIDGET_PROGRESS_BAR, KRX_WIDGET_ROUNDED_BOX, KRX_WIDGET_S_METER, KRX_WIDGET_SCROLLBAR,
    KRX_WIDGET_SHEET_FRAME, KRX_WIDGET_SIGNAL, KRX_WIDGET_SMALL_DIGIT, KRX_WIDGET_TAG_BADGE,
    KernCallFn, KrxApiTable, KrxBusReq, KrxDirEntry, KrxEvent, KrxFsReq, KrxIpcSendReq,
    KrxModalReq, KrxNativeEntryFn, KrxProcInfo, KrxRadioState, KrxStatusBar, KrxSweepReq,
    KrxSymbol, KrxSysInfo, KrxTextReq, KrxWidget, MODAL_RESULT_CANCELED, MODAL_RESULT_CONFIRMED,
    MODAL_RESULT_DISMISSED, MODAL_RESULT_SHEET_SELECTED, MODAL_RESULT_TEXT_DONE, Op,
    PROC_CTL_FOCUS_APP, PROC_CTL_IPC_ENTRY, PROC_CTL_KILL, PROC_CTL_LEASE_ENTRY,
    PROC_CTL_PROC_EXT, PROC_CTL_RESET_STATS, PROC_CTL_REVOKE_LEASE, PROC_CTL_SET_PRIORITY,
    PROC_CTL_SET_STATE, PROC_CTL_SVC_ENTRY, PROC_CTL_SYS_OVERVIEW, VERSION, domain, icon, idx,
    pack_xy, prop, scope, sub, types,
};

/// Divisionless `/ 10` and `% 10` for `0..=255` on ARM Cortex-M0+ (`__aeabi_uidiv`-free).
#[inline(always)]
pub const fn div_rem_10(val: u8) -> (u8, u8) {
    let q = ((val as u16 * 205) >> 11) as u8;
    let r = val - q * 10;
    (q, r)
}

// ── Position-Independent Relative-Offset String Table ────────────────────────

/// Zero-relocation static string table for PIC `.krx` packages.
/// Stores byte offsets and lengths instead of pointer slices so LLVM never emits
/// absolute data relocations in `.rodata`.
#[repr(C)]
pub struct RelStrTable<const N: usize, const BYTES: usize> {
    pub offsets: [u16; N],
    pub lens: [u8; N],
    pub blob: [u8; BYTES],
}

impl<const N: usize, const BYTES: usize> RelStrTable<N, BYTES> {
    #[inline(always)]
    pub fn get(&self, idx: usize) -> &[u8] {
        if idx >= N {
            return &[];
        }
        let off = self.offsets[idx] as usize;
        let len = self.lens[idx] as usize;
        let end = off + len;
        if end <= BYTES {
            &self.blob[off..end]
        } else {
            &[]
        }
    }
}

/// Defines a compile-time `RelStrTable` from byte-string literals with zero pointer relocations.
/// Automatically computes both the entry count and total byte length at compile time.
#[macro_export]
macro_rules! rel_strings {
    ($vis:vis static $name:ident $(: [$ _count:expr; $ _total:expr])? = [ $($lit:expr),+ $(,)? ];) => {
        $vis static $name: $crate::RelStrTable<
            { [$($lit as &[u8]),+].len() },
            { 0usize $(+ ($lit as &[u8]).len())+ }
        > = {
            const COUNT: usize = [$($lit as &[u8]),+].len();
            const TOTAL: usize = 0usize $(+ ($lit as &[u8]).len())+;
            let parts: [&[u8]; COUNT] = [ $($lit as &[u8]),+ ];
            let mut offsets = [0u16; COUNT];
            let mut lens = [0u8; COUNT];
            let mut blob = [0u8; TOTAL];
            let mut cur = 0usize;
            let mut i = 0usize;
            while i < COUNT {
                let s = parts[i];
                offsets[i] = cur as u16;
                lens[i] = s.len() as u8;
                let mut j = 0usize;
                while j < s.len() {
                    blob[cur + j] = s[j];
                    j += 1;
                }
                cur += s.len();
                i += 1;
            }
            $crate::RelStrTable { offsets, lens, blob }
        };
    };
}

/// Declares the position-independent `.text.entry` trampoline at offset 0 for a Rust KRX unit.
#[macro_export]
macro_rules! krx_entry {
    ($handler:path) => {
        #[unsafe(link_section = ".text.entry")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn _krx_entry(
            op: u32,
            api: &$crate::KrxApiTable,
            arg: *const (),
        ) -> u32 {
            $handler(op, api, arg)
        }

        #[cfg(all(not(test), target_os = "none"))]
        #[panic_handler]
        fn _krx_panic(_info: &core::panic::PanicInfo) -> ! {
            loop {
                core::hint::spin_loop();
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    rel_strings!(static DEMO_LABELS: [3; 17] = [b"Display", b"Audio", b"System"];);

    #[test]
    fn rel_str_table_indexes_without_pointer_relocs() {
        assert_eq!(DEMO_LABELS.get(0), b"Display");
        assert_eq!(DEMO_LABELS.get(1), b"Audio");
        assert_eq!(DEMO_LABELS.get(2), b"System");
        assert_eq!(DEMO_LABELS.get(3), b"");
        assert_eq!(core::mem::size_of::<KrxStatusBar>(), 64);
    }
}
