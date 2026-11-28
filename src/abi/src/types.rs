//! Canonical KRX C-ABI types, request descriptors, and constants shared across
//! the microkernel, firmware syscall dispatcher, and the Rust/C package SDKs.

use crate::Op;

/// `KrxApiTable::abi_version`: `abi::VERSION` in the major half-word.
pub const KRX_ABI_VERSION: u32 = (crate::VERSION as u32) << 16;

// Standard Service Package Interface IDs (0x0100+ for package services).
pub const INTERFACE_SVC_AX25: u16 = 0x0100;
pub const INTERFACE_SVC_CW: u16 = 0x0101;
pub const INTERFACE_SVC_DTMF: u16 = 0x0102;
pub const INTERFACE_SVC_SPECTRUM: u16 = 0x0103;
pub const INTERFACE_SVC_DATA: u16 = 0x0104;
pub const INTERFACE_SVC_TRIGGER: u16 = 0x0105;

// ── Lifecycle, Display & Event Descriptors ───────────────────────────────────

/// 32-bit Virtual Function Pointer (`P1 v2`) supporting callbacks across evicted pages up to `384 KiB`:
/// `0x8000_0000 | ((page_id as u32) << 10) | ((offset as u32) | 1)`.
/// When bit 31 is `0`, `raw` is a direct resident Thumb-1 address (`N0`/`N1`/`.text.pinned`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Vfp32(pub u32);

impl Vfp32 {
    pub const VIRTUAL_BIT: u32 = 0x8000_0000;

    #[inline(always)]
    pub const fn encode_paged(page_id: u16, offset: u16) -> Self {
        Self(
            Self::VIRTUAL_BIT
                | (((page_id as u32) & 0x1F_FFFF) << 10)
                | (((offset as u32) & 0x03FE) | 1),
        )
    }

    #[inline(always)]
    pub const fn from_resident(thumb_addr: u32) -> Self {
        Self((thumb_addr & !Self::VIRTUAL_BIT) | 1)
    }

    #[inline(always)]
    pub const fn is_virtual(self) -> bool {
        (self.0 & Self::VIRTUAL_BIT) != 0
    }

    #[inline(always)]
    pub const fn page_id(self) -> u16 {
        ((self.0 & !Self::VIRTUAL_BIT) >> 10) as u16
    }

    #[inline(always)]
    pub const fn offset(self) -> u16 {
        (self.0 & 0x03FE) as u16
    }
}

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
pub const KRX_IPC_ACTION_FLAG: u8 = 0xAC;

/// Low byte of `Topic::RADIO` (`1 << 1`).
pub const KRX_BUS_TOPIC_RADIO: u8 = 1 << 1;

pub const MODAL_RESULT_DISMISSED: u8 = 0;
pub const MODAL_RESULT_CONFIRMED: u8 = 1;
pub const MODAL_RESULT_TEXT_DONE: u8 = 2;
pub const MODAL_RESULT_SHEET_SELECTED: u8 = 3;
pub const MODAL_RESULT_CANCELED: u8 = 4;
pub const MODAL_RESULT_NUMBER_DONE: u8 = 5;

pub const KRX_LEAF_KIND_DONE: u8 = 0;
pub const KRX_LEAF_KIND_CALL_LEAF: u8 = 1;
pub const KRX_LEAF_KIND_YIELD: u8 = 2;
pub const KRX_LEAF_KIND_ERROR: u8 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KrxLeafContinuation {
    pub kind: u8,
    pub target_or_code: u8,
    pub reserved: u16,
    pub payload: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KrxLeafContext {
    pub leaf_id: u8,
    pub flags: u8,
    pub reserved: u16,
    pub arg: u32,
    pub continuation: KrxLeafContinuation,
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

impl KrxEvent {
    #[inline(always)]
    pub const fn is_key(&self) -> bool {
        self.kind == KRX_EV_KEY
    }

    #[inline(always)]
    pub const fn is_encoder(&self) -> bool {
        self.kind == KRX_EV_ENCODER
    }

    #[inline(always)]
    pub const fn is_tick(&self) -> bool {
        self.kind == KRX_EV_TICK
    }

    #[inline(always)]
    pub const fn is_bus(&self) -> bool {
        self.kind == KRX_EV_BUS
    }

    #[inline(always)]
    pub const fn is_radio_bus(&self) -> bool {
        self.kind == KRX_EV_BUS && self.flags == KRX_BUS_TOPIC_RADIO
    }

    #[inline(always)]
    pub const fn is_usb(&self) -> bool {
        self.kind == KRX_EV_USB
    }

    #[inline(always)]
    pub const fn is_power(&self) -> bool {
        self.kind == KRX_EV_POWER
    }

    #[inline(always)]
    pub const fn is_modal(&self) -> bool {
        self.kind == KRX_EV_MODAL
    }

    #[inline(always)]
    pub const fn is_ipc(&self) -> bool {
        self.kind == KRX_EV_IPC
    }
}

// ── Single-Entry System Call Table ───────────────────────────────────────────

/// Unified AAPCS-Thumb system call entry point (`r0..r3 -> r0` on Cortex-M0+).
pub type KernCallFn = unsafe extern "C" fn(op: u32, a0: usize, a1: usize, a2: usize) -> usize;

/// Standard native KRX application entry point signature.
pub type KrxNativeEntryFn =
    unsafe extern "C" fn(op: u32, api: *const KrxApiTable, arg: *const ()) -> u32;

/// Compact 16-byte ($2^4$ on ARM32) KRX API handle passed in `r1` to `KrxNativeEntryFn`.
#[repr(C)]
pub struct KrxApiTable {
    pub abi_version: u32,
    pub table_size: u16,
    pub _pad0: u16,
    pub granted_caps: u32,
    pub call: KernCallFn,
}

unsafe impl Sync for KrxApiTable {}
unsafe impl Send for KrxApiTable {}

/// Bitflags in `KrxModalReq::o_len` for `kind == 2` (text / passcode prompt).
pub const TEXT_FLAG_DIGITS_ONLY: u8 = 1 << 0;
pub const TEXT_FLAG_PASSCODE: u8 = 1 << 1;
pub const TEXT_FLAG_EXPOSE_LEN: u8 = 1 << 2;

/// Bitflags packed into `Op::SysSec` (`STATUS` return value and `SET_OPTS` / `SET` options byte).
pub const SEC_FLAG_IS_SET: u32 = 1 << 0;
pub const SEC_FLAG_EXPOSE_LEN: u32 = 1 << 1;
pub const SEC_FLAG_STEALTH: u32 = 1 << 2;
pub const SEC_FLAG_DIGITS_ONLY: u32 = 1 << 3;
pub const SEC_FLAG_AVAILABLE: u32 = 1 << 4;

impl KrxApiTable {
    /// Constructs a live `KrxApiTable` bound to `call` with the specified `granted_caps`.
    #[inline(always)]
    pub const fn with_call(granted_caps: u32, call: KernCallFn) -> Self {
        Self {
            abi_version: KRX_ABI_VERSION,
            table_size: core::mem::size_of::<Self>() as u16,
            _pad0: 0,
            granted_caps,
            call,
        }
    }

    /// Constructs a safe no-op stub table for host unit tests and dry runs.
    pub const fn safe_stub(granted_caps: u32) -> Self {
        Self::with_call(granted_caps, stub_kern_call)
    }

    /// Invokes a typed `Op` through this table's single `kern_call` entry point.
    #[inline(always)]
    pub unsafe fn invoke(&self, op: Op, a0: usize, a1: usize, a2: usize) -> usize {
        unsafe { (self.call)(op.u32_id(), a0, a1, a2) }
    }

    #[inline(always)]
    pub fn call_op(&self, op: Op, a0: usize, a1: usize, a2: usize) -> usize {
        unsafe { (self.call)(op.u32_id(), a0, a1, a2) }
    }

    /// Invokes `op` and decodes the canonical ABI v5 `Status` return (`>= 0` Ok, `< 0` Err).
    #[inline(always)]
    pub fn call_result(
        &self,
        op: Op,
        a0: usize,
        a1: usize,
        a2: usize,
    ) -> Result<usize, crate::Status> {
        crate::Status::from_raw(self.call_op(op, a0, a1, a2))
    }

    #[inline(always)]
    pub fn prop_get32(&self, id: u8) -> u32 {
        self.call_op(
            Op::SysProp,
            (id as usize) | ((crate::sub::sys_prop::GET as usize) << 8),
            0,
            0,
        ) as u32
    }

    #[inline(always)]
    pub fn prop_get(&self, id: u8) -> u8 {
        self.prop_get32(id) as u8
    }

    #[inline(always)]
    pub fn prop_set32(&self, id: u8, val: u32) -> usize {
        self.call_op(
            Op::SysProp,
            (id as usize) | ((crate::sub::sys_prop::SET as usize) << 8),
            val as usize,
            0,
        )
    }

    #[inline(always)]
    pub fn prop_set(&self, id: u8, val: u8) -> bool {
        self.prop_set32(id, val as u32) == 0
    }

    #[inline(always)]
    pub fn pref_set(&self, id: u8, val: u8) -> bool {
        self.call_op(
            Op::SysProp,
            (id as usize) | ((crate::sub::sys_prop::PREF as usize) << 8),
            val as usize,
            0,
        ) == 0
    }

    #[inline(always)]
    pub fn flush(&self) -> bool {
        self.call_op(Op::SysQuery, crate::sub::sys_query::FLUSH as usize, 0, 0) == 0
    }

    #[inline(always)]
    pub fn reset_all_prefs(&self) -> bool {
        self.call_op(Op::SysQuery, crate::sub::sys_query::RESET as usize, 0, 0) == 0
    }

    #[inline(always)]
    pub fn sec_status(&self) -> u32 {
        self.call_op(Op::SysSec, crate::sub::sys_sec::STATUS as usize, 0, 0) as u32
    }

    #[inline(always)]
    pub fn sec_set_opts(&self, flags: u8, max_tries: u8) -> bool {
        let arg = (flags as usize) | ((max_tries as usize) << 8);
        self.call_op(Op::SysSec, crate::sub::sys_sec::SET_OPTS as usize, arg, 0) == 0
    }

    #[inline(always)]
    pub fn sec_verify(&self, pin: &[u8]) -> u8 {
        self.call_op(
            Op::SysSec,
            crate::sub::sys_sec::VERIFY as usize,
            pin.as_ptr() as usize,
            pin.len(),
        ) as u8
    }

    #[inline(always)]
    pub fn sec_set_passcode(&self, pin: &[u8], flags: u8, max_tries: u8) -> bool {
        let meta = (pin.len() & 0xFF) | ((flags as usize) << 16) | ((max_tries as usize) << 24);
        self.call_op(
            Op::SysSec,
            crate::sub::sys_sec::SET as usize,
            pin.as_ptr() as usize,
            meta,
        ) == 0
    }

    #[inline(always)]
    pub fn clear(&self) {
        let _ = self.call_op(Op::GfxDraw, crate::sub::gfx_draw::CLEAR as usize, 0, 0);
    }

    #[inline(always)]
    pub fn framebuffer_ptr(&self) -> *mut u8 {
        self.call_op(Op::GfxDraw, crate::sub::gfx_draw::FB as usize, 0, 0) as *mut u8
    }

    #[inline(always)]
    pub fn commit(&self) {
        let _ = self.call_op(Op::GfxDraw, crate::sub::gfx_draw::COMMIT as usize, 0, 0);
    }

    #[inline(always)]
    pub fn fill_rect(&self, x: i16, y: i16, w: u8, h: u8, color: u8) {
        let sub_color = (crate::sub::gfx_draw::FILL_RECT as usize) | ((color as usize) << 8);
        let wh = (w as usize) | ((h as usize) << 16);
        let _ = self.call_op(Op::GfxDraw, sub_color, pack_xy(x, y), wh);
    }

    #[inline(always)]
    pub fn draw_rect(&self, x: i16, y: i16, w: u8, h: u8, color: u8) {
        let sub_color = (crate::sub::gfx_draw::RECT as usize) | ((color as usize) << 8);
        let wh = (w as usize) | ((h as usize) << 16);
        let _ = self.call_op(Op::GfxDraw, sub_color, pack_xy(x, y), wh);
    }

    #[inline(always)]
    pub fn draw_icon(&self, x: i16, y: i16, icon_id: u8, color: u8) {
        let sub_color = (crate::sub::gfx_draw::ICON as usize) | ((color as usize) << 8);
        let _ = self.call_op(Op::GfxDraw, sub_color, pack_xy(x, y), icon_id as usize);
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
        let _ = self.call_op(Op::GfxText, &raw const req as usize, 0, 0);
    }

    #[inline(always)]
    pub fn ticks_ms(&self) -> u32 {
        self.prop_get32(crate::prop::TICKS_MS)
    }

    #[inline(always)]
    pub fn serial_write(&self, bytes: &[u8]) -> usize {
        self.call_op(
            Op::IoSerial,
            crate::sub::io_serial::WRITE as usize,
            bytes.as_ptr() as usize,
            bytes.len(),
        )
    }

    #[inline(always)]
    pub fn draw_menu_item(&self, y: i16, title: &[u8], value: &[u8], icon_id: u8, flags: u8) {
        let desc = KrxWidget {
            p0: title.as_ptr(),
            p1: value.as_ptr(),
            x: 2,
            y,
            len0: title.len() as u16,
            len1: value.len() as u16,
            extra: 0,
            w: 120,
            h: 13,
            a: flags,
            b: icon_id,
            _pad: [0; 2],
        };
        let _ = self.call_op(
            Op::UiWidget,
            KRX_WIDGET_MENU_ITEM as usize,
            &raw const desc as usize,
            0,
        );
    }

    #[inline(always)]
    pub fn clear_corners(&self, x: i16, y: i16, w: u8, h: u8) {
        let wh = (w as usize) | ((h as usize) << 16);
        let _ = self.call_op(
            Op::GfxDraw,
            crate::sub::gfx_draw::CLR_CORNER as usize,
            pack_xy(x, y),
            wh,
        );
    }

    #[inline(always)]
    pub fn measure_text(&self, text: &[u8], font: u8) -> i16 {
        let req = KrxTextReq {
            text: text.as_ptr(),
            x: 0,
            y: 0,
            len: text.len() as u16,
            arg: 0,
            font,
            color: COLOR_FILL,
            flags: KRX_TEXT_MEASURE,
            _pad: 0,
        };
        self.call_op(Op::GfxText, &raw const req as usize, 0, 0) as u16 as i16
    }

    #[inline(always)]
    pub fn udivmod(&self, n: u32, d: u32) -> u32 {
        self.call_op(Op::SysDiv, n as usize, d as usize, 0) as u32
    }

    #[inline(always)]
    pub fn udivmod_rem(&self, n: u32, d: u32, rem: &mut u32) -> u32 {
        self.call_op(Op::SysDiv, n as usize, d as usize, rem as *mut u32 as usize) as u32
    }

    pub fn draw_scrollbar(&self, x: i16, y: i16, h: u8, scroll: u16, total: u16, visible: u16) {
        if total == 0 || visible == 0 || total <= visible || h < 4 {
            return;
        }
        self.fill_rect(x + 1, y, 1, h, COLOR_FILL);
        let th = self
            .udivmod(
                (h as u32) * (visible as u32) + ((total as u32) >> 1),
                total as u32,
            )
            .clamp(4, h as u32) as u8;
        let max_off = (total - visible) as u32;
        let travel = (h - th) as u32;
        let pos = (scroll as u32).min(max_off);
        let ty = if pos >= max_off {
            travel
        } else {
            self.udivmod(pos * travel + (max_off >> 1), max_off)
        };
        self.fill_rect(x, y + ty as i16, 3, th, COLOR_FILL);
    }

    #[inline(always)]
    pub fn show_toast(&self, msg: &[u8], duration_ms: u16) {
        let len_dur = (msg.len() & 0xFF) | ((duration_ms as usize) << 16);
        let _ = self.call_op(
            Op::UiModal,
            crate::sub::ui_modal::TOAST as usize,
            msg.as_ptr() as usize,
            len_dur,
        );
    }

    #[inline(always)]
    pub fn show_confirm(
        &self,
        tag: u8,
        title: &[u8],
        msg: &[u8],
        ok: &[u8],
        cancel: &[u8],
        hold_ms: u16,
    ) {
        let req = KrxModalReq {
            title: title.as_ptr(),
            msg: msg.as_ptr(),
            ok: ok.as_ptr(),
            cancel: cancel.as_ptr(),
            kind: 1,
            tag,
            max_len: 0,
            t_len: title.len() as u8,
            m_len: msg.len() as u8,
            o_len: ok.len() as u8,
            c_len: cancel.len() as u8,
            _pad: 0,
        };
        let _ = self.call_op(
            Op::UiModal,
            crate::sub::ui_modal::SHOW as usize,
            &raw const req as usize,
            0,
        );
        if hold_ms > 0 {
            let _ = self.prop_set32(crate::prop::MODAL_HOLD_MS, hold_ms as u32);
        }
    }

    #[inline(always)]
    pub fn show_sheet(
        &self,
        tag: u8,
        title: &[u8],
        packed_opts: &[u8],
        selected: u8,
        show_radio: bool,
    ) {
        let len = packed_opts.len();
        let req = KrxModalReq {
            title: title.as_ptr(),
            msg: packed_opts.as_ptr(),
            ok: core::ptr::null(),
            cancel: core::ptr::null(),
            tag,
            kind: 3,
            t_len: title.len().min(255) as u8,
            m_len: (len & 0xFF) as u8,
            o_len: ((len >> 8) & 0xFF) as u8,
            c_len: u8::from(show_radio),
            max_len: selected,
            _pad: 0,
        };
        let _ = self.call_op(
            Op::UiModal,
            crate::sub::ui_modal::SHOW as usize,
            &raw const req as usize,
            0,
        );
    }

    #[inline(always)]
    pub fn prompt_text(&self, tag: u8, title: &[u8], initial: &[u8], max_len: u8) {
        self.prompt_text_flags(tag, title, initial, max_len, 0);
    }

    #[inline(always)]
    pub fn prompt_text_flags(&self, tag: u8, title: &[u8], initial: &[u8], max_len: u8, flags: u8) {
        let req = KrxModalReq {
            title: title.as_ptr(),
            msg: initial.as_ptr(),
            ok: core::ptr::null(),
            cancel: core::ptr::null(),
            tag,
            kind: 2,
            t_len: title.len().min(255) as u8,
            m_len: initial.len().min(255) as u8,
            o_len: flags,
            c_len: 0,
            max_len,
            _pad: 0,
        };
        let _ = self.call_op(
            Op::UiModal,
            crate::sub::ui_modal::SHOW as usize,
            &raw const req as usize,
            0,
        );
    }

    #[inline(always)]
    pub fn prompt_number(
        &self,
        tag: u8,
        title: &[u8],
        unit: &[u8],
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        decimals: u8,
    ) {
        let req = KrxNumReq {
            title: title.as_ptr(),
            unit: unit.as_ptr(),
            value,
            min,
            max,
            step,
            tag,
            decimals,
            title_len: title.len().min(255) as u8,
            unit_len: unit.len().min(255) as u8,
            _pad: [0; 4],
        };
        let _ = self.call_op(
            Op::UiModal,
            crate::sub::ui_modal::PROMPT_NUM as usize,
            &raw const req as usize,
            0,
        );
    }

    #[inline(always)]
    pub fn get_modal_text(&self, out: &mut [u8]) -> usize {
        self.call_op(
            Op::UiModal,
            crate::sub::ui_modal::GET_TEXT as usize,
            out.as_mut_ptr() as usize,
            out.len().min(255),
        )
    }

    #[inline(always)]
    pub fn draw_bitmap(&self, x: i16, y: i16, w: u8, h: u8, bits: &[u8]) {
        let req_bytes = (((w as usize) + 7) >> 3) * (h as usize);
        if bits.len() < req_bytes || req_bytes == 0 {
            return;
        }
        let a0 = (crate::sub::gfx_draw::BITMAP as usize)
            | ((COLOR_FILL as usize) << 8)
            | ((w as usize) << 16)
            | ((h as usize) << 24);
        let _ = self.call_op(Op::GfxDraw, a0, pack_xy(x, y), bits.as_ptr() as usize);
    }

    pub fn draw_tag_badge(&self, x: i16, y: i16, text: &[u8], filled: bool) {
        if text.is_empty() {
            return;
        }
        let tw = (self.measure_text(text, FONT_TOMTHUMB) + 4).max(6) as u8;
        if filled {
            self.fill_rect(x, y, tw, 7, COLOR_FILL);
            self.draw_text(x + 2, y + 6, text, FONT_TOMTHUMB, COLOR_CLEAR, ALIGN_LEFT);
        } else {
            self.draw_rect(x, y, tw, 7, COLOR_FILL);
            self.draw_text(x + 2, y + 6, text, FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);
        }
    }

    pub fn draw_key_badge(&self, x: i16, y: i16, key_s: &[u8], lbl: &[u8]) {
        if key_s.is_empty() {
            return;
        }
        let kw = (self.measure_text(key_s, FONT_TOMTHUMB) + 4).max(5) as u8;
        self.fill_rect(x, y, kw, 7, COLOR_FILL);
        self.clear_corners(x, y, kw, 7);
        self.draw_text(x + 2, y + 6, key_s, FONT_TOMTHUMB, COLOR_CLEAR, ALIGN_LEFT);
        if !lbl.is_empty() {
            self.draw_text(
                x + kw as i16 + 2,
                y + 6,
                lbl,
                FONT_TOMTHUMB,
                COLOR_FILL,
                ALIGN_LEFT,
            );
        }
    }

    pub fn draw_progress_bar(&self, x: i16, y: i16, w: u8, h: u8, pct: u8, border: bool) {
        if w < 4 || h < 2 {
            return;
        }
        let p = pct.min(100) as u32;
        self.fill_rect(x, y, w, h, COLOR_CLEAR);
        if border {
            self.draw_rect(x, y, w, h, COLOR_FILL);
            if h >= 5 {
                self.clear_corners(x, y, w, h);
            }
        }
        let inset = if border { 2u8 } else { 0u8 };
        let max_fw = w.saturating_sub(inset * 2);
        let fh = h.saturating_sub(inset * 2).max(1);
        let fw = self.udivmod((max_fw as u32) * p, 100) as u8;
        if fw > 0 {
            self.fill_rect(x + inset as i16, y + inset as i16, fw, fh, COLOR_FILL);
        }
    }

    pub fn draw_capsule_button(&self, x: i16, y: i16, w: u8, h: u8, text: &[u8], filled: bool) {
        if w < 4 || h < 5 {
            return;
        }
        if filled {
            self.fill_rect(x, y, w, h, COLOR_FILL);
        } else {
            self.fill_rect(x, y, w, h, COLOR_CLEAR);
            self.draw_rect(x, y, w, h, COLOR_FILL);
        }
        self.clear_corners(x, y, w, h);
        if !text.is_empty() {
            let col = if filled { COLOR_CLEAR } else { COLOR_FILL };
            self.draw_text(
                x + (w >> 1) as i16,
                y + h as i16 - 2,
                text,
                FONT_TOMTHUMB,
                col,
                ALIGN_CENTER,
            );
        }
    }

    #[inline(always)]
    pub fn draw_line(&self, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
        let sub_color = (crate::sub::gfx_draw::LINE as usize) | ((color as usize) << 8);
        let _ = self.call_op(Op::GfxDraw, sub_color, pack_xy(x0, y0), pack_xy(x1, y1));
    }

    #[inline(always)]
    pub fn play_beep(&self, beep_type: u8) {
        let _ = self.call_op(
            Op::Aud,
            (crate::sub::aud::TONE as usize) | ((beep_type as usize) << 8),
            0xFFFF,
            0,
        );
    }

    #[inline(always)]
    pub fn battery_mv(&self) -> u16 {
        self.prop_get32(crate::prop::BATT_MV) as u16
    }

    #[inline(always)]
    pub fn battery_pct(&self) -> u8 {
        self.prop_get(crate::prop::BATT_PCT)
    }

    #[inline(always)]
    pub fn mcu_temp_deci_c(&self) -> i16 {
        self.prop_get32(crate::prop::MCU_TEMP) as u16 as i16
    }

    #[inline(always)]
    pub fn reboot(&self, mode: u8) {
        let _ = self.call_op(
            Op::SysCtl,
            crate::sub::sys_ctl::REBOOT as usize,
            mode as usize,
            0,
        );
    }

    #[inline(always)]
    pub fn stack_free(&self) -> u32 {
        self.prop_get32(crate::prop::STACK_FREE)
    }

    #[inline(always)]
    pub fn cpu_hz(&self) -> u32 {
        self.prop_get32(crate::prop::CPU_HZ)
    }

    #[inline(always)]
    pub fn sys_info(&self, out: &mut KrxSysInfo) -> bool {
        self.call_op(
            Op::SysQuery,
            crate::sub::sys_query::INFO as usize,
            out as *mut KrxSysInfo as usize,
            0,
        ) == 0
    }

    #[inline(always)]
    pub fn set_status_bar(&self, sb: &KrxStatusBar) {
        let _ = self.call_op(
            Op::UiModal,
            crate::sub::ui_modal::SET_BAR as usize,
            sb as *const KrxStatusBar as usize,
            0,
        );
    }

    #[inline(always)]
    pub fn set_status_title(&self, title: &[u8]) {
        let mut sb = KrxStatusBar::empty();
        sb.flags = KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER | KRX_STATUS_CUSTOM_TITLE;
        let n = title.len().min(sb.title.len());
        let mut i = 0usize;
        while i < n {
            unsafe { *sb.title.get_unchecked_mut(i) = *title.get_unchecked(i) };
            i += 1;
        }
        self.set_status_bar(&sb);
    }

    #[inline(always)]
    pub fn list_dir(&self, path: &[u8], out: &mut [KrxDirEntry]) -> usize {
        self.list_dir_page(path, 0, out)
    }

    /// Paginated directory listing starting at `offset` (returns number of entries written).
    #[inline(always)]
    pub fn list_dir_page(&self, path: &[u8], offset: u32, out: &mut [KrxDirEntry]) -> usize {
        let req = KrxFsReq {
            path: path.as_ptr(),
            buf: out.as_mut_ptr() as *mut u8,
            offset,
            len: out.len() as u16,
            path_len: path.len() as u8,
            extra: 0,
        };
        self.call_op(
            Op::FsRead,
            crate::sub::fs_read::LIST_DIR as usize,
            &req as *const KrxFsReq as usize,
            0,
        )
    }

    /// Return total entry count in `path` without allocating a full directory array.
    #[inline(always)]
    pub fn count_dir(&self, path: &[u8]) -> usize {
        let req = KrxFsReq {
            path: path.as_ptr(),
            buf: core::ptr::null_mut(),
            offset: 0,
            len: 0,
            path_len: path.len() as u8,
            extra: 1,
        };
        self.call_op(
            Op::FsRead,
            crate::sub::fs_read::LIST_DIR as usize,
            &req as *const KrxFsReq as usize,
            0,
        )
    }

    #[inline(always)]
    pub fn read_file(&self, path: &[u8], out: &mut [u8]) -> usize {
        let req = KrxFsReq {
            path: path.as_ptr(),
            buf: out.as_mut_ptr(),
            offset: 0,
            len: out.len() as u16,
            path_len: path.len() as u8,
            extra: 0,
        };
        self.call_op(
            Op::FsRead,
            crate::sub::fs_read::FILE_AT as usize,
            &req as *const KrxFsReq as usize,
            0,
        )
    }

    #[inline(always)]
    pub fn remove_file(&self, path: &[u8]) -> bool {
        self.call_op(
            Op::FsMeta,
            crate::sub::fs_meta::REMOVE as usize,
            path.as_ptr() as usize,
            path.len(),
        ) == 0
    }

    #[inline(always)]
    pub fn battery_ma(&self) -> u16 {
        self.prop_get32(crate::prop::BATT_MA) as u16
    }

    #[inline(always)]
    pub fn is_charging(&self) -> bool {
        self.prop_get32(crate::prop::CHARGING) != 0
    }

    #[inline(always)]
    pub fn crc32(&self, data: &[u8]) -> u32 {
        self.call_op(
            Op::SysQuery,
            crate::sub::sys_query::CRC32 as usize,
            data.as_ptr() as usize,
            data.len(),
        ) as u32
    }

    #[inline(always)]
    pub fn reload_config(&self) -> bool {
        self.call_op(Op::SysQuery, crate::sub::sys_query::RELOAD as usize, 0, 0) == 0
    }

    #[inline(always)]
    pub fn write_file(&self, path: &[u8], data: &[u8]) -> bool {
        let req = KrxFsReq {
            path: path.as_ptr(),
            buf: data.as_ptr() as *mut u8,
            offset: 0,
            len: data.len() as u16,
            path_len: path.len() as u8,
            extra: 0,
        };
        self.call_op(
            Op::FsWrite,
            crate::sub::fs_write::FILE as usize,
            &req as *const KrxFsReq as usize,
            0,
        ) == 0
    }

    #[inline(always)]
    pub fn read_file_at(&self, path: &[u8], offset: u32, out: &mut [u8]) -> usize {
        let req = KrxFsReq {
            path: path.as_ptr(),
            buf: out.as_mut_ptr(),
            offset,
            len: out.len() as u16,
            path_len: path.len() as u8,
            extra: 0,
        };
        self.call_op(
            Op::FsRead,
            crate::sub::fs_read::FILE_AT as usize,
            &req as *const KrxFsReq as usize,
            0,
        )
    }

    #[inline(always)]
    pub fn call_leaf(&self, leaf_id: u8, arg: u32) -> u32 {
        self.call_op(
            Op::ProcExec,
            (crate::sub::proc_exec::LEAF as usize) | ((leaf_id as usize) << 8),
            arg as usize,
            0,
        ) as u32
    }

    #[inline(always)]
    pub fn launch_pkg(&self, path: &[u8]) -> bool {
        let a0 = (crate::sub::proc_ctl::LAUNCH_PKG as usize) | ((path.len().min(31)) << 8);
        self.call_op(Op::ProcCtl, a0, path.as_ptr() as usize, 0) == 0
    }

    #[inline(always)]
    pub fn self_pid(&self) -> u8 {
        self.prop_get(crate::prop::SELF_PID)
    }

    #[inline(always)]
    pub fn proc_count(&self) -> u8 {
        self.prop_get(crate::prop::PROC_COUNT)
    }

    #[inline(always)]
    pub fn proc_info(&self, index: u8, out: &mut KrxProcInfo) -> bool {
        let a0 = (crate::sub::proc_ctl::INFO as usize) | ((index as usize) << 8);
        self.call_op(Op::ProcCtl, a0, out as *mut KrxProcInfo as usize, 0) == 0
    }

    #[inline(always)]
    pub fn ipc_send(&self, msg: &KrxIpcMessage) -> bool {
        self.call_op(
            Op::Ipc,
            crate::sub::ipc::SEND as usize,
            msg as *const KrxIpcMessage as usize,
            0,
        ) == 0
    }

    #[inline(always)]
    pub fn ipc_recv(&self, out: &mut KrxIpcMessage) -> bool {
        self.call_op(
            Op::Ipc,
            crate::sub::ipc::RECV as usize,
            out as *mut KrxIpcMessage as usize,
            0,
        ) == 0
    }

    pub fn ipc_reply(&self, req_msg: &KrxIpcMessage, payload: &[u8]) -> bool {
        let rep = req_msg.reply_to(payload, false);
        self.ipc_send(&rep)
    }

    pub fn ipc_poll(&self, dst: &mut [u8]) -> usize {
        let mut msg = KrxIpcMessage::empty();
        if self.ipc_recv(&mut msg) {
            let n = (msg.payload_len as usize).min(dst.len());
            dst[..n].copy_from_slice(&msg.payload[..n]);
            n
        } else {
            0xFF
        }
    }

    pub fn ipc_send_str(&self, req: &KrxIpcSendReq) -> bool {
        let target_pid = if !req.target.is_null() && req.target_len > 0 {
            let slice = unsafe { core::slice::from_raw_parts(req.target, req.target_len as usize) };
            if slice == b"*" || slice == b"ALL" {
                ipc_target::BROADCAST
            } else {
                let pid = self.call_op(
                    Op::ProcCtl,
                    (crate::sub::proc_ctl::FIND as usize) | ((req.target_len as usize) << 8),
                    req.target as usize,
                    0,
                ) as u8;
                if pid == 0 {
                    return false;
                }
                pid
            }
        } else {
            ipc_target::FOREGROUND_APP
        };
        let payload = if !req.msg.is_null() && req.msg_len > 0 {
            unsafe { core::slice::from_raw_parts(req.msg, (req.msg_len as usize).min(24)) }
        } else {
            &[]
        };
        let msg = KrxIpcMessage::new(0, target_pid, ipc_msg_type::APP_DATA_TRANSFER, payload);
        self.ipc_send(&msg)
    }
}

#[inline(always)]
pub const fn pack_xy(x: i16, y: i16) -> usize {
    ((x as u16 as u32) | ((y as u16 as u32) << 16)) as usize
}

unsafe extern "C" fn stub_kern_call(op: u32, a0: usize, _a1: usize, _a2: usize) -> usize {
    match op as u8 {
        x if x == Op::SysProp.id() => {
            if ((a0 >> 8) & 0xFF) == 0 {
                match (a0 & 0xFF) as u8 {
                    crate::prop::OS_VERSION => KRX_ABI_VERSION as usize,
                    crate::prop::STACK_FREE => 1024,
                    crate::prop::CPU_HZ => 48_000_000,
                    crate::prop::BATT_MV => 4000,
                    crate::prop::MCU_TEMP => 250,
                    crate::prop::RF_RSSI => (-140i16 as i32 as u32) as usize,
                    _ => 0,
                }
            } else {
                0
            }
        }
        x if x == Op::ProcExec.id() => {
            if (a0 & 0xFF) == crate::sub::proc_exec::LEAF as usize {
                0xFFFF_FFFF
            } else {
                0
            }
        }
        x if x == Op::FsMeta.id() => usize::from(matches!(a0, 2 | 3 | 4 | 5)),
        x if x == Op::GfxDraw.id() => {
            if (a0 & 0xFF) == 5 {
                6
            } else {
                0
            }
        }
        x if x == Op::IoBus.id() => usize::from(matches!(a0, 0 | 1 | 6)),
        x if x == Op::SysQuery.id()
            || x == Op::ProcCtl.id()
            || x == Op::Ipc.id()
            || x == Op::FsWrite.id()
            || x == Op::RfTune.id()
            || x == Op::RfVfo.id()
            || x == Op::UiWidget.id() =>
        {
            1
        }
        _ => 0,
    }
}

// ── Compact Request Descriptors (Pointers-First, Zero Hidden Padding) ────────

/// `KrxTextReq::flags` bitmask constants.
pub const KRX_TEXT_MEASURE: u8 = 1 << 0;
pub const KRX_TEXT_CLIP: u8 = 1 << 1;
/// Translate `text` through the active app/OS `.lng` pack (falling back to the literal).
pub const KRX_TEXT_LOCALIZE: u8 = 1 << 2;
/// When combined with [`KRX_TEXT_LOCALIZE`], prefer the natural compact/short translation variant.
pub const KRX_TEXT_SHORT: u8 = 1 << 3;

/// Text draw / measure request (`Op::GfxText`).
/// `flags`: bit 0 = measure only, bit 1 = clip to `arg` (`max_w`), bit 2 = localize, bit 3 = short.
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

/// Filesystem read/write/rename/list request (`Op::FsRead`, `Op::FsWrite`, `Op::FsRename`).
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

/// Modal popup/confirm/text-prompt request (`Op::UiModal`, sub = 5).
/// `kind`: 0 = popup, 1 = confirm, 2 = text prompt.
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

/// Numeric / frequency prompt request (`Op::UiModal`, sub = `PROMPT_NUM`, `32 B` aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxNumReq {
    pub title: *const u8,
    pub unit: *const u8,
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub tag: u8,
    pub decimals: u8,
    pub title_len: u8,
    pub unit_len: u8,
    pub _pad: [u8; 4],
}

/// Raw peripheral bus request (`Op::IoBus`, sub = 0 I2C / 1 SPI / 6 flash_read).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxBusReq {
    pub tx: *const u8,
    pub rx: *mut u8,
    pub addr: u32,
    pub tx_len: u16,
    pub rx_len: u16,
}

/// Name-addressed IPC send request (`Op::IpcSendStr`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxIpcSendReq {
    pub target: *const u8,
    pub msg: *const u8,
    pub target_len: u8,
    pub msg_len: u8,
    pub _pad: [u8; 2],
}

/// Frequency sweep request (`Op::RfBulk`, sub = 0).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxSweepReq {
    pub out_rssi: *mut i16,
    pub start_hz: u32,
    pub step_hz: u32,
    pub count: u16,
    pub _pad: u16,
}

// ── UI Widget & Status Bar Descriptors ───────────────────────────────────────

pub const KRX_STATUS_VISIBLE: u8 = 1 << 0;
pub const KRX_STATUS_DIVIDER: u8 = 1 << 1;
pub const KRX_STATUS_HAS_SIGNAL: u8 = 1 << 2;
pub const KRX_STATUS_CUSTOM_TITLE: u8 = 1 << 3;
pub const KRX_STATUS_PARTIAL: u8 = 1 << 4;

pub const KRX_ICON_CUSTOM_0: u8 = 0x80;
pub const KRX_ICON_CUSTOM_1: u8 = 0x81;
pub const KRX_ICON_NONE: u8 = 0xFF;

pub const KRX_CUSTOM_TRAY_PERSIST: u8 = 1 << 0;
pub const KRX_CUSTOM_TRAY_ACTIVE: u8 = 1 << 1;

/// 64-byte ($2^6$) Power-of-Two Aligned C-ABI Status Bar Descriptor.
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

const _: () = assert!(core::mem::size_of::<KrxStatusBar>() == 64);

impl Default for KrxStatusBar {
    fn default() -> Self {
        Self::empty()
    }
}

impl KrxStatusBar {
    pub const EMPTY: Self = Self::empty();

    pub const fn empty() -> Self {
        Self {
            flags: KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER,
            signal_level: 0,
            left_icon_count: 0,
            center_icon_count: 0,
            left_icons: [0; 4],
            center_icons: [0; 4],
            title: [0; 16],
            badge: [0; 8],
            center: [0; 16],
            custom_icon_0: [0; 5],
            custom_icon_1: [0; 5],
            tray_icon_id: 0,
            custom_flags: 0,
        }
    }
}

pub const KRX_WIDGET_ROUNDED_BOX: u8 = 2;
pub const KRX_WIDGET_NEGATED_HEADER: u8 = 3;
pub const KRX_WIDGET_SHEET_FRAME: u8 = 4;
pub const KRX_WIDGET_PROGRESS_BAR: u8 = 5;
pub const KRX_WIDGET_S_METER: u8 = 6;
pub const KRX_WIDGET_TAG_BADGE: u8 = 7;
pub const KRX_WIDGET_KEY_BADGE: u8 = 8;
pub const KRX_WIDGET_CAPSULE_BUTTON: u8 = 9;
pub const KRX_WIDGET_SCROLLBAR: u8 = 10;
pub const KRX_WIDGET_MENU_ITEM: u8 = 11;
pub const KRX_WIDGET_BIG_DIGIT: u8 = 12;
pub const KRX_WIDGET_SMALL_DIGIT: u8 = 13;
pub const KRX_WIDGET_BATTERY: u8 = 14;
pub const KRX_WIDGET_SIGNAL: u8 = 15;
pub const KRX_WIDGET_MODAL_BOX: u8 = 16;
pub const KRX_WIDGET_DETAIL_ROW: u8 = 17;

/// Unified widget / bitmap draw descriptor (`Op::UiWidget`, `Op::GfxCtl` bitmap).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxWidget {
    pub p0: *const u8,
    pub p1: *const u8,
    pub x: i16,
    pub y: i16,
    pub len0: u16,
    pub len1: u16,
    pub extra: u16,
    pub w: u8,
    pub h: u8,
    pub a: u8,
    pub b: u8,
    pub _pad: [u8; 2],
}

unsafe impl Sync for KrxWidget {}
unsafe impl Send for KrxWidget {}

// ── Domain Data Structures (Power-of-Two Record Layouts) ─────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxSymbol {
    pub freq_word: u16,
    pub duration_us: u16,
}

/// Directory listing entry record with $O(1)$ power-of-two geometry (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxDirEntry {
    pub name: [u8; 24],
    pub size: u32,
    pub is_dir: u8,
    pub reserved: [u8; 3],
}

const _: () = assert!(core::mem::size_of::<KrxDirEntry>() == 32);

/// `KrxProcInfo::flags` bitmasks.
pub const KRX_PROC_FLAG_PERSISTENT: u8 = 1 << 0;
pub const KRX_PROC_FLAG_SUSPENDED: u8 = 1 << 1;
pub const KRX_PROC_FLAG_FOREGROUND: u8 = 1 << 2;
pub const KRX_PROC_FLAG_HAS_LEASE: u8 = 1 << 3;
pub const KRX_PROC_FLAG_KILLABLE: u8 = 1 << 4;

/// Process state descriptor record with $O(1)$ power-of-two geometry (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxProcInfo {
    pub pid: u8,
    pub app_id: u8,
    pub state: u8,
    pub priority: u8,
    pub runtime_ticks: u32,
    pub memory_bytes: u16,
    pub name: [u8; 12],
    pub exec_type: u8,
    pub flags: u8,
    pub stack_watermark: u16,
    pub ipc_events: u16,
    pub page_faults: u8,
    pub slice_overruns: u8,
    pub hibernations: u8,
    pub fault_count: u8,
}

const _: () = assert!(core::mem::size_of::<KrxProcInfo>() == 32);

pub mod ipc_flags {
    pub const NONE: u8 = 0;
    pub const REPLY: u8 = 1 << 0;
    pub const URGENT: u8 = 1 << 1;
    pub const NEED_ACK: u8 = 1 << 2;
    pub const ERROR: u8 = 1 << 3;
}

pub mod ipc_target {
    pub const FOREGROUND_APP: u8 = 0x00;
    pub const KERNEL: u8 = 0x01;
    pub const BROADCAST: u8 = 0xFF;
}

pub mod ipc_msg_type {
    pub const RAW_DATA: u16 = 0x0000;
    pub const PING: u16 = 0x0001;
    pub const PONG: u16 = 0x0002;
    pub const DAEMON_START: u16 = 0x0010;
    pub const DAEMON_STOP: u16 = 0x0011;
    pub const DAEMON_STATUS: u16 = 0x0012;
    pub const SERVICE_REQUEST: u16 = 0x0020;
    pub const SERVICE_RESPONSE: u16 = 0x0021;
    pub const APP_SWITCH: u16 = 0x0030;
    pub const APP_DATA_TRANSFER: u16 = 0x0031;
    pub const TELEMETRY_FRAME: u16 = 0x0040;
}

/// 32-byte zero-heap inter-process communication message (power-of-two $2^5$ layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxIpcMessage {
    pub sender_pid: u8,
    pub target_pid: u8,
    pub msg_type: u16,
    pub payload_len: u8,
    pub flags: u8,
    pub seq: u16,
    pub payload: [u8; 24],
}

const _: () = assert!(core::mem::size_of::<KrxIpcMessage>() == 32);

impl KrxIpcMessage {
    pub const fn empty() -> Self {
        Self {
            sender_pid: 0,
            target_pid: 0,
            msg_type: 0,
            payload_len: 0,
            flags: 0,
            seq: 0,
            payload: [0u8; 24],
        }
    }

    pub fn new(sender_pid: u8, target_pid: u8, msg_type: u16, payload_data: &[u8]) -> Self {
        let mut msg = Self::empty();
        msg.sender_pid = sender_pid;
        msg.target_pid = target_pid;
        msg.msg_type = msg_type;
        let len = payload_data.len().min(24);
        msg.payload_len = len as u8;
        let mut i = 0;
        while i < len {
            msg.payload[i] = payload_data[i];
            i += 1;
        }
        msg
    }

    pub fn make_reply_in_place(&mut self, payload_data: &[u8], is_error: bool) {
        core::mem::swap(&mut self.sender_pid, &mut self.target_pid);
        let len = payload_data.len().min(24);
        self.payload_len = len as u8;
        self.payload[..len].copy_from_slice(&payload_data[..len]);
        self.payload[len..].fill(0);
        self.flags = ipc_flags::REPLY | if is_error { ipc_flags::ERROR } else { 0 };
    }

    pub fn reply_to(&self, payload_data: &[u8], is_error: bool) -> Self {
        let mut rep = *self;
        rep.make_reply_in_place(payload_data, is_error);
        rep
    }

    #[inline(always)]
    pub const fn is_reply(&self) -> bool {
        (self.flags & ipc_flags::REPLY) != 0
    }

    #[inline(always)]
    pub const fn is_error(&self) -> bool {
        (self.flags & ipc_flags::ERROR) != 0
    }

    #[inline(always)]
    pub const fn is_broadcast(&self) -> bool {
        self.target_pid == ipc_target::BROADCAST
    }
}

/// C-ABI live coordinator telemetry snapshot.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KrxRadioState {
    pub active_vfo: u8,
    pub listening_vfo: u8,
    pub dual_watch: u8,
    pub cross_band: u8,
    pub bclo_policy: u8,
    pub ptt_active: u8,
    pub squelch_open: u8,
    pub subaudible_matched: u8,
    pub tot_locked_out: u8,
    pub lock_active: u8,
    pub tx_perm: u8,
    pub _reserved: u8,
    pub tot_remaining_s: u16,
    pub rssi_dbm: i16,
}

pub const KRX_SYS_INFO_FLAG_HAS_CERT: u16 = 1 << 0;
pub const KRX_SYS_INFO_FLAG_RDP_PROTECTED: u16 = 1 << 1;
pub const KRX_SYS_INFO_FLAG_CALIB_VALID: u16 = 1 << 2;

pub const KRX_QS1_NONE: u8 = 0;
pub const KRX_QS1_EMPTY: u8 = 1;
pub const KRX_QS1_INVALID: u8 = 2;
pub const KRX_QS1_UNVERIFIED: u8 = 3;
pub const KRX_QS1_VERIFIED: u8 = 4;

/// Canonical hardware, firmware build, and provisioning identity (`2^7 = 128` bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxSysInfo {
    pub flash_kb: u16,
    pub sram_kb: u16,
    pub flags: u16,
    pub mac: [u8; 6],
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub target_id: u8,
    pub model: [u8; 12],
    pub mcu: [u8; 12],
    pub boot_ver: [u8; 8],
    pub git_hash: [u8; 22],
    pub build_date: [u8; 12],
    pub serial: [u8; 15],
    pub qs1_len: u8,
    pub qs1: [u8; 17],
    pub mcu_uid: [u8; 12],
    pub qs1_status: u8,
}

const _: () = assert!(core::mem::size_of::<KrxSysInfo>() == 128);

unsafe impl Sync for KrxSysInfo {}
unsafe impl Send for KrxSysInfo {}
