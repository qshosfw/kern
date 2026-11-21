//! Kern Factory Calibration Inspector (`calib.mod` — External System Module).
//!
//! Reads `/sys/calib.dat` (512-byte factory RF/battery calibration block) and
//! presents XTAL trim, LNA/Mixer masks, audio gains, and battery ADC curves
//! using native OS `KRX_WIDGET_MENU_ITEM` widgets.
#![no_std]
#![no_main]

use kern_sdk::{
    COLOR_CLEAR, KRX_EV_ENCODER, KRX_EV_KEY, KRX_OP_EVENT, KRX_OP_EXIT, KRX_OP_RENDER,
    KRX_OP_START, KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_VISIBLE,
    KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxEvent, KrxStatusBar, KrxWidget, Op, format_u16_into, key,
    krx_entry, nth_packed_slice,
};

const ROWS: u8 = 11;
const VISIBLE: u8 = 4;
const LABELS: &[u8] = concat!(
    "Status\0",
    "XTAL Trim\0",
    "LNA Mask\0",
    "Mix Mask\0",
    "Vol Gain\0",
    "DAC Gain\0",
    "Batt W0\0",
    "Batt W1\0",
    "Batt W2\0",
    "Batt W3 (7.6V)\0",
    "Batt W4\0",
)
.as_bytes();

struct CalibState {
    sel: u8,
    loaded: bool,
    raw: [u8; 512],
}

impl CalibState {
    const fn new() -> Self {
        Self {
            sel: 0,
            loaded: false,
            raw: [0u8; 512],
        }
    }

    #[inline(always)]
    fn rd_u16(&self, off: usize) -> u16 {
        let lo = unsafe { *self.raw.get_unchecked(off) };
        let hi = unsafe { *self.raw.get_unchecked(off + 1) };
        u16::from_le_bytes([lo, hi])
    }

    #[inline(never)]
    fn set_title(api: &KrxApiTable) {
        let mut words = [0u32; 16];
        let ptr = words.as_mut_ptr() as *mut u8;
        let title = b"Calibration";
        unsafe {
            *ptr = KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER | KRX_STATUS_CUSTOM_TITLE;
            let mut i = 0usize;
            while i < title.len() {
                *ptr.add(12 + i) = *title.get_unchecked(i);
                i += 1;
            }
            api.set_status_bar(&*(ptr as *const KrxStatusBar));
        }
    }

    #[inline(never)]
    fn on_start(&mut self, api: &KrxApiTable) {
        self.sel = 0;
        let n = api.read_file(b"/sys/calib.dat", &mut self.raw);
        self.loaded = n == 512;
        Self::set_title(api);
    }

    #[inline(never)]
    fn step(&mut self, up: bool) {
        self.sel = if up {
            if self.sel > 0 { self.sel - 1 } else { ROWS - 1 }
        } else if self.sel + 1 < ROWS {
            self.sel + 1
        } else {
            0
        };
    }

    #[inline(never)]
    fn render(&self, api: &KrxApiTable) {
        api.clear();
        let top = if self.sel >= VISIBLE {
            self.sel - (VISIBLE - 1)
        } else {
            0
        };
        let mut buf = [0u8; 16];
        let mut vis = 0u8;
        while vis < VISIBLE {
            let idx = top + vis;
            if idx >= ROWS {
                break;
            }
            let val: &[u8] = if !self.loaded {
                if idx == 0 { b"Missing" } else { b"-" }
            } else {
                match idx {
                    0 => b"Valid 512B",
                    1 => {
                        let v = self.rd_u16(0x188) as i16;
                        if v < 0 {
                            let abs = (-v) as u16;
                            let s = format_u16_into(abs, 0, b"", &mut buf);
                            let slen = s.len().min(14);
                            let mut j = slen;
                            while j > 0 {
                                unsafe { *buf.get_unchecked_mut(j) = *buf.get_unchecked(j - 1) };
                                j -= 1;
                            }
                            unsafe {
                                *buf.get_unchecked_mut(0) = b'-';
                                core::slice::from_raw_parts(buf.as_ptr(), slen + 1)
                            }
                        } else {
                            format_u16_into(v as u16, 0, b"", &mut buf)
                        }
                    }
                    2 => format_u16_into(self.rd_u16(0x18A), 0, b"", &mut buf),
                    3 => format_u16_into(self.rd_u16(0x18C), 0, b"", &mut buf),
                    4 => format_u16_into(unsafe { *self.raw.get_unchecked(0x18E) } as u16, 0, b"", &mut buf),
                    5 => format_u16_into(unsafe { *self.raw.get_unchecked(0x18F) } as u16, 0, b"", &mut buf),
                    6 => format_u16_into(self.rd_u16(0x140), 0, b"", &mut buf),
                    7 => format_u16_into(self.rd_u16(0x142), 0, b"", &mut buf),
                    8 => format_u16_into(self.rd_u16(0x144), 0, b"", &mut buf),
                    9 => format_u16_into(self.rd_u16(0x146), 0, b"", &mut buf),
                    _ => format_u16_into(self.rd_u16(0x148), 0, b"", &mut buf),
                }
            };
            let label = nth_packed_slice(LABELS, idx);
            let y = 9 + (vis as i16) * 13;
            let desc = KrxWidget {
                p0: label.as_ptr(),
                p1: val.as_ptr(),
                x: 2,
                y,
                len0: label.len() as u16,
                len1: val.len() as u16,
                extra: 0,
                w: 120,
                h: 13,
                a: u8::from(idx == self.sel),
                b: 0,
                _pad: [0; 2],
            };
            let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
            vis += 1;
        }
        api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
        api.draw_scrollbar(125, 9, 52, top as u16, ROWS as u16, VISIBLE as u16);
        api.commit();
    }
}

static mut STATE: CalibState = CalibState::new();

fn app_entry(op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    let state = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match op {
        KRX_OP_START => {
            state.on_start(api);
            1
        }
        KRX_OP_EVENT => {
            if arg.is_null() {
                return 0;
            }
            let ev = unsafe { &*(arg as *const KrxEvent) };
            match ev.kind {
                KRX_EV_ENCODER => {
                    let d = ev.payload as i32;
                    if d != 0 {
                        state.step(d < 0);
                        return 1;
                    }
                    0
                }
                KRX_EV_KEY if ev.payload == key::STATE_PRESSED => match ev.key {
                    key::UP | key::KEY2 => {
                        state.step(true);
                        1
                    }
                    key::DOWN | key::KEY8 => {
                        state.step(false);
                        1
                    }
                    key::EXIT => 0,
                    _ => 0,
                },
                _ => 0,
            }
        }
        KRX_OP_RENDER => {
            state.render(api);
            1
        }
        KRX_OP_EXIT => 0,
        _ => 0,
    }
}

krx_entry!(app_entry);
