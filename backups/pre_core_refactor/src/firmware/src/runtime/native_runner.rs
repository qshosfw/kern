//! Native & Paged ARM Thumb-1 Execution Adapter (§11.2 / KRX-ABI 3.0).
//!
//! Provides zero-copy execution of relocatable ARM Thumb-1 machine code
//! loaded into an aligned execution segment, linked against the kernel's
//! KRX-ABI 3.0 C-ABI dispatch table and system primitive API.

use crate::services::FirmwareContext;
use gfx::Canvas;
use kernel::app::{AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::caps::CapabilitySet;
use kernel::event::{Event, KeyState};
use kernel::proc::abi::*;
use kernel::proc::krx::{KrxPreamble, SectionExecution};
use kernel::proc::reloc::apply_relocs;

pub const NATIVE_EXEC_BUFFER_WORDS: usize = 768; // 3.0 KB (3072 bytes) static buffer preserving SRAM stack headroom

#[repr(align(4))]
struct NativeExecSlot {
    buffer: [u32; NATIVE_EXEC_BUFFER_WORDS],
    entry_point: usize,
    app_name: heapless::String<16>,
    is_loaded: bool,
    caps: CapabilitySet,
}

static mut NATIVE_SLOT: NativeExecSlot = NativeExecSlot {
    buffer: [0u32; NATIVE_EXEC_BUFFER_WORDS],
    entry_point: 0,
    app_name: heapless::String::new(),
    is_loaded: false,
    caps: CapabilitySet::NONE,
};

static mut ACTIVE_CTX: *mut FirmwareContext<'static, 'static> = core::ptr::null_mut();
static mut ACTIVE_CANVAS: *mut Canvas = core::ptr::null_mut();

// C-ABI dispatch implementations
unsafe extern "C" fn api_draw_pixel(x: i16, y: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).draw_pixel(x, y, color);
        }
    }
}

unsafe extern "C" fn api_draw_line(x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).draw_line(x0, y0, x1, y1, color);
        }
    }
}

unsafe extern "C" fn api_draw_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).draw_rect(x, y, w, h, color);
        }
    }
}

unsafe extern "C" fn api_fill_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).fill_rect(x, y, w, h, color);
        }
    }
}

unsafe extern "C" fn api_draw_str(x: i16, y: i16, text: *const u8, len: u16, font: u8, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() && !text.is_null() && len > 0 {
            let slice = core::slice::from_raw_parts(text, len as usize);
            if let Ok(s) = core::str::from_utf8(slice) {
                use kernel::proc::primitives::DisplayPrimitive;
                (*ACTIVE_CTX).draw_str(x, y, s, font, color);
            }
        }
    }
}

unsafe extern "C" fn api_draw_str_align(
    x: i16,
    y: i16,
    text: *const u8,
    len: u16,
    font: u8,
    color: u8,
    align: u8,
) {
    unsafe {
        if !ACTIVE_CTX.is_null() && !text.is_null() && len > 0 {
            let slice = core::slice::from_raw_parts(text, len as usize);
            if let Ok(s) = core::str::from_utf8(slice) {
                use kernel::proc::primitives::DisplayPrimitive;
                (*ACTIVE_CTX).draw_str_align(x, y, s, font, color, align);
            }
        }
    }
}

unsafe extern "C" fn api_commit_frame() {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).commit_frame();
        }
    }
}

unsafe extern "C" fn api_commit_rows(row_mask: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).commit_rows(row_mask);
        }
    }
}

unsafe extern "C" fn api_get_framebuffer() -> *mut u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).get_framebuffer_mut().as_mut_ptr()
        } else {
            core::ptr::null_mut()
        }
    }
}

unsafe extern "C" fn api_rf_tune(freq_hz: u32, bw: u8, step_hz: u32) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            let bw_hz = if bw == 1 { 25_000 } else { 12_500 };
            (*ACTIVE_CTX).tune(freq_hz, bw_hz, step_hz);
            0
        } else {
            1
        }
    }
}

unsafe extern "C" fn api_rf_get_freq() -> u32 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).get_frequency()
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_rf_get_rssi() -> i16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).get_rssi_dbm()
        } else {
            -140
        }
    }
}

unsafe extern "C" fn api_rf_set_tx(active: u8) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).set_tx(active != 0);
            0
        } else {
            1
        }
    }
}

unsafe extern "C" fn api_rf_sweep(
    start_hz: u32,
    step_hz: u32,
    count: u16,
    out_rssi: *mut i16,
) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !out_rssi.is_null() && count > 0 {
            use kernel::proc::primitives::RadioPrimitive;
            let slice = core::slice::from_raw_parts_mut(out_rssi, count as usize);
            (*ACTIVE_CTX).sweep(start_hz, step_hz, slice) as u16
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_audio_play_tone(freq_hz: u16, duration_ms: u16) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            (*ACTIVE_CTX).play_tone(freq_hz, duration_ms);
        }
    }
}

unsafe extern "C" fn api_audio_capture_start(rate_hz: u16) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::AudioPrimitive;
            if (*ACTIVE_CTX).capture_start(rate_hz).is_ok() {
                0
            } else {
                1
            }
        } else {
            1
        }
    }
}

unsafe extern "C" fn api_audio_capture_read(out_samples: *mut i16, max_samples: u16) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !out_samples.is_null() && max_samples > 0 {
            use kernel::proc::primitives::AudioPrimitive;
            let slice = core::slice::from_raw_parts_mut(out_samples, max_samples as usize);
            (*ACTIVE_CTX).capture_read(slice) as u16
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_audio_capture_stop() {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::AudioPrimitive;
            (*ACTIVE_CTX).capture_stop();
        }
    }
}

unsafe extern "C" fn api_rf_symbol_tx_emit(symbols: *const KrxSymbol, count: u16) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !symbols.is_null() && count > 0 {
            use kernel::proc::primitives::RadioPrimitive;
            let slice = core::slice::from_raw_parts(symbols, count as usize);
            if (*ACTIVE_CTX).emit_symbols(slice).is_ok() {
                0
            } else {
                1
            }
        } else {
            1
        }
    }
}

unsafe extern "C" fn api_storage_load_config(buf: *mut u8, max_len: u16) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !buf.is_null() && max_len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let slot = &*core::ptr::addr_of!(NATIVE_SLOT);
            let slice = core::slice::from_raw_parts_mut(buf, max_len as usize);
            (*ACTIVE_CTX)
                .load_config(slot.app_name.as_str(), slice)
                .unwrap_or(0) as u16
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_storage_save_config(buf: *const u8, len: u16) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !buf.is_null() && len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let slot = &*core::ptr::addr_of!(NATIVE_SLOT);
            let slice = core::slice::from_raw_parts(buf, len as usize);
            if (*ACTIVE_CTX)
                .save_config(slot.app_name.as_str(), slice)
                .is_ok()
            {
                0
            } else {
                1
            }
        } else {
            1
        }
    }
}

unsafe extern "C" fn api_storage_read_asset(
    asset_id: u16,
    offset: u32,
    buf: *mut u8,
    len: u16,
) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !buf.is_null() && len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let slice = core::slice::from_raw_parts_mut(buf, len as usize);
            (*ACTIVE_CTX)
                .read_asset(asset_id, offset, slice)
                .unwrap_or(0) as u16
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_sys_poll_key() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::KeypadPrimitive;
            (*ACTIVE_CTX).poll_key().unwrap_or(0)
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_sys_nav_dir(key: u8) -> i8 {
    match key {
        1 => -1, // Up
        2 => 1,  // Down
        _ => 0,
    }
}

unsafe extern "C" fn api_sys_ticks_ms() -> u32 {
    py32_hal::delay::millis()
}

unsafe extern "C" fn api_sys_delay_ms(ms: u32) {
    py32_hal::delay::delay_ms(ms);
}

unsafe extern "C" fn api_sys_battery_mv() -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).battery_mv()
        } else {
            4000
        }
    }
}

unsafe extern "C" fn api_sys_battery_pct() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).battery_pct()
        } else {
            100
        }
    }
}

unsafe extern "C" fn api_sys_rand32() -> u32 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::MathPrimitive;
            (*ACTIVE_CTX).rand32()
        } else {
            0x5A5A_5A5A
        }
    }
}

unsafe extern "C" fn api_sys_stack_free() -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::ServicePrimitive;
            (*ACTIVE_CTX).get_stack_free_now()
        } else {
            1024
        }
    }
}

unsafe extern "C" fn api_hw_i2c_transfer(
    addr: u8,
    reg: u8,
    tx: *const u8,
    tx_len: u16,
    rx: *mut u8,
    rx_len: u16,
) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::HardwarePrimitive;
            if !tx.is_null() && tx_len > 0 {
                let tx_slice = core::slice::from_raw_parts(tx, tx_len as usize);
                if (*ACTIVE_CTX).i2c_write(addr, reg, tx_slice).is_err() {
                    return 1;
                }
            }
            if !rx.is_null() && rx_len > 0 {
                let rx_slice = core::slice::from_raw_parts_mut(rx, rx_len as usize);
                if (*ACTIVE_CTX).i2c_read(addr, reg, rx_slice).is_err() {
                    return 1;
                }
            }
            0
        } else {
            1
        }
    }
}

unsafe extern "C" fn api_hw_spi_transfer(cs_pin: u8, tx: *const u8, rx: *mut u8, len: u16) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && len > 0 {
            use kernel::proc::primitives::HardwarePrimitive;
            let tx_slice = if tx.is_null() {
                &[]
            } else {
                core::slice::from_raw_parts(tx, len as usize)
            };
            let rx_slice = if rx.is_null() {
                &mut []
            } else {
                core::slice::from_raw_parts_mut(rx, len as usize)
            };
            if (*ACTIVE_CTX)
                .spi_transfer(cs_pin, tx_slice, rx_slice)
                .is_ok()
            {
                0
            } else {
                1
            }
        } else {
            1
        }
    }
}

unsafe extern "C" fn api_hw_gpio_set(pin: u8, high: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::HardwarePrimitive;
            (*ACTIVE_CTX).gpio_write(pin, high != 0);
        }
    }
}

unsafe extern "C" fn api_hw_gpio_get(pin: u8) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::HardwarePrimitive;
            if (*ACTIVE_CTX).gpio_read(pin) { 1 } else { 0 }
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_hw_rf_read_reg(reg: u8) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).read_register(reg)
        } else {
            0
        }
    }
}

unsafe extern "C" fn api_hw_rf_write_reg(reg: u8, val: u16) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).write_register(reg, val);
        }
    }
}

static mut NATIVE_API_TABLE: KrxApiTable = KrxApiTable {
    abi_version: KRX_ABI_VERSION_3_0,
    table_size: core::mem::size_of::<KrxApiTable>() as u16,
    granted_caps: 0,
    draw_pixel: api_draw_pixel,
    draw_line: api_draw_line,
    draw_rect: api_draw_rect,
    fill_rect: api_fill_rect,
    draw_str: api_draw_str,
    draw_str_align: api_draw_str_align,
    commit_frame: api_commit_frame,
    commit_rows: api_commit_rows,
    get_framebuffer: api_get_framebuffer,
    rf_tune: api_rf_tune,
    rf_get_freq: api_rf_get_freq,
    rf_get_rssi: api_rf_get_rssi,
    rf_set_tx: api_rf_set_tx,
    rf_sweep: api_rf_sweep,
    audio_play_tone: api_audio_play_tone,
    audio_capture_start: api_audio_capture_start,
    audio_capture_read: api_audio_capture_read,
    audio_capture_stop: api_audio_capture_stop,
    rf_symbol_tx_emit: api_rf_symbol_tx_emit,
    storage_load_config: api_storage_load_config,
    storage_save_config: api_storage_save_config,
    storage_read_asset: api_storage_read_asset,
    sys_poll_key: api_sys_poll_key,
    sys_nav_dir: api_sys_nav_dir,
    sys_ticks_ms: api_sys_ticks_ms,
    sys_delay_ms: api_sys_delay_ms,
    sys_battery_mv: api_sys_battery_mv,
    sys_battery_pct: api_sys_battery_pct,
    sys_rand32: api_sys_rand32,
    sys_stack_free: api_sys_stack_free,
    hw_i2c_transfer: api_hw_i2c_transfer,
    hw_spi_transfer: api_hw_spi_transfer,
    hw_gpio_set: api_hw_gpio_set,
    hw_gpio_get: api_hw_gpio_get,
    hw_rf_read_reg: api_hw_rf_read_reg,
    hw_rf_write_reg: api_hw_rf_write_reg,
};

/// Loads a native relocatable binary from flash into the static execution arena.
/// Returns true if loaded and relocations (if any) succeeded.
pub fn load_native_from_storage(
    name: &str,
    storage: &mut crate::services::SystemStorage,
    path: &str,
    preamble: &KrxPreamble,
    exec: &SectionExecution,
    caps: CapabilitySet,
) -> bool {
    let slot = unsafe { &mut *core::ptr::addr_of_mut!(NATIVE_SLOT) };
    let buffer_bytes = unsafe {
        core::slice::from_raw_parts_mut(
            slot.buffer.as_mut_ptr() as *mut u8,
            NATIVE_EXEC_BUFFER_WORDS * 4,
        )
    };

    let total_read = storage.read_file(path, buffer_bytes).unwrap_or(0);
    let h_len = preamble.header_len as usize;
    if total_read <= h_len {
        return false;
    }

    let payload_len = total_read - h_len;
    // Shift payload to the beginning of the buffer
    buffer_bytes.copy_within(h_len..total_read, 0);

    let load_base = slot.buffer.as_ptr() as u32;
    let load_delta = load_base.wrapping_sub(preamble.link_base);

    if exec.reloc_count > 0 && exec.reloc_offset > 0 {
        let r_off = exec.reloc_offset as usize;
        if r_off < payload_len {
            let reloc_stream = &buffer_bytes[r_off..payload_len];
            let code_len = r_off;
            let res = unsafe {
                apply_relocs(
                    slot.buffer.as_mut_ptr() as *mut u8,
                    code_len,
                    reloc_stream,
                    load_delta,
                    exec.reloc_count,
                )
            };
            if res.is_err() {
                return false;
            }
        }
    }

    slot.entry_point = ((load_base + exec.entry_point) | 1) as usize;
    slot.app_name.clear();
    let _ = slot.app_name.push_str(name);
    slot.caps = caps;
    slot.is_loaded = true;

    unsafe {
        NATIVE_API_TABLE.granted_caps = caps.raw();
    }
    true
}

pub unsafe fn native_on_start(_ptr: *mut (), ctx: &mut FirmwareContext<'static, 'static>) {
    let slot = unsafe { &*core::ptr::addr_of!(NATIVE_SLOT) };
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *mut _;
            let entry: KrxNativeEntryFn = core::mem::transmute(slot.entry_point);
            let _ = entry(KRX_OP_START, &raw const NATIVE_API_TABLE, core::ptr::null());
            ACTIVE_CTX = core::ptr::null_mut();
        }
    }
}

pub unsafe fn native_on_event(
    _ptr: *mut (),
    ctx: &mut FirmwareContext<'static, 'static>,
    ev: &Event,
) -> EventResult {
    let slot = unsafe { &*core::ptr::addr_of!(NATIVE_SLOT) };
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *mut _;
            let (key_val, payload_val) = match ev {
                Event::Key { code, state } => {
                    let s_val = match state {
                        KeyState::Pressed => 0u32,
                        KeyState::Released => 1u32,
                        KeyState::Held => 2u32,
                    };
                    (*code as u8, s_val)
                }
                Event::Encoder(dir) => (0u8, *dir as u32),
                _ => (0u8, 0u32),
            };
            let krx_ev = KrxEvent {
                kind: match ev {
                    Event::Key { .. } => 1,
                    Event::Encoder(_) => 2,
                    Event::Tick => 3,
                    _ => 0,
                },
                key: key_val,
                flags: 0,
                reserved: 0,
                payload: payload_val,
            };
            let entry: KrxNativeEntryFn = core::mem::transmute(slot.entry_point);
            let res = entry(
                KRX_OP_EVENT,
                &raw const NATIVE_API_TABLE,
                &krx_ev as *const _ as *const (),
            );
            ACTIVE_CTX = core::ptr::null_mut();
            if res != 0 {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn native_on_modal_result(
    _ptr: *mut (),
    _ctx: &mut FirmwareContext<'static, 'static>,
    _r: &ModalResult<'_>,
) -> EventResult {
    EventResult::Ignored
}

pub unsafe fn native_on_render(
    _ptr: *const (),
    canvas: &mut Canvas,
    ctx: &FirmwareContext<'static, 'static>,
    status_bar: &mut StatusBarConfig<'_>,
) {
    let slot = unsafe { &*core::ptr::addr_of!(NATIVE_SLOT) };
    *status_bar = StatusBarConfig::new(slot.app_name.as_str());
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *const _ as *mut _;
            ACTIVE_CANVAS = canvas as *mut _;
            let entry: KrxNativeEntryFn = core::mem::transmute(slot.entry_point);
            let _ = entry(
                KRX_OP_RENDER,
                &raw const NATIVE_API_TABLE,
                core::ptr::null(),
            );
            ACTIVE_CANVAS = core::ptr::null_mut();
            ACTIVE_CTX = core::ptr::null_mut();
        }
    }
}

pub unsafe fn native_on_exit(_ptr: *mut (), ctx: &mut FirmwareContext<'static, 'static>) {
    let slot = unsafe { &*core::ptr::addr_of!(NATIVE_SLOT) };
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *mut _;
            let entry: KrxNativeEntryFn = core::mem::transmute(slot.entry_point);
            let _ = entry(KRX_OP_EXIT, &raw const NATIVE_API_TABLE, core::ptr::null());
            ACTIVE_CTX = core::ptr::null_mut();
        }
    }
}

pub static NATIVE_RUNNER_VTABLE: AppVTable<FirmwareContext<'static, 'static>> = AppVTable {
    on_start: native_on_start,
    on_event: native_on_event,
    on_modal_result: native_on_modal_result,
    render: native_on_render,
    on_exit: native_on_exit,
};
