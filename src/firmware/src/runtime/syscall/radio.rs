//! Radio & Broadcast FM Subsystem System Call Handlers (`RadioApi`, `FmApi`).

use super::active_ctx_opt;
use kernel::proc::abi::{FmApi, KrxSymbol, RadioApi};
use kernel::proc::primitives::*;

pub unsafe extern "C" fn api_rf_tune(freq_hz: u32, bw: u8, step_hz: u32) -> u8 {
    let bw_hz = if bw == 1 { 25_000 } else { 12_500 };
    if let Some(ctx) = active_ctx_opt() {
        ctx.tune(freq_hz, bw_hz, step_hz);
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn api_rf_get_freq() -> u32 {
    crate::radio_state_ref()
        .coord
        .active_channel()
        .rx_freq
        .raw()
}

pub unsafe extern "C" fn api_rf_get_rssi() -> i16 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.get_rssi_dbm()
    } else {
        -130
    }
}

pub unsafe extern "C" fn api_rf_is_carrier() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_carrier_detected() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_rf_set_modulation(mod_type: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_modulation(mod_type);
    }
}

pub unsafe extern "C" fn api_rf_set_squelch(level: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_squelch(level);
    }
}

#[inline(never)]
fn active_krx_token(exclusive: bool) -> kernel::service::radio::ClientToken {
    use kernel::service::radio::{ClientId, ClientToken, LockTier};
    let slot = crate::runtime::native_runner::slot_ref();
    let id = if slot.is_loaded && slot.pid.0 != 0 {
        ClientId::for_krx_pid(slot.pid.0 as u8)
    } else {
        ClientId::VFO_UI
    };
    let tier = if exclusive {
        LockTier::EXCLUSIVE
    } else {
        LockTier::FOREGROUND
    };
    ClientToken::new(id, tier)
}

pub unsafe extern "C" fn api_rf_set_tx(active: u8) -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if active != 0 {
            let token = active_krx_token(false);
            let perm = ctx.state.coord.request_ptt_start(ctx.radio, token);
            if perm.is_granted() {
                0
            } else {
                (perm as u8) + 1
            }
        } else {
            ctx.state.coord.request_ptt_stop(ctx.radio);
            0
        }
    } else {
        1
    }
}

pub unsafe extern "C" fn api_rf_set_tx_power(level: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_tx_power(level);
    }
}

pub unsafe extern "C" fn api_rf_set_af_mode(mode: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_af_mode(mode);
    }
}

pub unsafe extern "C" fn api_rf_sweep(
    start_hz: u32,
    step_hz: u32,
    count: u16,
    out_rssi: *mut i16,
) -> u16 {
    if !out_rssi.is_null() && count > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let slice = unsafe { core::slice::from_raw_parts_mut(out_rssi, count as usize) };
            ctx.sweep(start_hz, step_hz, slice) as u16
        } else {
            0
        }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_rf_symbol_tx_emit(symbols: *const KrxSymbol, count: u16) -> u8 {
    if !symbols.is_null() && count > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let slice = unsafe { core::slice::from_raw_parts(symbols, count as usize) };
            if ctx.emit_symbols(slice).is_ok() {
                0
            } else {
                1
            }
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
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_dual_tones_raw(tone1_word, tone1_gain, tone2_word, tone2_gain);
    }
}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_init(baud: u16) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.fsk_init(baud);
    }
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_init(_baud: u16) {}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_send(data: *const u8, len: u16) -> u8 {
    if !data.is_null() && len > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
            if ctx.fsk_send(slice).is_ok() { 0 } else { 1 }
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
        if let Some(ctx) = active_ctx_opt() {
            let slice = unsafe { core::slice::from_raw_parts_mut(buf, max_len as usize) };
            ctx.fsk_receive(slice) as u16
        } else {
            0
        }
    } else {
        0
    }
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_recv(_buf: *mut u8, _max_len: u16) -> u16 {
    0
}

pub unsafe extern "C" fn api_rf_set_mic_mute(muted: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.mute_mic(muted != 0);
    }
}

pub unsafe extern "C" fn api_rf_set_subaudible_ctcss(freq_tenths_hz: u16) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_subaudible_ctcss(freq_tenths_hz);
    }
}

pub unsafe extern "C" fn api_rf_set_subaudible_dcs(code: u16, inverted: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_subaudible_dcs(code, inverted != 0);
    }
}

pub unsafe extern "C" fn api_rf_send_tail() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.send_tail();
    }
}

pub unsafe extern "C" fn api_rf_get_bandwidth() -> u32 {
    crate::radio_state_ref()
        .coord
        .active_channel()
        .bandwidth
        .in_hz()
}

pub unsafe extern "C" fn api_rf_get_modulation() -> u8 {
    crate::radio_state_ref().coord.active_channel().modulation as u8
}

pub unsafe extern "C" fn api_rf_get_tx_state() -> u8 {
    crate::radio_state_ref().coord.is_transmitting as u8
}

pub unsafe extern "C" fn api_rf_get_glitch() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.get_glitch()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_rf_set_agc(table_idx: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_agc(table_idx);
    }
}

pub unsafe extern "C" fn api_rf_set_dual_tones_hz(
    tone1_hz: u16,
    tone1_gain: u8,
    tone2_hz: u16,
    tone2_gain: u8,
) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_dual_tones_hz(tone1_hz, tone1_gain, tone2_hz, tone2_gain);
    }
}

pub unsafe extern "C" fn api_rf_set_dtmf(digit: u8, gain: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_dtmf(digit, gain);
    }
}

pub unsafe extern "C" fn api_rf_get_snr() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.get_snr()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_rf_get_noise() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.get_noise()
    } else {
        0
    }
}

#[inline(never)]
fn decode_vfo_slot(slot: u8) -> Option<kernel::service::radio::VfoSlot> {
    use kernel::service::radio::VfoSlot;
    match slot {
        0 => Some(VfoSlot::VfoA),
        1 => Some(VfoSlot::VfoB),
        2 => Some(VfoSlot::VfoC),
        _ => None,
    }
}

fn encode_subaudible(
    tone: kernel::service::radio::SubaudibleTone,
) -> kernel::proc::abi::KrxSubaudible {
    use kernel::proc::abi::KrxSubaudible;
    use kernel::service::radio::SubaudibleTone;
    let (kind, value) = match tone {
        SubaudibleTone::None => (0, 0),
        SubaudibleTone::Ctcss(v) => (1, v),
        SubaudibleTone::DcsNormal(v) => (2, v),
        SubaudibleTone::DcsInverted(v) => (3, v),
    };
    KrxSubaudible {
        kind,
        _pad: 0,
        value,
    }
}

#[inline(never)]
fn decode_subaudible(kind: u8, value: u16) -> kernel::service::radio::SubaudibleTone {
    use kernel::service::radio::SubaudibleTone;
    match kind {
        1 if value > 0 => SubaudibleTone::Ctcss(value),
        2 => SubaudibleTone::DcsNormal(value),
        3 => SubaudibleTone::DcsInverted(value),
        _ => SubaudibleTone::None,
    }
}

pub unsafe extern "C" fn api_rf_get_vfo_config(
    slot: u8,
    out: *mut kernel::proc::abi::KrxVfoConfig,
) -> u8 {
    let Some(vfo) = decode_vfo_slot(slot) else {
        return 1;
    };
    if out.is_null() {
        return 1;
    }
    use kernel::service::radio::RepeaterShift;
    let ch = crate::radio_state_ref().coord.vfo_channel(vfo);
    let (tx_shift, tx_offset_hz) = match ch.tx_shift {
        RepeaterShift::Simplex => (0, 0),
        RepeaterShift::Positive(f) => (1, f.raw()),
        RepeaterShift::Negative(f) => (2, f.raw()),
    };
    let dst = unsafe { &mut *out };
    dst.rx_freq_hz = ch.rx_freq.raw();
    dst.tx_offset_hz = tx_offset_hz;
    dst.step_hz = ch.step.in_hz();
    dst.rx_subaudible = encode_subaudible(ch.rx_subaudible);
    dst.tx_subaudible = encode_subaudible(ch.tx_subaudible);
    dst.modulation = ch.modulation as u8;
    dst.bandwidth = ch.bandwidth as u8;
    dst.power = ch.power as u8;
    dst.tx_shift = tx_shift;
    dst.squelch_level = ch.squelch_level;
    dst.reverse = ch.reverse as u8;
    dst.busy_lock = ch.busy_lock as u8;
    dst._reserved = 0;
    0
}

pub unsafe extern "C" fn api_rf_set_vfo_config(
    slot: u8,
    cfg_ptr: *const kernel::proc::abi::KrxVfoConfig,
) -> u8 {
    let Some(vfo) = decode_vfo_slot(slot) else {
        return 1;
    };
    if cfg_ptr.is_null() {
        return 1;
    }
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    let cfg = unsafe { &*cfg_ptr };
    use kernel::service::radio::{
        Bandwidth, ModulationMode, RepeaterShift, RfChannel, RfPowerLevel, StepSetting,
    };
    use utils::units::Frequency;
    let step = StepSetting::from_hz_x10(utils::math::div10_u32(cfg.step_hz).0 as u16);
    let needs_cfg = !ctx.state.coord.is_transmitting && ctx.state.coord.listening_vfo == vfo;
    let ch = ctx.state.coord.vfo_channel_mut(vfo);
    ch.rx_freq = Frequency::new(cfg.rx_freq_hz);
    ch.tx_shift = match cfg.tx_shift {
        1 => RepeaterShift::Positive(Frequency::new(cfg.tx_offset_hz)),
        2 => RepeaterShift::Negative(Frequency::new(cfg.tx_offset_hz)),
        _ => RepeaterShift::Simplex,
    };
    ch.step = step;
    ch.rx_subaudible = decode_subaudible(cfg.rx_subaudible.kind, cfg.rx_subaudible.value);
    ch.tx_subaudible = decode_subaudible(cfg.tx_subaudible.kind, cfg.tx_subaudible.value);
    ch.modulation = ModulationMode::from_u8(cfg.modulation);
    ch.bandwidth = match cfg.bandwidth {
        0 => Bandwidth::Wide25k,
        2 => Bandwidth::Narrow6k25,
        _ => Bandwidth::Narrow12k5,
    };
    ch.power = match cfg.power {
        0 => RfPowerLevel::Low,
        1 => RfPowerLevel::Medium,
        _ => RfPowerLevel::High,
    };
    ch.squelch_level = cfg.squelch_level.min(9);
    ch.reverse = cfg.reverse != 0;
    ch.busy_lock = cfg.busy_lock != 0;
    if needs_cfg {
        ctx.radio.configure_channel(ch);
    }
    0
}

pub unsafe extern "C" fn api_rf_get_radio_state(out: *mut kernel::proc::abi::KrxRadioState) -> u8 {
    if out.is_null() {
        return 1;
    }
    use crate::services::radio::coordinator::DualWatchMode;
    let c = &crate::radio_state_ref().coord;
    let dw = match c.dual_watch {
        DualWatchMode::Off => 0,
        DualWatchMode::EqualDwell { .. } => 1,
        DualWatchMode::Priority { .. } => 2,
        DualWatchMode::Adaptive => 3,
        DualWatchMode::TripleWatch { .. } => 4,
    };
    let tot_elapsed_s = utils::math::div100_u16(c.tot_elapsed_10ms).0;
    let tx_slot = c.effective_tx_slot();
    let dst = unsafe { &mut *out };
    dst.active_vfo = c.active_vfo as u8;
    dst.listening_vfo = c.listening_vfo as u8;
    dst.dual_watch = dw;
    dst.cross_band = c.cross_band as u8;
    dst.bclo_policy = c.bclo_policy as u8;
    dst.ptt_active = c.is_transmitting as u8;
    dst.squelch_open = c.carrier_active as u8;
    dst.subaudible_matched = c.subaudible_matched as u8;
    dst.tot_locked_out = c.tot_locked_out as u8;
    dst.lock_active = c.is_vfo_locked() as u8;
    dst.tx_perm = c.check_tx_permission(tx_slot, active_krx_token(false)) as u8;
    dst._reserved = 0;
    dst.tot_remaining_s = c.tot_limit_seconds.saturating_sub(tot_elapsed_s);
    dst.rssi_dbm = c.vfo_rssi(c.listening_vfo);
    0
}

pub unsafe extern "C" fn api_rf_set_watch_mode(
    active_vfo: u8,
    dual_watch: u8,
    cross_band: u8,
    bclo: u8,
) -> u8 {
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    use crate::services::radio::coordinator::{CrossBandMode, DualWatchMode};
    use kernel::service::radio::BcloPolicy;
    if active_vfo != 0xFF {
        let Some(vfo) = decode_vfo_slot(active_vfo) else {
            return 1;
        };
        if !ctx.state.coord.is_transmitting {
            ctx.state.coord.select_vfo(ctx.radio, vfo);
        } else {
            ctx.state.coord.active_vfo = vfo;
        }
    }
    if dual_watch != 0xFF {
        let priority_vfo = ctx.state.coord.active_vfo;
        ctx.state.coord.dual_watch = match dual_watch {
            0 => DualWatchMode::Off,
            1 => DualWatchMode::EqualDwell { dwell_ms: 250 },
            2 => DualWatchMode::Priority {
                priority_vfo,
                check_interval_ms: 250,
            },
            3 => DualWatchMode::Adaptive,
            4 => DualWatchMode::TripleWatch { dwell_ms: 250 },
            _ => return 1,
        };
    }
    if cross_band != 0xFF {
        ctx.state.coord.cross_band = match cross_band {
            0 => CrossBandMode::Off,
            1 => CrossBandMode::FixedTxOnA,
            2 => CrossBandMode::FixedTxOnB,
            3 => CrossBandMode::MasterFollowsRx,
            _ => return 1,
        };
    }
    if bclo != 0xFF {
        ctx.state.coord.bclo_policy = match bclo {
            0 => BcloPolicy::Disabled,
            1 => BcloPolicy::CarrierOnly,
            2 => BcloPolicy::ToneMismatchOnly,
            _ => return 1,
        };
    }
    0
}

pub unsafe extern "C" fn api_rf_set_rx_tx_subaudible(is_tx: u8, kind: u8, value: u16) -> u8 {
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    let tone = decode_subaudible(kind, value);
    ctx.apply_subaudible_split(is_tx == 0, is_tx != 0, tone);
    0
}

pub unsafe extern "C" fn api_rf_lock_vfo(
    resources: u16,
    slot: u8,
    timeout_ms: u16,
    exclusive: u8,
) -> u8 {
    let Some(vfo) = decode_vfo_slot(slot) else {
        return 1;
    };
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    let res = kernel::service::radio::RfResourceMask(resources as u8);
    let token = active_krx_token(exclusive != 0);
    if ctx
        .state
        .coord
        .request_resource_lock(token, res, vfo, timeout_ms, false)
    {
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn api_rf_unlock_vfo() -> u8 {
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    let id = active_krx_token(false).id;
    if ctx.state.coord.release_client_lock(ctx.radio, id) {
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn api_rf_trigger_burst(_freq_hz: u16, duration_ms: u16) -> u8 {
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    if duration_ms == 0 {
        ctx.state.coord.stop_1750hz_burst(ctx.radio);
        return 0;
    }
    if ctx.state.coord.trigger_1750hz_burst(ctx.radio, duration_ms) {
        0
    } else {
        1
    }
}

pub static RADIO_VTABLE: RadioApi = RadioApi {
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
    rf_set_tx_carrier: api_rf_set_tx,
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
    rf_get_vfo_config: api_rf_get_vfo_config,
    rf_set_vfo_config: api_rf_set_vfo_config,
    rf_get_radio_state: api_rf_get_radio_state,
    rf_set_watch_mode: api_rf_set_watch_mode,
    rf_set_rx_tx_subaudible: api_rf_set_rx_tx_subaudible,
    rf_lock_vfo: api_rf_lock_vfo,
    rf_unlock_vfo: api_rf_unlock_vfo,
    rf_trigger_burst: api_rf_trigger_burst,
};

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::proc::abi::{KrxRadioState, KrxVfoConfig};
    use kernel::service::radio::{SubaudibleTone, VfoSlot};

    #[test]
    fn test_krx_vfo_slot_and_subaudible_codec() {
        assert_eq!(decode_vfo_slot(0), Some(VfoSlot::VfoA));
        assert_eq!(decode_vfo_slot(1), Some(VfoSlot::VfoB));
        assert_eq!(decode_vfo_slot(2), Some(VfoSlot::VfoC));
        assert_eq!(decode_vfo_slot(3), None);

        for tone in [
            SubaudibleTone::None,
            SubaudibleTone::Ctcss(885),
            SubaudibleTone::DcsNormal(0o023),
            SubaudibleTone::DcsInverted(0o754),
        ] {
            let enc = encode_subaudible(tone);
            assert_eq!(decode_subaudible(enc.kind, enc.value), tone);
        }

        // Verify C-ABI struct stability and bounded footprint
        assert_eq!(core::mem::size_of::<KrxVfoConfig>(), 28);
        assert_eq!(core::mem::size_of::<KrxRadioState>(), 16);
    }
}

pub unsafe extern "C" fn api_fm_tune(freq_100khz: u16) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.fm_tune(freq_100khz);
    }
}

pub unsafe extern "C" fn api_fm_set_volume(volume: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.fm_set_volume(volume);
    }
}

pub unsafe extern "C" fn api_fm_mute(mute: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.fm_mute(mute != 0);
    }
}

pub unsafe extern "C" fn api_fm_get_rssi() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.fm_get_rssi()
    } else {
        0
    }
}

pub static FM_VTABLE: FmApi = FmApi {
    fm_tune: api_fm_tune,
    fm_set_volume: api_fm_set_volume,
    fm_mute: api_fm_mute,
    fm_get_rssi: api_fm_get_rssi,
};
