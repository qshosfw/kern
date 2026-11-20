//! Radio & Broadcast FM Subsystem System Call Dispatcher (`Op::Rf*`, domain `0x4`).

#[cfg(test)]
use super::active_ctx_opt;
use super::{ptr_slice, ptr_slice_mut};
use crate::data::ChannelRecord;
use crate::services::primitives;
use crate::services::radio::RadioCoordinator;
use drivers::bk4829::{Bk4829, Disabled};
use kernel::proc::abi::{KrxRadioState, KrxSweepReq, KrxSymbol};
use kernel::service::radio::{
    BcloPolicy, CssTailMode, RfResourceMask, RfSquelch, RfSubaudible, RfToneGenerator,
    SubaudibleTone, VfoSlot,
};

#[inline(always)]
fn with_radio<R>(
    _default: R,
    f: impl FnOnce(&mut RadioCoordinator, &mut Bk4829<'_, bsp::RadioBus, Disabled>) -> R,
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
fn fm_tuner() -> drivers::bk1080::Bk1080<
    py32_hal::i2c::BitbangI2c<py32_hal::gpio::Pin, py32_hal::gpio::Pin>,
    bsp::BspDelay,
> {
    drivers::bk1080::Bk1080::new(
        py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA),
        bsp::BspDelay,
    )
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

/// Result of an operation refused because the image may not key the transmitter.
pub const RF_TX_DENIED: usize = 0xFF;

/// True when the RF operation keys the transmitter and so needs `Cap::RadioTx`
/// on top of the `Cap::RadioRx` every RF operation declares.
#[inline(always)]
fn keys_transmitter(idx: u8, sub: u8, a1: usize, a2: usize) -> bool {
    use ::abi::{
        idx as op,
        sub::{rf_bulk, rf_set},
    };
    match idx {
        op::RfSet => (sub == rf_set::TX && a1 != 0) || (sub == rf_set::BURST && a2 as u16 != 0),
        op::RfBulk => sub == rf_bulk::FSK_SEND,
        _ => false,
    }
}

#[inline(always)]
fn decode_vfo_slot(slot: u8) -> Option<VfoSlot> {
    match slot {
        0 => Some(VfoSlot::VfoA),
        1 => Some(VfoSlot::VfoB),
        2 => Some(VfoSlot::VfoC),
        _ => None,
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
unsafe fn dispatch_rf_vfo(
    coord: &mut RadioCoordinator,
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    sub: u8,
    a1: usize,
    a2: usize,
) -> usize {
    use ::abi::sub::rf_vfo;
    match sub {
        // A VFO is exchanged as the ORDS-01 channel record: the same 64 bytes an app
        // reads from `/user/channels.ch`, so no attribute is lost between memory,
        // storage and the live radio.
        rf_vfo::GET_CHANNEL | rf_vfo::SET_CHANNEL => {
            let (Some(vfo), Some(bytes)) = (decode_vfo_slot(a1 as u8), unsafe {
                ptr_slice_mut(a2 as *mut u8, ChannelRecord::SIZE)
            }) else {
                return 1;
            };
            if (a2 & 3) != 0 {
                return 1;
            }
            let rec = unsafe { &mut *(bytes.as_mut_ptr() as *mut ChannelRecord) };
            if sub == rf_vfo::GET_CHANNEL {
                *rec = ChannelRecord::empty();
                coord.save_vfo_to_channel(vfo, rec);
                let memory = coord.vfo_memory_index[vfo.index().min(2)];
                return memory.map_or(0, |i| i as usize + 1) << 8;
            }
            // Bits 8..24 of `a1`: memory channel number + 1 the record came from (0 = free VFO).
            let memory = ((a1 >> 8) as u16).checked_sub(1);
            coord.load_channel_into_vfo(radio, vfo, memory, rec);
            0
        }
        rf_vfo::STATE => {
            let out = a1 as *mut KrxRadioState;
            if out.is_null() {
                return 1;
            }
            use crate::services::radio::coordinator::DualWatchMode;
            let dw = match coord.dual_watch {
                DualWatchMode::Off => 0,
                DualWatchMode::EqualDwell { .. } => 1,
                DualWatchMode::Priority { .. } => 2,
                DualWatchMode::Adaptive => 3,
                DualWatchMode::TripleWatch { .. } => 4,
            };
            let tot_elapsed_s = utils::math::div100_u16(coord.tot_elapsed_10ms).0;
            let tx_slot = coord.effective_tx_slot();
            let dst = unsafe { &mut *out };
            dst.active_vfo = coord.active_vfo as u8;
            dst.listening_vfo = coord.listening_vfo as u8;
            dst.dual_watch = dw;
            dst.cross_band = coord.cross_band as u8;
            dst.bclo_policy = coord.bclo_policy as u8;
            dst.ptt_active = coord.is_transmitting as u8;
            dst.squelch_open = coord.carrier_active as u8;
            dst.subaudible_matched = coord.subaudible_matched as u8;
            dst.tot_locked_out = coord.tot_locked_out as u8;
            dst.lock_active = coord.is_vfo_locked() as u8;
            dst.tx_perm = coord.check_tx_permission(tx_slot, active_krx_token(false)) as u8;
            dst._reserved = (coord.monitor_active as u8)
                | ((coord.fsk_sync_active as u8) << 1)
                | ((coord.pa_thermal_foldback_active as u8) << 2)
                | ((coord.dwell.is_rx_held() as u8) << 3)
                | (((coord.lock_depth() as u8) & 0x03) << 4);
            dst.tot_remaining_s = coord.tot_limit_seconds.saturating_sub(tot_elapsed_s);
            dst.rssi_dbm = coord.vfo_rssi(coord.listening_vfo);
            0
        }
        rf_vfo::WATCH_MODE => {
            use crate::services::radio::coordinator::{CrossBandMode, DualWatchMode};
            let active_vfo = a1 as u8;
            let dual_watch = (a1 >> 8) as u8;
            let cross_band = (a1 >> 16) as u8;
            let bclo = (a1 >> 24) as u8;
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
                let Some(cb) = CrossBandMode::from_u8(cross_band) else {
                    return 1;
                };
                coord.cross_band = cb;
            }
            if bclo != 0xFF {
                let Some(bp) = BcloPolicy::from_u8(bclo) else {
                    return 1;
                };
                coord.bclo_policy = bp;
            }
            0
        }
        rf_vfo::LOCK => {
            let slot = (a1 >> 16) as u8;
            let exclusive = ((a1 >> 24) as u8) != 0;
            let Some(vfo) = decode_vfo_slot(slot) else {
                return 1;
            };
            let res = RfResourceMask(a1 as u8);
            let token = active_krx_token(exclusive);
            usize::from(!coord.request_resource_lock(token, res, vfo, a2 as u16, false))
        }
        rf_vfo::MONITOR => {
            let target = if a1 >= 2 {
                !coord.monitor_active
            } else {
                a1 != 0
            };
            coord.set_monitor(radio, target);
            usize::from(coord.monitor_active)
        }
        _ => 1,
    }
}

#[inline(never)]
unsafe fn dispatch_rf_bulk(
    #[cfg_attr(not(feature = "rf_fsk"), allow(unused_variables))] coord: &mut RadioCoordinator,
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    sub: u8,
    a1: usize,
    a2: usize,
) -> usize {
    use ::abi::sub::rf_bulk;
    match sub {
        rf_bulk::SWEEP => {
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
            primitives::rf_sweep(radio, r.start_hz, r.step_hz, slice)
        }
        rf_bulk::SYMBOLS => {
            let count = a2 as u16;
            let Some(bytes) = (unsafe { ptr_slice(a1 as *const u8, (count as usize) * 4) }) else {
                return 1;
            };
            let slice = unsafe {
                core::slice::from_raw_parts(bytes.as_ptr() as *const KrxSymbol, count as usize)
            };
            usize::from(primitives::rf_emit_symbols(radio, slice).is_err())
        }
        #[cfg(feature = "rf_fsk")]
        rf_bulk::FSK_INIT => {
            primitives::rf_fsk_init(radio, a1 as u16);
            0
        }
        #[cfg(feature = "rf_fsk")]
        rf_bulk::FSK_SEND => {
            let Some(slice) = (unsafe { ptr_slice(a1 as *const u8, (a2 as u16) as usize) }) else {
                return 1;
            };
            // A data burst is a transmission: same BCLO, time-out, inhibit and battery
            // rules as PTT.
            let perm =
                coord.check_tx_permission(coord.effective_tx_slot(), active_krx_token(false));
            if !perm.is_granted() {
                return (perm as usize) + 1;
            }
            usize::from(primitives::rf_fsk_send(radio, slice).is_err())
        }
        #[cfg(feature = "rf_fsk")]
        rf_bulk::FSK_RECV => {
            let Some(slice) = (unsafe { ptr_slice_mut(a1 as *mut u8, (a2 as u16) as usize) })
            else {
                return 0;
            };
            primitives::rf_fsk_receive(radio, slice)
        }
        #[cfg(not(feature = "rf_fsk"))]
        rf_bulk::FSK_SEND => 1,
        _ => 0,
    }
}

#[inline(never)]
pub unsafe fn dispatch_rf(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::{
        idx as op,
        sub::{rf_get, rf_set},
    };
    if keys_transmitter(idx, a0 as u8, a1, a2)
        && !crate::runtime::native_runner::active_native_caps().contains(::abi::Cap::RadioTx)
    {
        return RF_TX_DENIED;
    }
    with_radio(0, |coord, radio| match idx {
        op::RfTune => {
            use kernel::service::radio::{Bandwidth, StepSetting};
            let bandwidth = match a1 {
                0 => Bandwidth::Narrow12k5,
                1 => Bandwidth::Wide25k,
                2 => Bandwidth::Narrow6k25,
                _ => return 1,
            };
            let Some(step) = StepSetting::from_request_hz(a2 as u32) else {
                return 1;
            };
            primitives::rf_tune(coord, radio, a0 as u32, bandwidth, step);
            0
        }
        op::RfGet => {
            let sub = a0 as u8;
            match sub {
                rf_get::FREQ | rf_get::BANDWIDTH | rf_get::MODULATION | rf_get::TX_STATE => {
                    let ch = coord.active_channel();
                    match sub {
                        rf_get::FREQ => ch.rx_freq.raw() as usize,
                        rf_get::BANDWIDTH => ch.bandwidth.in_hz() as usize,
                        rf_get::MODULATION => ch.modulation as usize,
                        _ => coord.is_transmitting as usize,
                    }
                }
                rf_get::RSSI => radio.get_rssi_dbm() as i32 as u32 as usize,
                rf_get::CARRIER => usize::from(coord.carrier_active || radio.is_carrier_open()),
                rf_get::GLITCH | rf_get::SNR | rf_get::NOISE => {
                    const REG_LUT: [u8; 3] = [0x63, 0x66, 0x65];
                    (radio.read_reg(REG_LUT[(sub - rf_get::GLITCH) as usize]) & 0xFF) as usize
                }
                rf_get::AFC..=rf_get::CTCSS_SCAN => {
                    const EXT_LUT: [u8; 4] = [0x6D, 0x6E, 0x6F, 0x68];
                    radio.read_reg(EXT_LUT[(sub - rf_get::AFC) as usize]) as usize
                }
                rf_get::FM_RSSI => {
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
        op::RfSet => {
            let sub = a0 as u8;
            match sub {
                rf_set::MODULATION..=rf_set::AGC => {
                    primitives::rf_set_param(coord, radio, sub, a1 as u8);
                    0
                }
                rf_set::TX => {
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
                }
                rf_set::TAIL => {
                    radio.send_css_tail(CssTailMode::Tone55Hz);
                    0
                }
                rf_set::SUBAUDIBLE => {
                    let mask = a1 as u8;
                    let kind = (a1 >> 8) as u8;
                    primitives::rf_apply_subaudible_split(
                        coord,
                        radio,
                        (mask & 1) != 0,
                        (mask & 2) != 0,
                        decode_subaudible(kind, a2 as u16),
                    );
                    0
                }
                rf_set::DTMF => {
                    primitives::rf_set_dtmf(radio, a1 as u8, a2 as u8);
                    0
                }
                rf_set::BURST => {
                    let dur = a2 as u16;
                    if dur == 0 {
                        coord.stop_1750hz_burst(radio);
                        0
                    } else {
                        usize::from(!coord.trigger_1750hz_burst(radio, dur))
                    }
                }
                rf_set::UNLOCK_VFO => {
                    let id = active_krx_token(false).id;
                    usize::from(!coord.release_client_lock(radio, id))
                }
                #[cfg(feature = "fm_radio")]
                rf_set::FM_TUNE => {
                    let _ = fm_tuner().set_frequency_10khz((a1 as u16).saturating_mul(10));
                    0
                }
                #[cfg(feature = "fm_radio")]
                rf_set::FM_VOLUME => {
                    let _ = fm_tuner().set_volume(a1 as u8);
                    0
                }
                #[cfg(feature = "fm_radio")]
                rf_set::FM_MUTE => {
                    let _ = fm_tuner().set_mute(a1 != 0);
                    0
                }
                _ => 0,
            }
        }
        op::RfTones => {
            let t1 = a1 as u16;
            let g1 = (a1 >> 16) as u8;
            let t2 = a2 as u16;
            let g2 = (a2 >> 16) as u8;
            if a0 != 0 {
                primitives::rf_set_dual_tones_hz(radio, t1, g1, t2, g2);
            } else {
                radio.set_dual_tones_raw(t1, g1, t2, g2);
            }
            0
        }
        op::RfVfo => unsafe { dispatch_rf_vfo(coord, radio, a0 as u8, a1, a2) },
        op::RfBulk => unsafe { dispatch_rf_bulk(coord, radio, a0 as u8, a1, a2) },
        _ => 0,
    })
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

        assert_eq!(decode_subaudible(1, 885), SubaudibleTone::Ctcss(885));
        assert_eq!(
            decode_subaudible(3, 0o754),
            SubaudibleTone::DcsInverted(0o754)
        );
        assert_eq!(decode_subaudible(1, 0), SubaudibleTone::None);
    }

    /// The record round trip itself is covered by the coordinator's
    /// `test_external_vfo_app_channel_roundtrip_talkaround_and_multi_app_arbitration`.
    #[test]
    fn vfo_exchange_uses_the_stored_channel_record_layout() {
        use ::abi::sub::rf_vfo;
        assert_eq!((rf_vfo::GET_CHANNEL, rf_vfo::SET_CHANNEL), (0, 1));
        assert_eq!(core::mem::size_of::<ChannelRecord>(), ChannelRecord::SIZE);
        assert_eq!(core::mem::offset_of!(ChannelRecord, flags), 20);
        assert_eq!(core::mem::offset_of!(ChannelRecord, name), 38);
    }

    #[test]
    fn keying_the_transmitter_needs_radio_tx_not_just_radio_rx() {
        use ::abi::{Cap, CapabilitySet, Op, idx, sub};
        assert!(keys_transmitter(idx::RfSet, sub::rf_set::TX, 1, 0));
        assert!(!keys_transmitter(idx::RfSet, sub::rf_set::TX, 0, 0));
        assert!(keys_transmitter(idx::RfSet, sub::rf_set::BURST, 0, 500));
        assert!(!keys_transmitter(idx::RfSet, sub::rf_set::BURST, 0, 0));
        assert!(keys_transmitter(idx::RfBulk, sub::rf_bulk::FSK_SEND, 0, 0));
        assert!(!keys_transmitter(idx::RfBulk, sub::rf_bulk::SWEEP, 0, 0));
        assert!(!keys_transmitter(idx::RfGet, sub::rf_get::RSSI, 1, 1));

        let slot = crate::runtime::native_runner::slot_mut();
        slot.is_loaded = true;
        slot.caps = CapabilitySet::empty().with(Cap::RadioRx);
        let key = |a1| unsafe {
            crate::runtime::syscall::kern_call(Op::RfSet.u32_id(), sub::rf_set::TX as usize, a1, 0)
        };
        assert_eq!(key(1), RF_TX_DENIED);
        assert_ne!(key(0), RF_TX_DENIED);
        crate::runtime::native_runner::init_native_slot();
        assert_eq!(core::mem::size_of::<KrxRadioState>(), 16);
    }
}
