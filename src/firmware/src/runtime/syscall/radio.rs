//! Radio & Broadcast FM Subsystem System Call Handlers (`RadioApi`, `FmApi`).

#[cfg(test)]
use super::active_ctx_opt;
use crate::services::primitives;
use crate::services::radio::RadioCoordinator;
use drivers::bk4829::{Bk4829, Disabled};
use kernel::proc::abi::{FmApi, KrxRadioState, KrxSubaudible, KrxSymbol, KrxVfoConfig, RadioApi};
use kernel::service::radio::{
    Bandwidth, BcloPolicy, CssTailMode, ModulationMode, RepeaterShift, RfChannel, RfPowerLevel,
    RfResourceMask, RfSquelch, RfSubaudible, RfToneGenerator, StepSetting, SubaudibleTone, VfoSlot,
};

#[inline(always)]
fn with_radio<R>(
    _default: R,
    f: impl FnOnce(&mut RadioCoordinator, &mut Bk4829<'_, Disabled>) -> R,
) -> R {
    #[cfg(not(test))]
    {
        f(&mut crate::radio_state().coord, &mut crate::kern_hw().radio)
    }
    #[cfg(test)]
    {
        if let Some(ctx) = active_ctx_opt() {
            f(&mut ctx.state.coord, ctx.radio)
        } else {
            _default
        }
    }
}

#[inline(never)]
fn coord_u32(op: u8) -> u32 {
    let coord = &crate::radio_state_ref().coord;
    let ch = coord.active_channel();
    match op {
        0 => ch.rx_freq.raw(),
        1 => ch.bandwidth.in_hz(),
        2 => ch.modulation as u32,
        _ => coord.is_transmitting as u32,
    }
}

#[inline(never)]
fn rf_set_u8(op: u8, val: u8) {
    with_radio((), |coord, radio| {
        primitives::rf_set_param(coord, radio, op, val);
    });
}

#[inline(never)]
fn rf_get_i16(op: u8) -> i16 {
    with_radio(if op == 0 { -130 } else { 0 }, |coord, radio| match op {
        0 => radio.get_rssi_dbm(),
        1 => (coord.carrier_active || radio.is_carrier_open()) as i16,
        _ => {
            let reg = match op {
                2 => 0x63,
                3 => 0x66,
                _ => 0x65,
            };
            (radio.read_reg(reg) & 0xFF) as i16
        }
    })
}

pub unsafe extern "C" fn api_rf_tune(freq_hz: u32, bw: u8, step_hz: u32) -> u8 {
    let bw_hz = if bw == 1 { 25_000 } else { 12_500 };
    with_radio(1, |coord, radio| {
        primitives::rf_tune(coord, radio, freq_hz, bw_hz, step_hz);
        0
    })
}

pub unsafe extern "C" fn api_rf_get_freq() -> u32 {
    coord_u32(0)
}

pub unsafe extern "C" fn api_rf_get_rssi() -> i16 {
    rf_get_i16(0)
}

pub unsafe extern "C" fn api_rf_is_carrier() -> u8 {
    rf_get_i16(1) as u8
}

pub unsafe extern "C" fn api_rf_set_modulation(mod_type: u8) {
    rf_set_u8(0, mod_type);
}

pub unsafe extern "C" fn api_rf_set_squelch(level: u8) {
    rf_set_u8(1, level);
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
    with_radio(1, |coord, radio| {
        if active != 0 {
            let perm = coord.request_ptt_start(radio, active_krx_token(false));
            if perm.is_granted() {
                0
            } else {
                (perm as u8) + 1
            }
        } else {
            coord.request_ptt_stop(radio);
            0
        }
    })
}

pub unsafe extern "C" fn api_rf_set_tx_power(level: u8) {
    rf_set_u8(2, level);
}

pub unsafe extern "C" fn api_rf_set_af_mode(mode: u8) {
    rf_set_u8(3, mode);
}

pub unsafe extern "C" fn api_rf_sweep(
    start_hz: u32,
    step_hz: u32,
    count: u16,
    out_rssi: *mut i16,
) -> u16 {
    let Some(bytes) = (unsafe { super::ptr_slice_mut(out_rssi as *mut u8, (count as usize) * 2) })
    else {
        return 0;
    };
    let slice =
        unsafe { core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut i16, count as usize) };
    with_radio(0, |_, radio| {
        primitives::rf_sweep(radio, start_hz, step_hz, slice) as u16
    })
}

pub unsafe extern "C" fn api_rf_symbol_tx_emit(symbols: *const KrxSymbol, count: u16) -> u8 {
    let Some(bytes) = (unsafe { super::ptr_slice(symbols as *const u8, (count as usize) * 4) })
    else {
        return 1;
    };
    let slice =
        unsafe { core::slice::from_raw_parts(bytes.as_ptr() as *const KrxSymbol, count as usize) };
    with_radio(1, |_, radio| {
        u8::from(primitives::rf_emit_symbols(radio, slice).is_err())
    })
}

pub unsafe extern "C" fn api_rf_set_dual_tones(
    tone1_word: u16,
    tone1_gain: u8,
    tone2_word: u16,
    tone2_gain: u8,
) {
    with_radio((), |_, radio| {
        radio.set_dual_tones_raw(tone1_word, tone1_gain, tone2_word, tone2_gain);
    });
}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_init(baud: u16) {
    with_radio((), |_, radio| primitives::rf_fsk_init(radio, baud));
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_init(_baud: u16) {}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_send(data: *const u8, len: u16) -> u8 {
    let Some(slice) = (unsafe { super::ptr_slice(data, len as usize) }) else {
        return 1;
    };
    with_radio(1, |_, radio| {
        u8::from(primitives::rf_fsk_send(radio, slice).is_err())
    })
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_send(_data: *const u8, _len: u16) -> u8 {
    1
}

#[cfg(feature = "rf_fsk")]
pub unsafe extern "C" fn api_rf_fsk_recv(buf: *mut u8, max_len: u16) -> u16 {
    let Some(slice) = (unsafe { super::ptr_slice_mut(buf, max_len as usize) }) else {
        return 0;
    };
    with_radio(0, |_, radio| {
        primitives::rf_fsk_receive(radio, slice) as u16
    })
}
#[cfg(not(feature = "rf_fsk"))]
pub unsafe extern "C" fn api_rf_fsk_recv(_buf: *mut u8, _max_len: u16) -> u16 {
    0
}

pub unsafe extern "C" fn api_rf_set_mic_mute(muted: u8) {
    rf_set_u8(4, muted);
}

pub unsafe extern "C" fn api_rf_set_subaudible_ctcss(freq_tenths_hz: u16) {
    let _ = set_subaudible_mask(3, 1, freq_tenths_hz);
}

pub unsafe extern "C" fn api_rf_set_subaudible_dcs(code: u16, inverted: u8) {
    let _ = set_subaudible_mask(3, 2 + u8::from(inverted != 0), code);
}

pub unsafe extern "C" fn api_rf_send_tail() {
    with_radio((), |_, radio| {
        radio.send_css_tail(CssTailMode::Tone55Hz);
    });
}

pub unsafe extern "C" fn api_rf_get_bandwidth() -> u32 {
    coord_u32(1)
}

pub unsafe extern "C" fn api_rf_get_modulation() -> u8 {
    coord_u32(2) as u8
}

pub unsafe extern "C" fn api_rf_get_tx_state() -> u8 {
    coord_u32(3) as u8
}

pub unsafe extern "C" fn api_rf_get_glitch() -> u8 {
    rf_get_i16(2) as u8
}

pub unsafe extern "C" fn api_rf_set_agc(table_idx: u8) {
    rf_set_u8(5, table_idx);
}

pub unsafe extern "C" fn api_rf_set_dual_tones_hz(
    tone1_hz: u16,
    tone1_gain: u8,
    tone2_hz: u16,
    tone2_gain: u8,
) {
    with_radio((), |_, radio| {
        primitives::rf_set_dual_tones_hz(radio, tone1_hz, tone1_gain, tone2_hz, tone2_gain);
    });
}

pub unsafe extern "C" fn api_rf_set_dtmf(digit: u8, gain: u8) {
    with_radio((), |_, radio| {
        primitives::rf_set_dtmf(radio, digit, gain);
    });
}

pub unsafe extern "C" fn api_rf_get_snr() -> u8 {
    rf_get_i16(3) as u8
}

pub unsafe extern "C" fn api_rf_get_noise() -> u8 {
    rf_get_i16(4) as u8
}

#[inline(never)]
fn decode_vfo_slot(slot: u8) -> Option<VfoSlot> {
    match slot {
        0 => Some(VfoSlot::VfoA),
        1 => Some(VfoSlot::VfoB),
        2 => Some(VfoSlot::VfoC),
        _ => None,
    }
}

#[inline(never)]
fn encode_subaudible(tone: SubaudibleTone) -> KrxSubaudible {
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
fn decode_subaudible(kind: u8, value: u16) -> SubaudibleTone {
    if value == 0 {
        return SubaudibleTone::None;
    }
    match kind {
        1 => SubaudibleTone::Ctcss(value),
        2 => SubaudibleTone::DcsNormal(value),
        3 => SubaudibleTone::DcsInverted(value),
        _ => SubaudibleTone::None,
    }
}

#[inline(never)]
fn set_subaudible_mask(mask: u8, kind: u8, value: u16) -> u8 {
    with_radio(1, |coord, radio| {
        primitives::rf_apply_subaudible_split(
            coord,
            radio,
            (mask & 1) != 0,
            (mask & 2) != 0,
            decode_subaudible(kind, value),
        );
        0
    })
}

pub unsafe extern "C" fn api_rf_get_vfo_config(slot: u8, out: *mut KrxVfoConfig) -> u8 {
    let Some(vfo) = decode_vfo_slot(slot) else {
        return 1;
    };
    if out.is_null() {
        return 1;
    }
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

pub unsafe extern "C" fn api_rf_set_vfo_config(slot: u8, cfg_ptr: *const KrxVfoConfig) -> u8 {
    let (Some(vfo), false) = (decode_vfo_slot(slot), cfg_ptr.is_null()) else {
        return 1;
    };
    let cfg = unsafe { &*cfg_ptr };
    with_radio(1, |coord, radio| {
        let step = StepSetting::from_hz_x10(utils::math::div10_u32(cfg.step_hz).0 as u16);
        let needs_cfg = !coord.is_transmitting && coord.listening_vfo == vfo;
        let ch = coord.vfo_channel_mut(vfo);
        ch.rx_freq = utils::units::Frequency::new(cfg.rx_freq_hz);
        let offset = utils::units::Frequency::new(cfg.tx_offset_hz);
        ch.tx_shift = match cfg.tx_shift {
            1 => RepeaterShift::Positive(offset),
            2 => RepeaterShift::Negative(offset),
            _ => RepeaterShift::Simplex,
        };
        ch.step = step;
        ch.rx_subaudible = decode_subaudible(cfg.rx_subaudible.kind, cfg.rx_subaudible.value);
        ch.tx_subaudible = decode_subaudible(cfg.tx_subaudible.kind, cfg.tx_subaudible.value);
        ch.modulation = ModulationMode::from_u8(cfg.modulation);
        ch.bandwidth = Bandwidth::from_u8(cfg.bandwidth);
        ch.power = RfPowerLevel::from_u8(cfg.power);
        ch.squelch_level = cfg.squelch_level.min(9);
        ch.reverse = cfg.reverse != 0;
        ch.busy_lock = cfg.busy_lock != 0;
        if needs_cfg {
            radio.configure_channel(ch);
        }
        0
    })
}

pub unsafe extern "C" fn api_rf_get_radio_state(out: *mut KrxRadioState) -> u8 {
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
    use crate::services::radio::coordinator::{CrossBandMode, DualWatchMode};
    with_radio(1, |coord, radio| {
        if active_vfo != 0xFF {
            let Some(vfo) = decode_vfo_slot(active_vfo) else {
                return 1;
            };
            if !coord.is_transmitting {
                coord.select_vfo(radio, vfo);
            } else {
                coord.active_vfo = vfo;
            }
        }
        if dual_watch != 0xFF {
            let priority_vfo = coord.active_vfo;
            coord.dual_watch = match dual_watch {
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
            const CROSS_BANDS: [CrossBandMode; 4] = [
                CrossBandMode::Off,
                CrossBandMode::FixedTxOnA,
                CrossBandMode::FixedTxOnB,
                CrossBandMode::MasterFollowsRx,
            ];
            let Some(&cb) = CROSS_BANDS.get(cross_band as usize) else {
                return 1;
            };
            coord.cross_band = cb;
        }
        if bclo != 0xFF {
            const BCLOS: [BcloPolicy; 3] = [
                BcloPolicy::Disabled,
                BcloPolicy::CarrierOnly,
                BcloPolicy::ToneMismatchOnly,
            ];
            let Some(&bp) = BCLOS.get(bclo as usize) else {
                return 1;
            };
            coord.bclo_policy = bp;
        }
        0
    })
}

pub unsafe extern "C" fn api_rf_set_rx_tx_subaudible(is_tx: u8, kind: u8, value: u16) -> u8 {
    set_subaudible_mask(if is_tx == 0 { 1 } else { 2 }, kind, value)
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
    with_radio(1, |coord, _| {
        let res = RfResourceMask(resources as u8);
        let token = active_krx_token(exclusive != 0);
        u8::from(!coord.request_resource_lock(token, res, vfo, timeout_ms, false))
    })
}

pub unsafe extern "C" fn api_rf_unlock_vfo() -> u8 {
    with_radio(1, |coord, radio| {
        let id = active_krx_token(false).id;
        u8::from(!coord.release_client_lock(radio, id))
    })
}

pub unsafe extern "C" fn api_rf_trigger_burst(_freq_hz: u16, duration_ms: u16) -> u8 {
    with_radio(1, |coord, radio| {
        if duration_ms == 0 {
            coord.stop_1750hz_burst(radio);
            0
        } else {
            u8::from(!coord.trigger_1750hz_burst(radio, duration_ms))
        }
    })
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

#[cfg(feature = "fm_radio")]
#[inline(always)]
fn fm_tuner() -> drivers::bk1080::Bk1080<py32_hal::i2c::BitbangI2c> {
    drivers::bk1080::Bk1080::new(py32_hal::i2c::BitbangI2c::new(
        bsp::pins::FM_SCL,
        bsp::pins::FM_SDA,
    ))
}

pub unsafe extern "C" fn api_fm_tune(freq_100khz: u16) {
    #[cfg(feature = "fm_radio")]
    {
        let _ = fm_tuner().set_frequency_10khz(freq_100khz.saturating_mul(10));
    }
    #[cfg(not(feature = "fm_radio"))]
    let _ = freq_100khz;
}

pub unsafe extern "C" fn api_fm_set_volume(volume: u8) {
    #[cfg(feature = "fm_radio")]
    {
        let _ = fm_tuner().set_volume(volume);
    }
    #[cfg(not(feature = "fm_radio"))]
    let _ = volume;
}

pub unsafe extern "C" fn api_fm_mute(mute: u8) {
    #[cfg(feature = "fm_radio")]
    {
        let _ = fm_tuner().set_mute(mute != 0);
    }
    #[cfg(not(feature = "fm_radio"))]
    let _ = mute;
}

pub unsafe extern "C" fn api_fm_get_rssi() -> u8 {
    #[cfg(feature = "fm_radio")]
    {
        fm_tuner().get_rssi()
    }
    #[cfg(not(feature = "fm_radio"))]
    {
        0
    }
}

pub static FM_VTABLE: FmApi = FmApi {
    fm_tune: api_fm_tune,
    fm_set_volume: api_fm_set_volume,
    fm_mute: api_fm_mute,
    fm_get_rssi: api_fm_get_rssi,
};
