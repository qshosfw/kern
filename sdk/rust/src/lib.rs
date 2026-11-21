//! Kern OS Public Rust Package SDK (`kern-sdk`).
//!
//! Provides zero-heap `#![no_std]` types, position-independent string table helpers,
//! entry-point macros, and inline syscall wrappers over [`abi`] for authoring
//! relocatable `.krx` applications, services, and system modules.
#![no_std]

pub use abi::{Cap, CapabilitySet, Op, VERSION, domain, icon, idx, prop, scope, sub};

// ── Lifecycle Opcodes & Display Constants ────────────────────────────────────

pub const KRX_OP_START: u32 = 0;
pub const KRX_OP_EVENT: u32 = 1;
pub const KRX_OP_RENDER: u32 = 2;
pub const KRX_OP_EXIT: u32 = 3;

pub const COLOR_CLEAR: u8 = 0;
pub const COLOR_FILL: u8 = 1;
pub const COLOR_INVERT: u8 = 2;
pub const COLOR_DARK_GRAY: u8 = 3;
pub const COLOR_LIGHT_GRAY: u8 = 4;

pub const FONT_TOMTHUMB: u8 = 0;
pub const FONT_MUMATRIX: u8 = 1;

pub const ALIGN_LEFT: i16 = 0;
pub const ALIGN_CENTER: i16 = 1;
pub const ALIGN_RIGHT: i16 = 2;

pub const KRX_EV_NONE: u8 = 0;
pub const KRX_EV_KEY: u8 = 1;
pub const KRX_EV_ENCODER: u8 = 2;
pub const KRX_EV_TICK: u8 = 3;
pub const KRX_EV_BUS: u8 = 4;
pub const KRX_EV_USB: u8 = 5;
pub const KRX_EV_POWER: u8 = 6;
pub const KRX_EV_MODAL: u8 = 7;
pub const KRX_EV_IPC: u8 = 8;

// ── Single-Entry System Call Table ───────────────────────────────────────────

pub type KernCallFn = unsafe extern "C" fn(op: u32, a0: usize, a1: usize, a2: usize) -> usize;

#[repr(C)]
pub struct KrxApiTable {
    pub abi_version: u32,
    pub table_size: u16,
    pub _pad0: u16,
    pub granted_caps: u32,
    pub call: KernCallFn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxEvent {
    pub kind: u8,
    pub key: u8,
    pub flags: u8,
    pub reserved: u8,
    pub payload: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxTextReq {
    pub text: *const u8,
    pub x: i16,
    pub y: i16,
    pub len: u16,
    pub arg: i16,
    pub font: u8,
    pub color: u8,
    pub flags: u8,
    pub _pad: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxFsReq {
    pub path: *const u8,
    pub buf: *mut u8,
    pub offset: u32,
    pub len: u16,
    pub path_len: u8,
    pub extra: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxModalReq {
    pub title: *const u8,
    pub msg: *const u8,
    pub ok: *const u8,
    pub cancel: *const u8,
    pub tag: u8,
    pub kind: u8,
    pub t_len: u8,
    pub m_len: u8,
    pub o_len: u8,
    pub c_len: u8,
    pub max_len: u8,
    pub _pad: u8,
}

#[repr(C, align(4))]
#[derive(Debug, Clone, Copy)]
pub struct KrxStatusBar {
    pub flags: u8,
    pub signal_level: u8,
    pub left_icon_count: u8,
    pub center_icon_count: u8,
    pub left_icons: [u8; 4],
    pub center_icons: [u8; 4],
    pub title: [u8; 16],
    pub badge: [u8; 8],
    pub center: [u8; 16],
    pub custom_icon_0: [u8; 5],
    pub custom_icon_1: [u8; 5],
    pub tray_icon_id: u8,
    pub custom_flags: u8,
}

#[inline(always)]
pub const fn pack_xy(x: i16, y: i16) -> usize {
    ((x as u16 as u32) | ((y as u16 as u32) << 16)) as usize
}

/// Divisionless `/ 10` and `% 10` for `0..=255` on ARM Cortex-M0+ (`__aeabi_uidiv`-free).
#[inline(always)]
pub const fn div_rem_10(val: u8) -> (u8, u8) {
    let q = ((val as u16 * 205) >> 11) as u8;
    let r = val - q * 10;
    (q, r)
}

impl KrxApiTable {
    #[inline(always)]
    pub fn invoke(&self, op: Op, a0: usize, a1: usize, a2: usize) -> usize {
        unsafe { (self.call)(op.u32_id(), a0, a1, a2) }
    }

    #[inline(always)]
    pub fn prop_get(&self, id: u8) -> u8 {
        self.invoke(Op::PropGet, id as usize, 0, 0) as u8
    }

    #[inline(always)]
    pub fn prop_set(&self, id: u8, val: u8) -> bool {
        self.invoke(Op::PropSet, id as usize, val as usize, 0) == 0
    }

    #[inline(always)]
    pub fn pref_set(&self, id: u8, val: u8) -> bool {
        self.invoke(Op::PrefSet, id as usize, val as usize, 0) == 0
    }

    #[inline(always)]
    pub fn flush(&self) -> bool {
        self.invoke(Op::SysFlush, 0, 0, 0) == 0
    }

    #[inline(always)]
    pub fn clear(&self) {
        let _ = self.invoke(Op::GfxClear, 0, 0, 0);
    }

    #[inline(always)]
    pub fn commit(&self) {
        let _ = self.invoke(Op::GfxCommit, 0, 0, 0);
    }

    #[inline(always)]
    pub fn fill_rect(&self, x: i16, y: i16, w: u8, h: u8, color: u8) {
        let sub_color = (sub::gfx_draw::FILL_RECT as usize) | ((color as usize) << 8);
        let wh = (w as usize) | ((h as usize) << 16);
        let _ = self.invoke(Op::GfxDraw, sub_color, pack_xy(x, y), wh);
    }

    #[inline(always)]
    pub fn draw_rect(&self, x: i16, y: i16, w: u8, h: u8, color: u8) {
        let sub_color = (sub::gfx_draw::RECT as usize) | ((color as usize) << 8);
        let wh = (w as usize) | ((h as usize) << 16);
        let _ = self.invoke(Op::GfxDraw, sub_color, pack_xy(x, y), wh);
    }

    #[inline(always)]
    pub fn draw_icon(&self, x: i16, y: i16, icon_id: u8, color: u8) {
        let sub_color = (sub::gfx_draw::ICON as usize) | ((color as usize) << 8);
        let _ = self.invoke(Op::GfxDraw, sub_color, pack_xy(x, y), icon_id as usize);
    }

    #[inline(always)]
    pub fn draw_text(&self, x: i16, y: i16, text: &[u8], font: u8, color: u8, align: i16) {
        let req = KrxTextReq {
            text: text.as_ptr(),
            x,
            y,
            len: text.len() as u16,
            arg: align,
            font,
            color,
            flags: 0,
            _pad: 0,
        };
        let _ = self.invoke(Op::GfxText, &raw const req as usize, 0, 0);
    }

    #[inline(always)]
    pub fn ticks_ms(&self) -> u32 {
        self.invoke(Op::IoTime, sub::io_time::TICKS_MS as usize, 0, 0) as u32
    }

    #[inline(always)]
    pub fn serial_write(&self, bytes: &[u8]) -> usize {
        self.invoke(
            Op::IoSerial,
            sub::io_serial::WRITE as usize,
            bytes.as_ptr() as usize,
            bytes.len(),
        )
    }
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
