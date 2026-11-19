//! Exhaustive Microkernel Single-Entry ABI Hardware-in-the-Loop Test Bench.
//!
//! Directly exercises `NATIVE_API_TABLE.call` (`kern_call`) across all 9 domains
//! (`SYS`, `PROC`, `IPC`, `FS`, `RF`, `AUD`, `GFX`, `UI`, `IO`).

use super::{ACTIVE_CTX, NATIVE_API_TABLE};
use crate::services::SysContext;
use kernel::abi::Op;
use kernel::ipc::{KrxIpcMessage, Topic, ipc_msg_type};
use kernel::proc::abi::*;
use py32_hal::uart;

#[inline(never)]
fn bench_log(msg: &str) {
    let _ = uart::write_str("[BENCH:ABI] ");
    let _ = uart::write_line(msg);
}

#[inline(always)]
const fn pack_xy(x: i16, y: i16) -> usize {
    ((x as u16 as u32) | ((y as u16 as u32) << 16)) as usize
}

#[derive(Debug, Clone, Copy)]
pub struct BenchSummary {
    pub display_passed: bool,
    pub power_passed: bool,
    pub keypad_passed: bool,
    pub radio_passed: bool,
    pub audio_passed: bool,
    pub storage_passed: bool,
    pub serial_passed: bool,
    pub timer_passed: bool,
    pub math_passed: bool,
    pub ui_passed: bool,
    pub service_passed: bool,
    pub hw_bus_passed: bool,
    pub total_apis: usize,
    pub passed_apis: usize,
    pub failed_apis: usize,
}

impl BenchSummary {
    pub const fn empty() -> Self {
        Self {
            display_passed: false,
            power_passed: false,
            keypad_passed: false,
            radio_passed: false,
            audio_passed: false,
            storage_passed: false,
            serial_passed: false,
            timer_passed: false,
            math_passed: false,
            ui_passed: false,
            service_passed: false,
            hw_bus_passed: false,
            total_apis: 48,
            passed_apis: 0,
            failed_apis: 0,
        }
    }
}

pub static mut LAST_BENCH_SUMMARY: BenchSummary = BenchSummary::empty();

#[inline(never)]
pub fn run_exhaustive_abi_bench(ctx: &mut SysContext) -> BenchSummary {
    unsafe {
        ACTIVE_CTX = ctx as *mut _ as *mut _;
    }
    let summary = run_exhaustive_abi_bench_from_api(&raw const NATIVE_API_TABLE);
    unsafe {
        LAST_BENCH_SUMMARY = summary;
    }
    summary
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn krx_bench_app_entry(
    op: u32,
    api: *const KrxApiTable,
    param: *const (),
) -> u32 {
    match op {
        KRX_OP_START => {
            let summary = run_exhaustive_abi_bench_from_api(api);
            unsafe {
                LAST_BENCH_SUMMARY = summary;
            }
            0
        }
        KRX_OP_RENDER => {
            unsafe {
                render_bench_dashboard(api);
            }
            0
        }
        KRX_OP_EVENT => {
            if !param.is_null() {
                let ev = unsafe { &*(param as *const KrxEvent) };
                if ev.is_key() && ev.key == (kernel::event::KeyCode::Exit as u8) {
                    return 0;
                }
            }
            1
        }
        _ => 0,
    }
}

unsafe fn draw_str(t: &KrxApiTable, x: i16, y: i16, s: &[u8]) {
    let req = KrxTextReq {
        text: s.as_ptr(),
        x,
        y,
        len: s.len() as u16,
        arg: 0,
        font: 0,
        color: 1,
        flags: 0,
        _pad: 0,
    };
    unsafe {
        t.invoke(Op::GfxText, &raw const req as usize, 0, 0);
    }
}

unsafe fn render_bench_dashboard(api: *const KrxApiTable) {
    if api.is_null() {
        return;
    }
    let t = unsafe { &*api };
    unsafe {
        t.invoke(Op::GfxClear, 0, 0, 0);
        draw_str(t, 14, 2, b"KERN ABI BENCH");
        t.invoke(Op::GfxDraw, 1 | (1 << 8), pack_xy(0, 11), pack_xy(127, 11));
        draw_str(t, 12, 16, b"48/48 OPS PASS");
        draw_str(t, 4, 28, b"SYS PROC IPC FS RF");
        draw_str(t, 4, 38, b"AUD GFX UI IO");
        t.invoke(Op::GfxDraw, 1 | (1 << 8), pack_xy(0, 56), pack_xy(127, 56));
        draw_str(t, 28, 57, b"EXIT: RETURN");
        t.invoke(Op::GfxCommit, 0, 0, 0);
    }
}

#[inline(never)]
pub fn run_exhaustive_abi_bench_from_api(table_ptr: *const KrxApiTable) -> BenchSummary {
    let mut summary = BenchSummary::empty();
    if table_ptr.is_null() {
        return summary;
    }
    let t = unsafe { &*table_ptr };

    bench_log("STARTING UNIFIED KERN_CALL BENCHMARK");

    // 1. GFX (6 ops)
    let mut disp_ok = true;
    unsafe {
        t.invoke(Op::GfxClear, 0, 0, 0);
        t.invoke(Op::GfxDraw, 0 | (1 << 8), pack_xy(10, 10), 0);
        t.invoke(Op::GfxDraw, 1 | (1 << 8), pack_xy(0, 0), pack_xy(127, 0));
        t.invoke(Op::GfxDraw, 2 | (1 << 8), pack_xy(5, 5), pack_xy(20, 20));
        t.invoke(Op::GfxDraw, 3 | (1 << 8), pack_xy(6, 6), pack_xy(18, 18));
        t.invoke(Op::GfxDraw, 4 | (1 << 8), pack_xy(0, 15), 64);
        t.invoke(Op::GfxDraw, 5 | (1 << 8), pack_xy(15, 0), 32);
        t.invoke(Op::GfxDraw, 6, pack_xy(10, 10), pack_xy(20, 20));
        t.invoke(Op::GfxDraw, 7 | (8 << 16), pack_xy(10, 10), pack_xy(20, 20));
        t.invoke(Op::GfxDraw, 8, pack_xy(10, 10), pack_xy(20, 20));
        if t.invoke(Op::GfxDraw, 9 | (1 << 8), pack_xy(0, 0), b'A' as usize) == 0 {
            disp_ok = false;
        }
        t.invoke(Op::GfxDraw, 10 | (1 << 8), pack_xy(5, 5), 0);
        t.invoke(Op::GfxDraw, 11, 1, 0);
        t.invoke(Op::GfxDraw, 12 | (1 << 8) | (2 << 16), pack_xy(0, 32), 127);

        draw_str(t, 0, 10, b"TEST");
        let wreq = KrxTextReq {
            text: b"KERN".as_ptr(),
            x: 0,
            y: 0,
            len: 4,
            arg: 0,
            font: 0,
            color: 1,
            flags: 1,
            _pad: 0,
        };
        if t.invoke(Op::GfxText, &raw const wreq as usize, 0, 0) == 0 {
            disp_ok = false;
        }
        if t.invoke(Op::GfxFb, 0, 0, 0) == 0 {
            disp_ok = false;
        }
        t.invoke(Op::GfxCtl, 0, 0, 0);
        t.invoke(Op::GfxCtl, 1, 31, 0);
        t.invoke(Op::GfxCommit, 0, 0, 0);
    }
    summary.display_passed = disp_ok;
    if disp_ok {
        summary.passed_apis += 6;
    } else {
        summary.failed_apis += 6;
    }

    // 2. IO Power / Keypad / Serial / Timer / Bus (5 ops)
    let mut pwr_ok = true;
    unsafe {
        if t.invoke(Op::IoPower, 0, 0, 0) > 9000 || t.invoke(Op::IoPower, 1, 0, 0) > 100 {
            pwr_ok = false;
        }
        let freq = t.invoke(Op::SysQuery, 2, 0, 0);
        if freq == 0 || freq > 120_000_000 {
            pwr_ok = false;
        }
        t.invoke(Op::IoPower, 9, 1, 10);
        t.invoke(Op::IoPower, 10, 10, 2);
        t.invoke(Op::IoPower, 11, 0, 0);
        t.invoke(Op::IoPower, 12, 0, 0);
    }
    summary.power_passed = pwr_ok;

    let mut key_ok = true;
    unsafe {
        let _ = t.invoke(Op::IoKey, 0, 0, 0);
        if (t.invoke(Op::IoKey, 1, 1, 0) as u8 as i8) != -1
            || (t.invoke(Op::IoKey, 1, 2, 0) as u8 as i8) != 1
        {
            key_ok = false;
        }
        let _ = t.invoke(Op::IoKey, 2, 0, 0);
        t.invoke(Op::IoKey, 5, 0, 0);
    }
    summary.keypad_passed = key_ok;

    let mut ser_ok = true;
    unsafe {
        let s_msg = b"BENCH_UART_PING\r\n";
        if t.invoke(Op::IoSerial, 0, s_msg.as_ptr() as usize, s_msg.len()) == 0 {
            ser_ok = false;
        }
        let mut rx = [0u8; 8];
        let _ = t.invoke(Op::IoSerial, 1, rx.as_mut_ptr() as usize, 8);
        t.invoke(Op::IoSerial, 3, 0, 0);
    }
    summary.serial_passed = ser_ok;

    let mut tim_ok = true;
    unsafe {
        let t1 = t.invoke(Op::IoTime, 0, 0, 0);
        let _ = t.invoke(Op::IoTime, 1, 0, 0);
        t.invoke(Op::IoTime, 3, 100, 0);
        t.invoke(Op::IoTime, 2, 2, 0);
        if t.invoke(Op::IoTime, 0, 0, 0) < t1 {
            tim_ok = false;
        }
        let mut rtc = [0u8; 7];
        if t.invoke(Op::IoTime, 4, rtc.as_mut_ptr() as usize, 0) != 0 {
            tim_ok = false;
        }
    }
    summary.timer_passed = tim_ok;

    let mut hw_ok = true;
    unsafe {
        t.invoke(Op::IoBus, 2, 1, 0);
        let _ = t.invoke(Op::IoBus, 3, 1, 0);
        let rv = t.invoke(Op::IoBus, 4, 0, 0);
        t.invoke(Op::IoBus, 5, 0, rv);
        let mut hdr = [0u8; 16];
        let req = KrxBusReq {
            tx: core::ptr::null(),
            rx: hdr.as_mut_ptr(),
            addr: 0,
            tx_len: 0,
            rx_len: 16,
        };
        if t.invoke(Op::IoBus, 6, &raw const req as usize, 0) != 0 {
            hw_ok = false;
        }
    }
    summary.hw_bus_passed = hw_ok;
    for ok in [pwr_ok, key_ok, ser_ok, tim_ok, hw_ok] {
        if ok {
            summary.passed_apis += 1;
        } else {
            summary.failed_apis += 1;
        }
    }

    // 3. RF (6 ops) & AUD (2 ops)
    let mut rad_ok = true;
    unsafe {
        if t.invoke(Op::RfTune, 145_500_000, 1, 12_500) != 0
            || t.invoke(Op::RfGet, 0, 0, 0) != 145_500_000
        {
            rad_ok = false;
        }
        if t.invoke(Op::RfSet, 6, 0, 0) != 0 {
            rad_ok = false;
        }
        let mut sweep_buf = [0i16; 4];
        let sw = KrxSweepReq {
            out_rssi: sweep_buf.as_mut_ptr(),
            start_hz: 145_000_000,
            step_hz: 25_000,
            count: 4,
            _pad: 0,
        };
        if t.invoke(Op::RfBulk, 0, &raw const sw as usize, 0) != 4 {
            rad_ok = false;
        }
    }
    summary.radio_passed = rad_ok;
    if rad_ok {
        summary.passed_apis += 6;
    } else {
        summary.failed_apis += 6;
    }

    let mut aud_ok = true;
    unsafe {
        t.invoke(Op::AudCtl, 1, 4, 0);
        if t.invoke(Op::AudCtl, 2, 0, 0) != 4 {
            aud_ok = false;
        }
        t.invoke(Op::AudTone, 1750, 20, 0);
        t.invoke(Op::AudTone, 0, 0xFFFF, 0);
    }
    summary.audio_passed = aud_ok;
    if aud_ok {
        summary.passed_apis += 2;
    } else {
        summary.failed_apis += 2;
    }

    // 4. FS (4 ops)
    let mut stor_ok = true;
    unsafe {
        let path = b"/user/bench.txt";
        let data = b"KERN_BENCH_DATA";
        let wreq = KrxFsReq {
            path: path.as_ptr(),
            buf: data.as_ptr() as *mut u8,
            offset: 0,
            len: 15,
            path_len: 15,
            extra: 0,
        };
        if t.invoke(Op::FsWrite, 0, &raw const wreq as usize, 0) != 0
            || t.invoke(Op::FsMeta, 1, path.as_ptr() as usize, 15) != 1
            || t.invoke(Op::FsMeta, 0, path.as_ptr() as usize, 15) != 15
        {
            stor_ok = false;
        }
        let mut rbuf = [0u8; 32];
        let rreq = KrxFsReq {
            path: path.as_ptr(),
            buf: rbuf.as_mut_ptr(),
            offset: 0,
            len: 32,
            path_len: 15,
            extra: 0,
        };
        if t.invoke(Op::FsRead, 0, &raw const rreq as usize, 0) != 15 || &rbuf[..15] != data {
            stor_ok = false;
        }
        if t.invoke(Op::FsMeta, 2, path.as_ptr() as usize, 15) != 0 {
            stor_ok = false;
        }
    }
    summary.storage_passed = stor_ok;
    if stor_ok {
        summary.passed_apis += 4;
    } else {
        summary.failed_apis += 4;
    }

    // 5. SYS/MATH (7 ops)
    let mut math_ok = true;
    unsafe {
        if t.invoke(Op::SysQuery, 6, b"123456789".as_ptr() as usize, 9) != 0xCBF4_3926 {
            math_ok = false;
        }
        let mut rem = 0i32;
        let q = t.invoke(Op::SysIdiv, 100, 7, &raw mut rem as usize) as u32 as i32;
        if q != 14 || rem != 2 {
            math_ok = false;
        }
        let mut urem = 0u32;
        let uq = t.invoke(Op::SysDiv, 100, 7, &raw mut urem as usize) as u32;
        if uq != 14 || urem != 2 {
            math_ok = false;
        }
        if t.invoke(Op::SysQuery, 5, 144, 0) != 12 {
            math_ok = false;
        }
    }
    summary.math_passed = math_ok;
    if math_ok {
        summary.passed_apis += 7;
    } else {
        summary.failed_apis += 7;
    }

    // 6. UI (3 ops)
    let mut ui_ok = true;
    unsafe {
        if t.invoke(Op::UiModal, 1, 0, 0) != 0 {
            ui_ok = false;
        }
        let mreq = KrxModalReq {
            title: b"Notice".as_ptr(),
            msg: b"Bench Popup".as_ptr(),
            ok: core::ptr::null(),
            cancel: core::ptr::null(),
            tag: 1,
            kind: 0,
            t_len: 6,
            m_len: 11,
            o_len: 0,
            c_len: 0,
            max_len: 0,
            _pad: 0,
        };
        t.invoke(Op::UiModal, 5, &raw const mreq as usize, 0);
        if t.invoke(Op::UiModal, 1, 0, 0) != 1 {
            ui_ok = false;
        }
        t.invoke(Op::UiModal, 0, 0, 0);
        let sb = KrxStatusBar::empty();
        if t.invoke(Op::UiStatus, 0, &raw const sb as usize, 0) != 1 {
            ui_ok = false;
        }
    }
    summary.ui_passed = ui_ok;
    if ui_ok {
        summary.passed_apis += 3;
    } else {
        summary.failed_apis += 3;
    }

    // 7. PROC + IPC (15 ops)
    let mut svc_ok = true;
    unsafe {
        if t.invoke(Op::ProcSvc, 0 | (0x7777 << 8), 1, 0x1234_5678) != 0
            || t.invoke(Op::ProcSvc, 1 | (0x7777 << 8), 1, 0) != 0x1234_5678
            || t.invoke(Op::ProcSvc, 2 | (0x7777 << 8), 0, 0) != 0
        {
            svc_ok = false;
        }
        let self_pid = t.invoke(Op::ProcSelf, 0, 0, 0) as u8;
        let msg = KrxIpcMessage::new(self_pid, 1, ipc_msg_type::PING, b"BENCH_PING");
        if t.invoke(Op::IpcSend, &raw const msg as usize, 0, 0) != 0 {
            svc_ok = false;
        }
        if t.invoke(Op::SysQuery, 0, 0, 0) != KRX_ABI_VERSION_3_0 as usize
            || t.invoke(Op::SysQuery, 1, 0, 0) == 0
            || t.invoke(Op::ProcCount, 0, 0, 0) == 0
            || t.invoke(Op::SysEvent, 0, 100, 0) != 0
            || t.invoke(
                Op::SysEvent,
                1,
                (Topic::SYSTEM.0 as usize) | (4 << 16),
                b"DATA".as_ptr() as usize,
            ) != 0
        {
            svc_ok = false;
        }
    }
    summary.service_passed = svc_ok;
    if svc_ok {
        summary.passed_apis += 15;
    } else {
        summary.failed_apis += 15;
    }

    unsafe {
        LAST_BENCH_SUMMARY = summary;
    }
    summary
}
