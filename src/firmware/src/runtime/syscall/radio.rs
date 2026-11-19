//! Radio & Broadcast FM Subsystem System Call Dispatcher (`Op::Rf*`, domain `0x4`).

#[cfg(test)]
use super::active_ctx_opt;
use super::{ptr_slice, ptr_slice_mut};
use crate::services::primitives;
use crate::services::radio::RadioCoordinator;
use drivers::bk4829::{Bk4829, Disabled};
use kernel::proc::abi::{KrxRadioState, KrxSubaudible, KrxSweepReq, KrxSymbol, KrxVfoConfig};
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

#[cfg(feature = "fm_radio")]
#[inline(always)]
fn fm_tuner() -> drivers::bk1080::Bk1080<py32_hal::i2c::BitbangI2c> {
    drivers::bk1080::Bk1080::new(py32_hal::i2c::BitbangI2c::new(
        bsp::pins::FM_SCL,
        bsp::pins::FM_SDA,
    ))
}

#[inline(never)]
fn active_krx_token(exclusive: bool) -> kernel::service::radio::ClientToken {
    use kernel::service::radio::{ClientId, ClientToken, LockTier};
    let slot = crate::runtime::native_runner::slot_ref();
    let id = if slot.is_loaded && slot.pid.0 != 0 {
        ClientId::for_krx_pid(slot.pid.0)
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
unsafe fn dispatch_rf_vfo(sub: u8, a1: usize, a2: usize) -> usize {
    match sub {
        0 => {
            let Some(vfo) = decode_vfo_slot(a1 as u8) else {
                return 1;
            };
            let out = a2 as *mut KrxVfoConfig;
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
        1 => {
            let cfg_ptr = a2 as *const KrxVfoConfig;
            let (Some(vfo), false) = (decode_vfo_slot(a1 as u8), cfg_ptr.is_null()) else {
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
                ch.rx_subaudible =
                    decode_subaudible(cfg.rx_subaudible.kind, cfg.rx_subaudible.value);
                ch.tx_subaudible =
                    decode_subaudible(cfg.tx_subaudible.kind, cfg.tx_subaudible.value);
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
        2 => {
            let out = a1 as *mut KrxRadioState;
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
        3 => {
            use crate::services::radio::coordinator::{CrossBandMode, DualWatchMode};
            let active_vfo = a1 as u8;
            let dual_watch = (a1 >> 8) as u8;
            let cross_band = (a1 >> 16) as u8;
            let bclo = (a1 >> 24) as u8;
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
        4 => {
            let slot = (a1 >> 16) as u8;
            let exclusive = ((a1 >> 24) as u8) != 0;
            let Some(vfo) = decode_vfo_slot(slot) else {
                return 1;
            };
            with_radio(1, |coord, _| {
                let res = RfResourceMask(a1 as u8);
                let token = active_krx_token(exclusive);
                usize::from(!coord.request_resource_lock(token, res, vfo, a2 as u16, false))
            })
        }
        _ => 1,
    }
}

#[inline(never)]
unsafe fn dispatch_rf_bulk(sub: u8, a1: usize, a2: usize) -> usize {
    match sub {
        0 => {
            let req = a1 as *const KrxSweepReq;
            if req.is_null() {
                return 0;
            }
            let r = unsafe { &*req };
            let Some(bytes) =
                (unsafe { ptr_slice_mut(r.out_rssi as *mut u8, (r.count as usize) * 2) })
            else {
                return 0;
            };
            let slice = unsafe {
                core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut i16, r.count as usize)
            };
            with_radio(0, |_, radio| {
                primitives::rf_sweep(radio, r.start_hz, r.step_hz, slice)
            })
        }
        1 => {
            let count = a2 as u16;
            let Some(bytes) = (unsafe { ptr_slice(a1 as *const u8, (count as usize) * 4) }) else {
                return 1;
            };
            let slice = unsafe {
                core::slice::from_raw_parts(bytes.as_ptr() as *const KrxSymbol, count as usize)
            };
            with_radio(1, |_, radio| {
                usize::from(primitives::rf_emit_symbols(radio, slice).is_err())
            })
        }
        #[cfg(feature = "rf_fsk")]
        2 => {
            with_radio((), |_, radio| primitives::rf_fsk_init(radio, a1 as u16));
            0
        }
        #[cfg(feature = "rf_fsk")]
        3 => {
            let Some(slice) = (unsafe { ptr_slice(a1 as *const u8, (a2 as u16) as usize) }) else {
                return 1;
            };
            with_radio(1, |_, radio| {
                usize::from(primitives::rf_fsk_send(radio, slice).is_err())
            })
        }
        #[cfg(feature = "rf_fsk")]
        4 => {
            let Some(slice) = (unsafe { ptr_slice_mut(a1 as *mut u8, (a2 as u16) as usize) })
            else {
                return 0;
            };
            with_radio(0, |_, radio| primitives::rf_fsk_receive(radio, slice))
        }
        #[cfg(not(feature = "rf_fsk"))]
        3 => 1,
        _ => 0,
    }
}

#[inline(never)]
pub unsafe fn dispatch_rf(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    match idx {
        // RfTune: a0 = freq_hz, a1 = bw, a2 = step_hz
        0 => {
            let bw_hz = if (a1 as u8) == 1 { 25_000 } else { 12_500 };
            with_radio(1, |coord, radio| {
                primitives::rf_tune(coord, radio, a0 as u32, bw_hz, a2 as u32);
                0
            })
        }
        // RfGet: 0=freq,1=rssi,2=carrier,3=bw,4=mod,5=tx,6=glitch,7=snr,8=noise,9=fm_rssi
        1 => {
            let sub = a0 as u8;
            match sub {
                0 | 3 | 4 | 5 => {
                    let coord = &crate::radio_state_ref().coord;
                    let ch = coord.active_channel();
                    match sub {
                        0 => ch.rx_freq.raw() as usize,
                        3 => ch.bandwidth.in_hz() as usize,
                        4 => ch.modulation as usize,
                        _ => coord.is_transmitting as usize,
                    }
                }
                1 => with_radio(-130i16, |_, radio| radio.get_rssi_dbm()) as i32 as u32 as usize,
                2 => with_radio(0, |coord, radio| {
                    usize::from(coord.carrier_active || radio.is_carrier_open())
                }),
                6..=8 => {
                    let reg = match sub {
                        6 => 0x63,
                        7 => 0x66,
                        _ => 0x65,
                    };
                    with_radio(0, |_, radio| (radio.read_reg(reg) & 0xFF) as usize)
                }
                9 => {
                    #[cfg(feature = "fm_radio")]
                    {
                        fm_tuner().get_rssi() as usize
                    }
                    #[cfg(not(feature = "fm_radio"))]
                    {
                        0
                    }
                }
                _ => 0,
            }
        }
        // RfSet
        2 => {
            let sub = a0 as u8;
            match sub {
                0..=5 => {
                    with_radio((), |coord, radio| {
                        primitives::rf_set_param(coord, radio, sub, a1 as u8);
                    });
                    0
                }
                6 => with_radio(1, |coord, radio| {
                    if a1 != 0 {
                        let perm = coord.request_ptt_start(radio, active_krx_token(false));
                        if perm.is_granted() {
                            0
                        } else {
                            (perm as usize) + 1
                        }
                    } else {
                        coord.request_ptt_stop(radio);
                        0
                    }
                }),
                7 => {
                    with_radio((), |_, radio| radio.send_css_tail(CssTailMode::Tone55Hz));
                    0
                }
                8 => {
                    let mask = a1 as u8;
                    let kind = (a1 >> 8) as u8;
                    with_radio(1, |coord, radio| {
                        primitives::rf_apply_subaudible_split(
                            coord,
                            radio,
                            (mask & 1) != 0,
                            (mask & 2) != 0,
                            decode_subaudible(kind, a2 as u16),
                        );
                        0
                    })
                }
                9 => {
                    with_radio((), |_, radio| {
                        primitives::rf_set_dtmf(radio, a1 as u8, a2 as u8);
                    });
                    0
                }
                10 => with_radio(1, |coord, radio| {
                    let dur = a2 as u16;
                    if dur == 0 {
                        coord.stop_1750hz_burst(radio);
                        0
                    } else {
                        usize::from(!coord.trigger_1750hz_burst(radio, dur))
                    }
                }),
                11 => with_radio(1, |coord, radio| {
                    let id = active_krx_token(false).id;
                    usize::from(!coord.release_client_lock(radio, id))
                }),
                #[cfg(feature = "fm_radio")]
                12 => {
                    let _ = fm_tuner().set_frequency_10khz((a1 as u16).saturating_mul(10));
                    0
                }
                #[cfg(feature = "fm_radio")]
                13 => {
                    let _ = fm_tuner().set_volume(a1 as u8);
                    0
                }
                #[cfg(feature = "fm_radio")]
                14 => {
                    let _ = fm_tuner().set_mute(a1 != 0);
                    0
                }
                _ => 0,
            }
        }
        // RfTones: a0 = is_hz, a1 = t1 | (g1<<16), a2 = t2 | (g2<<16)
        3 => {
            let t1 = a1 as u16;
            let g1 = (a1 >> 16) as u8;
            let t2 = a2 as u16;
            let g2 = (a2 >> 16) as u8;
            with_radio((), |_, radio| {
                if a0 != 0 {
                    primitives::rf_set_dual_tones_hz(radio, t1, g1, t2, g2);
                } else {
                    radio.set_dual_tones_raw(t1, g1, t2, g2);
                }
            });
            0
        }
        4 => unsafe { dispatch_rf_vfo(a0 as u8, a1, a2) },
        5 => unsafe { dispatch_rf_bulk(a0 as u8, a1, a2) },
        _ => 0,
    }
}

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

        assert_eq!(core::mem::size_of::<KrxVfoConfig>(), 28);
        assert_eq!(core::mem::size_of::<KrxRadioState>(), 16);
    }
}
