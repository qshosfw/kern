//! Modular Capability Router & C-ABI VTable Bridge (KRX-ABI 2.5.0 Chapter 8).
//!
//! Implements the kernel primitive C-ABI handlers, the immutable per-capability
//! domain VTables (`CapDisplayVTable`, `CapRadioVTable`, `CapAudioVTable`,
//! `CapStorageVTable`, `CapKeypadVTable`, `CapPowerVTable`, `CapTimerVTable`,
//! `CapMathVTable`, `CapServiceVTable`, `CapHwBusVTable`), the dynamic
//! `ServiceRegistry`, and the `CapImports` GOT binder (`bind_cap_imports`).

use crate::services::FirmwareContext;
use gfx::Canvas;
use kernel::caps::{Cap, CapabilitySet};
use kernel::proc::abi::*;
use kernel::proc::krx::CapBinding;
use kernel::proc::service_registry::ServiceRegistry;

pub(crate) static mut ACTIVE_CTX: *mut FirmwareContext<'static, 'static> = core::ptr::null_mut();
pub(crate) static mut ACTIVE_CANVAS: *mut Canvas = core::ptr::null_mut();
pub static mut SERVICE_REGISTRY: ServiceRegistry = ServiceRegistry::new();

// Zero-heap inter-app handoff mailbox (32-byte payload + 16-byte target name)
static mut HANDOFF_TARGET: [u8; 16] = [0u8; 16];
static mut HANDOFF_TARGET_LEN: u8 = 0;
static mut HANDOFF_MSG: [u8; 32] = [0u8; 32];
static mut HANDOFF_MSG_LEN: u8 = 0;
static mut HANDOFF_PENDING: bool = false;

#[inline(always)]
pub fn service_registry_mut() -> &'static mut ServiceRegistry {
    unsafe { &mut *core::ptr::addr_of_mut!(SERVICE_REGISTRY) }
}

// ── 1. CapDisplay C-ABI Handlers ────────────────────────────────────────────

pub unsafe extern "C" fn api_display_clear() {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).clear();
        }
    }
}

pub unsafe extern "C" fn api_draw_pixel(x: i16, y: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).draw_pixel(x, y, color);
        }
    }
}

pub unsafe extern "C" fn api_draw_line(x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).draw_line(x0, y0, x1, y1, color);
        }
    }
}

pub unsafe extern "C" fn api_draw_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).draw_rect(x, y, w, h, color);
        }
    }
}

pub unsafe extern "C" fn api_fill_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).fill_rect(x, y, w, h, color);
        }
    }
}

pub unsafe extern "C" fn api_draw_bitmap(x: i16, y: i16, w: u8, h: u8, data: *const u8, len: u16) {
    unsafe {
        if !ACTIVE_CTX.is_null() && !data.is_null() && len > 0 {
            use kernel::proc::primitives::DisplayPrimitive;
            let slice = core::slice::from_raw_parts(data, len as usize);
            (*ACTIVE_CTX).draw_bitmap(x, y, w, h, slice);
        }
    }
}

pub unsafe extern "C" fn api_draw_str(
    x: i16,
    y: i16,
    text: *const u8,
    len: u16,
    font: u8,
    color: u8,
) {
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

pub unsafe extern "C" fn api_draw_str_align(
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

pub unsafe extern "C" fn api_commit_frame() {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).commit_frame();
        }
    }
}

pub unsafe extern "C" fn api_commit_rows(row_mask: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).commit_rows(row_mask);
        }
    }
}

pub unsafe extern "C" fn api_get_framebuffer() -> *mut u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::DisplayPrimitive;
            (*ACTIVE_CTX).get_framebuffer_mut().as_mut_ptr()
        } else {
            core::ptr::null_mut()
        }
    }
}

// ── 2. CapRadio C-ABI Handlers ──────────────────────────────────────────────

pub unsafe extern "C" fn api_rf_tune(freq_hz: u32, bw: u8, step_hz: u32) -> u8 {
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

pub unsafe extern "C" fn api_rf_get_freq() -> u32 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).get_frequency()
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_rf_get_rssi() -> i16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).get_rssi_dbm()
        } else {
            -140
        }
    }
}

pub unsafe extern "C" fn api_rf_is_carrier() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            if (*ACTIVE_CTX).is_carrier_detected() {
                1
            } else {
                0
            }
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_rf_set_modulation(mod_type: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).set_modulation(mod_type);
        }
    }
}

pub unsafe extern "C" fn api_rf_set_squelch(level: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).set_squelch(level);
        }
    }
}

pub unsafe extern "C" fn api_rf_set_tx(active: u8) -> u8 {
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

pub unsafe extern "C" fn api_rf_set_tx_power(level: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).set_tx_power(level);
        }
    }
}

pub unsafe extern "C" fn api_rf_set_af_mode(mode: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).set_af_mode(mode);
        }
    }
}

pub unsafe extern "C" fn api_rf_sweep(
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

pub unsafe extern "C" fn api_rf_symbol_tx_emit(symbols: *const KrxSymbol, count: u16) -> u8 {
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

pub unsafe extern "C" fn api_rf_set_dual_tones(
    tone1_word: u16,
    tone1_gain: u8,
    tone2_word: u16,
    tone2_gain: u8,
) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).set_dual_tones_raw(tone1_word, tone1_gain, tone2_word, tone2_gain);
        }
    }
}

pub unsafe extern "C" fn api_rf_fsk_init(baud: u16) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).fsk_init(baud);
        }
    }
}

pub unsafe extern "C" fn api_rf_fsk_send(data: *const u8, len: u16) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !data.is_null() && len > 0 {
            use kernel::proc::primitives::RadioPrimitive;
            let slice = core::slice::from_raw_parts(data, len as usize);
            if (*ACTIVE_CTX).fsk_send(slice).is_ok() {
                0
            } else {
                1
            }
        } else {
            1
        }
    }
}

pub unsafe extern "C" fn api_rf_fsk_recv(buf: *mut u8, max_len: u16) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !buf.is_null() && max_len > 0 {
            use kernel::proc::primitives::RadioPrimitive;
            let slice = core::slice::from_raw_parts_mut(buf, max_len as usize);
            (*ACTIVE_CTX).fsk_receive(slice) as u16
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_rf_set_mic_mute(muted: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).mute_mic(muted != 0);
        }
    }
}

pub unsafe extern "C" fn api_rf_set_tx_carrier(active: u8) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).set_tx_carrier(active != 0);
            0
        } else {
            1
        }
    }
}

// ── 3. CapAudio C-ABI Handlers ──────────────────────────────────────────────

pub unsafe extern "C" fn api_audio_play_tone(freq_hz: u16, duration_ms: u16) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            (*ACTIVE_CTX).play_tone(freq_hz, duration_ms);
        }
    }
}

pub unsafe extern "C" fn api_audio_play_beep(beep_type: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::AudioPrimitive;
            AudioPrimitive::play_beep(&mut *ACTIVE_CTX, beep_type);
        }
    }
}

pub unsafe extern "C" fn api_audio_set_path(on: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::AudioPrimitive;
            (*ACTIVE_CTX).set_audio_path(on != 0);
        }
    }
}

pub unsafe extern "C" fn api_audio_capture_start(rate_hz: u16) -> u8 {
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

pub unsafe extern "C" fn api_audio_capture_read(out_samples: *mut i16, max_samples: u16) -> u16 {
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

pub unsafe extern "C" fn api_audio_capture_stop() {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::AudioPrimitive;
            (*ACTIVE_CTX).capture_stop();
        }
    }
}

// ── 4. CapStorage C-ABI Handlers ────────────────────────────────────────────

pub unsafe extern "C" fn api_storage_load_config(buf: *mut u8, max_len: u16) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !buf.is_null() && max_len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let app_name = super::native_runner::active_native_app_name();
            let slice = core::slice::from_raw_parts_mut(buf, max_len as usize);
            (*ACTIVE_CTX).load_config(app_name, slice).unwrap_or(0) as u16
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_storage_save_config(buf: *const u8, len: u16) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !buf.is_null() && len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let app_name = super::native_runner::active_native_app_name();
            let slice = core::slice::from_raw_parts(buf, len as usize);
            if (*ACTIVE_CTX).save_config(app_name, slice).is_ok() {
                0
            } else {
                1
            }
        } else {
            1
        }
    }
}

pub unsafe extern "C" fn api_storage_read_asset(
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

pub unsafe extern "C" fn api_storage_read_file(
    path: *const u8,
    path_len: u8,
    buf: *mut u8,
    max_len: u16,
) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !path.is_null() && !buf.is_null() && path_len > 0 && max_len > 0
        {
            use kernel::proc::primitives::StoragePrimitive;
            let path_slice = core::slice::from_raw_parts(path, path_len as usize);
            if let Ok(path_str) = core::str::from_utf8(path_slice) {
                let dst = core::slice::from_raw_parts_mut(buf, max_len as usize);
                return (*ACTIVE_CTX).read_file(path_str, dst).unwrap_or(0) as u16;
            }
        }
        0
    }
}

pub unsafe extern "C" fn api_storage_write_file(
    path: *const u8,
    path_len: u8,
    data: *const u8,
    len: u16,
) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !path.is_null() && !data.is_null() && path_len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let path_slice = core::slice::from_raw_parts(path, path_len as usize);
            if let Ok(path_str) = core::str::from_utf8(path_slice) {
                if path_str.starts_with("/sys/") {
                    return 1;
                }
                let src = core::slice::from_raw_parts(data, len as usize);
                if (*ACTIVE_CTX).write_file(path_str, src).is_ok() {
                    return 0;
                }
            }
        }
        1
    }
}

pub unsafe extern "C" fn api_storage_remove_file(path: *const u8, path_len: u8) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !path.is_null() && path_len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let path_slice = core::slice::from_raw_parts(path, path_len as usize);
            if let Ok(path_str) = core::str::from_utf8(path_slice) {
                if path_str.starts_with("/sys/") {
                    return 1;
                }
                if (*ACTIVE_CTX).remove_file(path_str).is_ok() {
                    return 0;
                }
            }
        }
        1
    }
}

pub unsafe extern "C" fn api_storage_file_size(path: *const u8, path_len: u8) -> u32 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !path.is_null() && path_len > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let path_slice = core::slice::from_raw_parts(path, path_len as usize);
            if let Ok(path_str) = core::str::from_utf8(path_slice) {
                return (*ACTIVE_CTX).file_size(path_str).unwrap_or(0) as u32;
            }
        }
        0
    }
}

pub unsafe extern "C" fn api_storage_list_dir(
    dir_path: *const u8,
    dir_len: u8,
    out_entries: *mut KrxDirEntry,
    max_entries: u8,
) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !out_entries.is_null() && max_entries > 0 {
            use kernel::proc::primitives::StoragePrimitive;
            let dir_slice = if dir_path.is_null() || dir_len == 0 {
                b"/"
            } else {
                core::slice::from_raw_parts(dir_path, dir_len as usize)
            };
            if let Ok(dir_str) = core::str::from_utf8(dir_slice) {
                let dst = core::slice::from_raw_parts_mut(out_entries, max_entries as usize);
                return (*ACTIVE_CTX).list_dir(dir_str, dst).unwrap_or(0) as u8;
            }
        }
        0
    }
}

// ── 5..12. CapKeypad / CapPower / CapSerial / CapTimer / CapUi / CapMath / CapFm / CapService ──

pub unsafe extern "C" fn api_sys_poll_key() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::KeypadPrimitive;
            (*ACTIVE_CTX).poll_key().unwrap_or(0)
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_nav_dir(key: u8) -> i8 {
    match key {
        1 => -1, // Up
        2 => 1,  // Down
        _ => 0,
    }
}

pub unsafe extern "C" fn api_sys_key_matrix() -> u32 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::KeypadPrimitive;
            (*ACTIVE_CTX).get_key_matrix()
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_ticks_ms() -> u32 {
    py32_hal::delay::millis()
}

pub unsafe extern "C" fn api_sys_uptime_secs() -> u32 {
    py32_hal::delay::millis() / 1000
}

pub unsafe extern "C" fn api_sys_delay_ms(ms: u32) {
    py32_hal::delay::delay_ms(ms);
}

pub unsafe extern "C" fn api_sys_battery_mv() -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).battery_mv()
        } else {
            4000
        }
    }
}

pub unsafe extern "C" fn api_sys_battery_pct() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).battery_pct()
        } else {
            100
        }
    }
}

pub unsafe extern "C" fn api_sys_is_charging() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            if (*ACTIVE_CTX).is_charging() { 1 } else { 0 }
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_wake_backlight() {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).wake_backlight();
        }
    }
}

pub unsafe extern "C" fn api_sys_battery_ma() -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).battery_ma()
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_battery_type() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).battery_type()
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_raw_battery_adc() -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).raw_battery_adc()
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_mcu_temp() -> i16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).mcu_temp_deci_c()
        } else {
            250
        }
    }
}

pub unsafe extern "C" fn api_sys_is_low() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            if (*ACTIVE_CTX).is_low() { 1 } else { 0 }
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_is_critical() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            if (*ACTIVE_CTX).is_critical() { 1 } else { 0 }
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_set_backlight(mode: u8, timeout_s: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).set_backlight(mode, timeout_s);
        }
    }
}

pub unsafe extern "C" fn api_sys_set_backlight_brightness(max: u8, min: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::PowerPrimitive;
            (*ACTIVE_CTX).set_backlight_brightness(max, min);
        }
    }
}

pub unsafe extern "C" fn api_serial_write(data: *const u8, len: u16) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !data.is_null() && len > 0 {
            use kernel::proc::primitives::SerialPrimitive;
            let slice = core::slice::from_raw_parts(data, len as usize);
            (*ACTIVE_CTX).write(slice);
            len
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_serial_read(buf: *mut u8, max_len: u16) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() && !buf.is_null() && max_len > 0 {
            use kernel::proc::primitives::SerialPrimitive;
            let slice = core::slice::from_raw_parts_mut(buf, max_len as usize);
            (*ACTIVE_CTX).read(slice) as u16
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_serial_is_connected() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::SerialPrimitive;
            if (*ACTIVE_CTX).is_usb_active() { 1 } else { 0 }
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_ui_close_modal() {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            (*ACTIVE_CTX).close_modal();
        }
    }
}

pub unsafe extern "C" fn api_ui_has_modal() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            if (*ACTIVE_CTX).has_active_modal() {
                1
            } else {
                0
            }
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_rand32() -> u32 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::MathPrimitive;
            (*ACTIVE_CTX).rand32()
        } else {
            0x5A5A_5A5A
        }
    }
}

pub unsafe extern "C" fn api_math_crc32(data: *const u8, len: u16) -> u32 {
    unsafe {
        if !data.is_null() && len > 0 {
            let slice = core::slice::from_raw_parts(data, len as usize);
            crypto::crc::CrcEngine::crc32(slice)
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_math_crc16(data: *const u8, len: u16) -> u16 {
    unsafe {
        if !data.is_null() && len > 0 {
            let slice = core::slice::from_raw_parts(data, len as usize);
            crypto::crc::CrcEngine::crc16_ccitt(slice)
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_math_idivmod(n: i32, d: i32) -> u64 {
    let (q, r) = if d == 0 { (0, n) } else { (n / d, n % d) };
    ((r as u32 as u64) << 32) | (q as u32 as u64)
}

pub unsafe extern "C" fn api_math_uidivmod(n: u32, d: u32) -> u64 {
    let (q, r) = if d == 0 { (0, n) } else { (n / d, n % d) };
    ((r as u64) << 32) | (q as u64)
}

pub unsafe extern "C" fn api_fm_tune(freq_100khz: u16) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::FmPrimitive;
            (*ACTIVE_CTX).fm_tune(freq_100khz);
        }
    }
}

pub unsafe extern "C" fn api_fm_set_volume(volume: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::FmPrimitive;
            (*ACTIVE_CTX).fm_set_volume(volume);
        }
    }
}

pub unsafe extern "C" fn api_fm_mute(mute: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::FmPrimitive;
            (*ACTIVE_CTX).fm_mute(mute != 0);
        }
    }
}

pub unsafe extern "C" fn api_fm_get_rssi() -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::FmPrimitive;
            (*ACTIVE_CTX).fm_get_rssi()
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_sys_stack_free() -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::ServicePrimitive;
            (*ACTIVE_CTX).get_stack_free_now()
        } else {
            kernel::sys::SystemDiagnostics::stack_margin_bytes() as u16
        }
    }
}

pub unsafe extern "C" fn api_sys_stack_free_min() -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::ServicePrimitive;
            (*ACTIVE_CTX).get_stack_free_min()
        } else {
            kernel::sys::SystemDiagnostics::ram_available_bytes() as u16
        }
    }
}

pub unsafe extern "C" fn api_sys_device_uid(out_uid12: *mut u8) {
    unsafe {
        if !out_uid12.is_null() {
            let uid = bsp::uid::read_mcu_uid();
            let dst = core::slice::from_raw_parts_mut(out_uid12, 12);
            dst.copy_from_slice(&uid[..12]);
        }
    }
}

pub unsafe extern "C" fn api_page_fault(block_id: u8, offset: u16) -> usize {
    super::native_runner::krx_page_fault(block_id, offset)
}

pub unsafe extern "C" fn api_svc_register(
    interface_id: u16,
    abi_version: u16,
    vtable_ptr: usize,
) -> u8 {
    let provider_id = crate::AppRegistry::count() as u8;
    if service_registry_mut().register(
        interface_id,
        abi_version,
        kernel::proc::Pid(provider_id),
        vtable_ptr,
        false,
    ) {
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn api_svc_resolve(interface_id: u16, min_abi: u16) -> usize {
    service_registry_mut()
        .resolve(interface_id, min_abi)
        .unwrap_or(0)
}

pub unsafe extern "C" fn api_ipc_handoff(
    target_name: *const u8,
    name_len: u8,
    msg: *const u8,
    msg_len: u8,
) -> u8 {
    unsafe {
        if target_name.is_null() || name_len == 0 {
            HANDOFF_PENDING = false;
            return 1;
        }
        let nlen = (name_len as usize).min(16);
        let nslice = core::slice::from_raw_parts(target_name, nlen);
        HANDOFF_TARGET[..nlen].copy_from_slice(nslice);
        HANDOFF_TARGET_LEN = nlen as u8;

        let mlen = if msg.is_null() {
            0
        } else {
            (msg_len as usize).min(32)
        };
        if mlen > 0 {
            let mslice = core::slice::from_raw_parts(msg, mlen);
            HANDOFF_MSG[..mlen].copy_from_slice(mslice);
        }
        HANDOFF_MSG_LEN = mlen as u8;
        HANDOFF_PENDING = true;
        0
    }
}

pub unsafe extern "C" fn api_ipc_handoff_read(out_buf: *mut u8, max_len: u8) -> u8 {
    unsafe {
        if !HANDOFF_PENDING {
            return 0xFF;
        }
        HANDOFF_PENDING = false;
        let mlen = HANDOFF_MSG_LEN as usize;
        let copy_len = mlen.min(max_len as usize);
        if !out_buf.is_null() && copy_len > 0 {
            let dst = core::slice::from_raw_parts_mut(out_buf, copy_len);
            dst.copy_from_slice(&HANDOFF_MSG[..copy_len]);
        }
        copy_len as u8
    }
}

pub unsafe extern "C" fn api_svc_release(interface_id: u16) -> u8 {
    if service_registry_mut().release(interface_id) {
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn api_ipc_send(
    target_name: *const u8,
    name_len: u8,
    msg: *const u8,
    msg_len: u8,
) -> u8 {
    unsafe { api_ipc_handoff(target_name, name_len, msg, msg_len) }
}

pub unsafe extern "C" fn api_ipc_poll(out_buf: *mut u8, max_len: u8) -> u8 {
    unsafe { api_ipc_handoff_read(out_buf, max_len) }
}

pub unsafe extern "C" fn api_sys_os_version() -> u32 {
    KRX_ABI_VERSION_3_0
}

pub unsafe extern "C" fn api_sys_ram_available() -> u16 {
    kernel::sys::SystemDiagnostics::ram_available_bytes() as u16
}

pub unsafe extern "C" fn api_sys_stack_margin() -> u16 {
    kernel::sys::SystemDiagnostics::stack_margin_bytes() as u16
}

pub unsafe extern "C" fn api_sys_proc_count() -> u8 {
    let table = crate::orchestrator::proc_table();
    let mut count = 0u8;
    for slot in table.slots.iter() {
        if slot.state != kernel::proc::ProcState::Dormant
            && slot.state != kernel::proc::ProcState::Dead
        {
            count += 1;
        }
    }
    count
}

pub unsafe extern "C" fn api_sys_proc_info(idx: u8, out_info: *mut KrxProcInfo) -> u8 {
    if out_info.is_null() {
        return 1;
    }
    let table = crate::orchestrator::proc_table();
    let mut match_idx = 0u8;
    for slot in table.slots.iter() {
        if slot.state != kernel::proc::ProcState::Dormant
            && slot.state != kernel::proc::ProcState::Dead
        {
            if match_idx == idx {
                unsafe {
                    let info = &mut *out_info;
                    info.pid = slot.pid.0;
                    info.app_id = slot.app_id as u8;
                    info.state = match slot.state {
                        kernel::proc::ProcState::Running => 2,
                        kernel::proc::ProcState::Suspended => 3,
                        _ => 1,
                    };
                    info.priority = slot.priority as u8;
                    info.memory_bytes = slot.mem_bytes;
                    info.name = [0u8; 12];
                    let name_bytes = slot.name.as_bytes();
                    let copy_len = name_bytes.len().min(12);
                    info.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                    info.runtime_ticks = slot.ledger.cpu_ticks;
                    info.reserved = [0u8; 10];
                }
                return 0;
            }
            match_idx += 1;
        }
    }
    1
}

pub unsafe extern "C" fn api_sys_proc_kill(pid: u8) -> u8 {
    let table = crate::orchestrator::proc_table_mut();
    if let Some(pcb) = table.find_mut(kernel::proc::Pid(pid)) {
        if pcb.app_id < crate::AppRegistry::count() {
            // Cannot kill core firmware app
            return 1;
        }
        pcb.state = kernel::proc::ProcState::Dead;
        crate::apps::AppRegistry::unregister_dynamic();
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn api_paged_call(
    target_block_id: u8,
    target_offset: u16,
    return_offset: u16,
) -> usize {
    super::native_runner::krx_call_paged(target_block_id, target_offset, return_offset).unwrap_or(0)
}

pub unsafe extern "C" fn api_paged_return() -> usize {
    super::native_runner::krx_return_thunk().unwrap_or(0)
}

pub unsafe extern "C" fn api_leaf_call(leaf_id: u8, arg: u32) -> u32 {
    super::native_runner::krx_leaf_call(leaf_id, arg)
}

pub unsafe extern "C" fn api_lease_acquire(lease_kind: u8, timeout_ms: u32) -> u8 {
    if let Some(kind) = kernel::proc::LeaseKind::from_u8(lease_kind) {
        let pid = kernel::proc::Pid(crate::apps::AppRegistry::count() as u8);
        let tick = py32_hal::delay::millis();
        if crate::orchestrator::lease_manager_mut()
            .acquire(pid, kind, timeout_ms, tick)
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

pub unsafe extern "C" fn api_lease_release(lease_kind: u8) -> u8 {
    if let Some(kind) = kernel::proc::LeaseKind::from_u8(lease_kind) {
        let pid = kernel::proc::Pid(crate::apps::AppRegistry::count() as u8);
        if crate::orchestrator::lease_manager_mut()
            .release(pid, kind)
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

pub unsafe extern "C" fn api_mem_query(
    kind: u8,
    out_desc: *mut kernel::proc::MemoryRegionDescriptor,
) -> u8 {
    if out_desc.is_null() {
        return 1;
    }
    let slot = super::native_runner::slot_ref();
    if !slot.is_loaded {
        return 1;
    }
    if let Some(rk) = kernel::proc::RegionKind::from_u8(kind) {
        if let Some(desc) = slot.mem_layout.find_region_by_kind(rk) {
            unsafe {
                *out_desc = *desc;
            }
            return 0;
        }
    }
    1
}

// ── 13. CapHwBus C-ABI Handlers ─────────────────────────────────────────────

pub unsafe extern "C" fn api_hw_i2c_transfer(
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

pub unsafe extern "C" fn api_hw_spi_transfer(
    cs_pin: u8,
    tx: *const u8,
    rx: *mut u8,
    len: u16,
) -> u8 {
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

pub unsafe extern "C" fn api_hw_gpio_set(pin: u8, high: u8) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::HardwarePrimitive;
            (*ACTIVE_CTX).gpio_write(pin, high != 0);
        }
    }
}

pub unsafe extern "C" fn api_hw_gpio_get(pin: u8) -> u8 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::HardwarePrimitive;
            if (*ACTIVE_CTX).gpio_read(pin) { 1 } else { 0 }
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_hw_rf_read_reg(reg: u8) -> u16 {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).read_register(reg)
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_hw_rf_write_reg(reg: u8, val: u16) {
    unsafe {
        if !ACTIVE_CTX.is_null() {
            use kernel::proc::primitives::RadioPrimitive;
            (*ACTIVE_CTX).write_register(reg, val);
        }
    }
}

// ── Immutable Per-Capability Domain VTables in XIP Flash (.rodata) ──────────

pub static DISPLAY_VTABLE: CapDisplayVTable = CapDisplayVTable {
    clear: api_display_clear,
    draw_pixel: api_draw_pixel,
    draw_line: api_draw_line,
    draw_rect: api_draw_rect,
    fill_rect: api_fill_rect,
    draw_bitmap: api_draw_bitmap,
    draw_str: api_draw_str,
    draw_str_align: api_draw_str_align,
    commit_frame: api_commit_frame,
    commit_rows: api_commit_rows,
    get_framebuffer: api_get_framebuffer,
};

pub static RADIO_VTABLE: CapRadioVTable = CapRadioVTable {
    rf_tune: api_rf_tune,
    rf_get_freq: api_rf_get_freq,
    rf_get_rssi: api_rf_get_rssi,
    rf_is_carrier: api_rf_is_carrier,
    rf_set_modulation: api_rf_set_modulation,
    rf_set_squelch: api_rf_set_squelch,
    rf_set_tx: api_rf_set_tx,
    rf_set_tx_power: api_rf_set_tx_power,
    rf_set_af_mode: api_rf_set_af_mode,
    rf_sweep: api_rf_sweep,
    rf_symbol_tx_emit: api_rf_symbol_tx_emit,
    rf_set_dual_tones: api_rf_set_dual_tones,
    rf_fsk_init: api_rf_fsk_init,
    rf_fsk_send: api_rf_fsk_send,
    rf_fsk_recv: api_rf_fsk_recv,
    rf_set_mic_mute: api_rf_set_mic_mute,
    rf_set_tx_carrier: api_rf_set_tx_carrier,
};

pub static AUDIO_VTABLE: CapAudioVTable = CapAudioVTable {
    audio_play_tone: api_audio_play_tone,
    audio_play_beep: api_audio_play_beep,
    audio_set_path: api_audio_set_path,
    audio_capture_start: api_audio_capture_start,
    audio_capture_read: api_audio_capture_read,
    audio_capture_stop: api_audio_capture_stop,
};

pub static STORAGE_VTABLE: CapStorageVTable = CapStorageVTable {
    storage_load_config: api_storage_load_config,
    storage_save_config: api_storage_save_config,
    storage_read_asset: api_storage_read_asset,
    storage_read_file: api_storage_read_file,
    storage_write_file: api_storage_write_file,
    storage_remove_file: api_storage_remove_file,
    storage_file_size: api_storage_file_size,
    storage_list_dir: api_storage_list_dir,
};

pub static KEYPAD_VTABLE: CapKeypadVTable = CapKeypadVTable {
    sys_poll_key: api_sys_poll_key,
    sys_nav_dir: api_sys_nav_dir,
    sys_key_matrix: api_sys_key_matrix,
};

pub static POWER_VTABLE: CapPowerVTable = CapPowerVTable {
    sys_battery_mv: api_sys_battery_mv,
    sys_battery_pct: api_sys_battery_pct,
    sys_battery_ma: api_sys_battery_ma,
    sys_battery_type: api_sys_battery_type,
    sys_raw_battery_adc: api_sys_raw_battery_adc,
    sys_mcu_temp: api_sys_mcu_temp,
    sys_is_charging: api_sys_is_charging,
    sys_is_low: api_sys_is_low,
    sys_is_critical: api_sys_is_critical,
    sys_set_backlight: api_sys_set_backlight,
    sys_set_backlight_brightness: api_sys_set_backlight_brightness,
    sys_wake_backlight: api_sys_wake_backlight,
};

pub static SERIAL_VTABLE: CapSerialVTable = CapSerialVTable {
    serial_write: api_serial_write,
    serial_read: api_serial_read,
    serial_is_connected: api_serial_is_connected,
};

pub static TIMER_VTABLE: CapTimerVTable = CapTimerVTable {
    sys_ticks_ms: api_sys_ticks_ms,
    sys_uptime_secs: api_sys_uptime_secs,
    sys_delay_ms: api_sys_delay_ms,
};

pub static UI_VTABLE: CapUiVTable = CapUiVTable {
    ui_close_modal: api_ui_close_modal,
    ui_has_modal: api_ui_has_modal,
};

pub static MATH_VTABLE: CapMathVTable = CapMathVTable {
    sys_rand32: api_sys_rand32,
    crc32: api_math_crc32,
    crc16: api_math_crc16,
    idivmod: api_math_idivmod,
    uidivmod: api_math_uidivmod,
};

pub static FM_VTABLE: CapFmVTable = CapFmVTable {
    fm_tune: api_fm_tune,
    fm_set_volume: api_fm_set_volume,
    fm_mute: api_fm_mute,
    fm_get_rssi: api_fm_get_rssi,
};

pub static SERVICE_VTABLE: CapServiceVTable = CapServiceVTable {
    svc_register: api_svc_register,
    svc_resolve: api_svc_resolve,
    svc_release: api_svc_release,
    ipc_send: api_ipc_send,
    ipc_poll: api_ipc_poll,
    sys_device_uid: api_sys_device_uid,
    sys_os_version: api_sys_os_version,
    sys_ram_available: api_sys_ram_available,
    sys_stack_margin: api_sys_stack_margin,
    sys_proc_count: api_sys_proc_count,
    sys_proc_info: api_sys_proc_info,
    sys_proc_kill: api_sys_proc_kill,
    paged_call: api_paged_call,
    paged_return: api_paged_return,
    leaf_call: api_leaf_call,
    lease_acquire: api_lease_acquire,
    lease_release: api_lease_release,
    mem_query: api_mem_query,
};

pub static HW_BUS_VTABLE: CapHwBusVTable = CapHwBusVTable {
    hw_i2c_transfer: api_hw_i2c_transfer,
    hw_spi_transfer: api_hw_spi_transfer,
    hw_gpio_set: api_hw_gpio_set,
    hw_gpio_get: api_hw_gpio_get,
    hw_rf_read_reg: api_hw_rf_read_reg,
    hw_rf_write_reg: api_hw_rf_write_reg,
};

pub static NATIVE_API_TABLE: KrxApiTable = KrxApiTable {
    abi_version: KRX_ABI_VERSION_3_0,
    table_size: core::mem::size_of::<KrxApiTable>() as u16,
    granted_caps: 0xFFFF_FFFF,
    display: &raw const DISPLAY_VTABLE,
    radio: &raw const RADIO_VTABLE,
    audio: &raw const AUDIO_VTABLE,
    storage: &raw const STORAGE_VTABLE,
    keypad: &raw const KEYPAD_VTABLE,
    power: &raw const POWER_VTABLE,
    serial: &raw const SERIAL_VTABLE,
    timer: &raw const TIMER_VTABLE,
    ui: &raw const UI_VTABLE,
    math: &raw const MATH_VTABLE,
    fm: &raw const FM_VTABLE,
    service: &raw const SERVICE_VTABLE,
    hw_bus: &raw const HW_BUS_VTABLE,
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

/// Normative Capability & Service Resolution Broker (KRX-ABI 2.5.0 Section 8.1).
/// Resolution order: preferred provider (`PIN_PROVIDER`) -> kernel primitive registry -> service registry.
#[inline(never)]
pub fn resolve_capability_vtable(
    interface_id: u16,
    min_abi: u16,
    granted_caps: CapabilitySet,
) -> Option<usize> {
    // 1. Check ServiceRegistry first for pinned overrides or package service providers (0x0100+)
    if let Some(ptr) = service_registry_mut().resolve(interface_id, min_abi) {
        return Some(ptr);
    }

    // Kernel primitives support ABI level 1
    if min_abi > 1 {
        return None;
    }

    // 2. Kernel Primitive Registry (0x0001..=0x000D)
    match interface_id {
        INTERFACE_DISPLAY if granted_caps.contains(Cap::DisplayDirect) => {
            Some(&raw const DISPLAY_VTABLE as usize)
        }
        INTERFACE_RADIO
            if granted_caps.contains(Cap::RadioRx) || granted_caps.contains(Cap::RadioTx) =>
        {
            Some(&raw const RADIO_VTABLE as usize)
        }
        INTERFACE_AUDIO
            if granted_caps.contains(Cap::AudioOutput)
                || granted_caps.contains(Cap::AudioCapture) =>
        {
            Some(&raw const AUDIO_VTABLE as usize)
        }
        INTERFACE_STORAGE
            if granted_caps.contains(Cap::StorageRead)
                || granted_caps.contains(Cap::StorageWrite) =>
        {
            Some(&raw const STORAGE_VTABLE as usize)
        }
        INTERFACE_KEYPAD if granted_caps.contains(Cap::Keypad) => {
            Some(&raw const KEYPAD_VTABLE as usize)
        }
        INTERFACE_POWER if granted_caps.contains(Cap::PowerManage) => {
            Some(&raw const POWER_VTABLE as usize)
        }
        INTERFACE_SERIAL
            if granted_caps.contains(Cap::SerialComm) || granted_caps.contains(Cap::UsbComm) =>
        {
            Some(&raw const SERIAL_VTABLE as usize)
        }
        INTERFACE_TIMER => Some(&raw const TIMER_VTABLE as usize),
        INTERFACE_UI if granted_caps.contains(Cap::DisplayDirect) => {
            Some(&raw const UI_VTABLE as usize)
        }
        INTERFACE_MATH => Some(&raw const MATH_VTABLE as usize),
        INTERFACE_FM if granted_caps.contains(Cap::BroadcastFm) => {
            Some(&raw const FM_VTABLE as usize)
        }
        INTERFACE_SERVICE => Some(&raw const SERVICE_VTABLE as usize),
        INTERFACE_HW_BUS if granted_caps.contains(Cap::RawPeripheral) => {
            Some(&raw const HW_BUS_VTABLE as usize)
        }
        _ => None,
    }
}

/// Bind `SECTION_CAP_IMPORTS (0x0003)` into the loaded package's GOT slots (Appendix C).
/// Returns `false` if any `FLAG_MANDATORY` import fails to resolve.
#[inline(never)]
pub fn bind_cap_imports(
    cap_section_body: &[u8],
    got_words: &mut [u32],
    granted_caps: CapabilitySet,
) -> bool {
    if cap_section_body.is_empty() {
        return true;
    }
    let stride = if cap_section_body.len() % 8 == 0 && cap_section_body.len() % 6 != 0 {
        CapBinding::SIZE_WITH_SLOT
    } else {
        CapBinding::SIZE
    };
    let count = cap_section_body.len() / stride;
    for i in 0..count {
        let slice = &cap_section_body[i * stride..];
        let Some(binding) = CapBinding::parse(slice, i as u16) else {
            return false;
        };
        let slot = binding.slot_idx as usize;
        if slot >= got_words.len() {
            return false;
        }
        match resolve_capability_vtable(binding.interface_id, binding.min_abi, granted_caps) {
            Some(vtable_addr) => {
                got_words[slot] = vtable_addr as u32;
            }
            None => {
                if (binding.flags & CapBinding::FLAG_MANDATORY) != 0 {
                    return false;
                }
                got_words[slot] = 0; // NULL for OPTIONAL binding
            }
        }
    }
    true
}
