//! Radio & Broadcast FM Subsystem System Call Handlers (`CapRadioVTable`, `CapFmVTable`).

use super::active_ctx;
use kernel::proc::abi::{CapFmVTable, CapRadioVTable, KrxSymbol};
use kernel::proc::primitives::*;

pub unsafe extern "C" fn api_rf_tune(freq_hz: u32, bw: u8, step_hz: u32) -> u8 {
    let bw_hz = if bw == 1 { 25_000 } else { 12_500 };
    unsafe { active_ctx().tune(freq_hz, bw_hz, step_hz) };
    0
}

pub unsafe extern "C" fn api_rf_get_freq() -> u32 {
    unsafe { active_ctx().get_frequency() }
}

pub unsafe extern "C" fn api_rf_get_rssi() -> i16 {
    unsafe { active_ctx().get_rssi_dbm() }
}

pub unsafe extern "C" fn api_rf_is_carrier() -> u8 {
    unsafe {
        if active_ctx().is_carrier_detected() {
            1
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_rf_set_modulation(mod_type: u8) {
    unsafe { active_ctx().set_modulation(mod_type) };
}

pub unsafe extern "C" fn api_rf_set_squelch(level: u8) {
    unsafe { active_ctx().set_squelch(level) };
}

pub unsafe extern "C" fn api_rf_set_tx(active: u8) -> u8 {
    unsafe { active_ctx().set_tx(active != 0) };
    0
}

pub unsafe extern "C" fn api_rf_set_tx_power(level: u8) {
    unsafe { active_ctx().set_tx_power(level) };
}

pub unsafe extern "C" fn api_rf_set_af_mode(mode: u8) {
    unsafe { active_ctx().set_af_mode(mode) };
}

pub unsafe extern "C" fn api_rf_sweep(
    start_hz: u32,
    step_hz: u32,
    count: u16,
    out_rssi: *mut i16,
) -> u16 {
    if !out_rssi.is_null() && count > 0 {
        let slice = unsafe { core::slice::from_raw_parts_mut(out_rssi, count as usize) };
        unsafe { active_ctx().sweep(start_hz, step_hz, slice) as u16 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_rf_symbol_tx_emit(symbols: *const KrxSymbol, count: u16) -> u8 {
    if !symbols.is_null() && count > 0 {
        let slice = unsafe { core::slice::from_raw_parts(symbols, count as usize) };
        if unsafe { active_ctx().emit_symbols(slice).is_ok() } {
            0
        } else {
            1
        }
    } else {
        1
    }
}

pub unsafe extern "C" fn api_rf_set_dual_tones(
    tone1_word: u16,
    tone1_gain: u8,
    tone2_word: u16,
    tone2_gain: u8,
) {
    unsafe { active_ctx().set_dual_tones_raw(tone1_word, tone1_gain, tone2_word, tone2_gain) };
}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_init(baud: u16) {
    unsafe { active_ctx().fsk_init(baud) };
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_init(_baud: u16) {}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_send(data: *const u8, len: u16) -> u8 {
    if !data.is_null() && len > 0 {
        let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
        if unsafe { active_ctx().fsk_send(slice).is_ok() } {
            0
        } else {
            1
        }
    } else {
        1
    }
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_send(_data: *const u8, _len: u16) -> u8 {
    1
}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_recv(buf: *mut u8, max_len: u16) -> u16 {
    if !buf.is_null() && max_len > 0 {
        let slice = unsafe { core::slice::from_raw_parts_mut(buf, max_len as usize) };
        unsafe { active_ctx().fsk_receive(slice) as u16 }
    } else {
        0
    }
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_recv(_buf: *mut u8, _max_len: u16) -> u16 {
    0
}

pub unsafe extern "C" fn api_rf_set_mic_mute(muted: u8) {
    unsafe { active_ctx().mute_mic(muted != 0) };
}

pub unsafe extern "C" fn api_rf_set_tx_carrier(active: u8) -> u8 {
    unsafe { active_ctx().set_tx_carrier(active != 0) };
    0
}

pub unsafe extern "C" fn api_rf_set_subaudible_ctcss(freq_tenths_hz: u16) {
    unsafe { active_ctx().set_subaudible_ctcss(freq_tenths_hz) };
}

pub unsafe extern "C" fn api_rf_set_subaudible_dcs(code: u16, inverted: u8) {
    unsafe { active_ctx().set_subaudible_dcs(code, inverted != 0) };
}

pub unsafe extern "C" fn api_rf_send_tail() {
    unsafe { active_ctx().send_tail() };
}

pub unsafe extern "C" fn api_rf_get_bandwidth() -> u32 {
    unsafe { active_ctx().get_bandwidth() }
}

pub unsafe extern "C" fn api_rf_get_modulation() -> u8 {
    unsafe { active_ctx().get_modulation() }
}

pub unsafe extern "C" fn api_rf_get_tx_state() -> u8 {
    unsafe { active_ctx().get_tx_state() }
}

pub unsafe extern "C" fn api_rf_get_glitch() -> u8 {
    unsafe { active_ctx().get_glitch() }
}

pub unsafe extern "C" fn api_rf_set_agc(table_idx: u8) {
    unsafe { active_ctx().set_agc(table_idx) };
}

pub unsafe extern "C" fn api_rf_set_dual_tones_hz(
    tone1_hz: u16,
    tone1_gain: u8,
    tone2_hz: u16,
    tone2_gain: u8,
) {
    unsafe { active_ctx().set_dual_tones_hz(tone1_hz, tone1_gain, tone2_hz, tone2_gain) };
}

pub unsafe extern "C" fn api_rf_set_dtmf(digit: u8, gain: u8) {
    unsafe { active_ctx().set_dtmf(digit, gain) };
}

pub unsafe extern "C" fn api_rf_get_snr() -> u8 {
    unsafe { active_ctx().get_snr() }
}

pub unsafe extern "C" fn api_rf_get_noise() -> u8 {
    unsafe { active_ctx().get_noise() }
}

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
    rf_set_subaudible_ctcss: api_rf_set_subaudible_ctcss,
    rf_set_subaudible_dcs: api_rf_set_subaudible_dcs,
    rf_send_tail: api_rf_send_tail,
    rf_get_bandwidth: api_rf_get_bandwidth,
    rf_get_modulation: api_rf_get_modulation,
    rf_get_tx_state: api_rf_get_tx_state,
    rf_get_glitch: api_rf_get_glitch,
    rf_set_agc: api_rf_set_agc,
    rf_set_dual_tones_hz: api_rf_set_dual_tones_hz,
    rf_set_dtmf: api_rf_set_dtmf,
    rf_get_snr: api_rf_get_snr,
    rf_get_noise: api_rf_get_noise,
};

pub unsafe extern "C" fn api_fm_tune(freq_100khz: u16) {
    unsafe { active_ctx().fm_tune(freq_100khz) };
}

pub unsafe extern "C" fn api_fm_set_volume(volume: u8) {
    unsafe { active_ctx().fm_set_volume(volume) };
}

pub unsafe extern "C" fn api_fm_mute(mute: u8) {
    unsafe { active_ctx().fm_mute(mute != 0) };
}

pub unsafe extern "C" fn api_fm_get_rssi() -> u8 {
    unsafe { active_ctx().fm_get_rssi() }
}

pub static FM_VTABLE: CapFmVTable = CapFmVTable {
    fm_tune: api_fm_tune,
    fm_set_volume: api_fm_set_volume,
    fm_mute: api_fm_mute,
    fm_get_rssi: api_fm_get_rssi,
};
