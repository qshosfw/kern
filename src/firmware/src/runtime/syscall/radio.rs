//! Radio & Broadcast FM System Call Handlers (`Op::RfTune`, `Op::RfVfo`, `Op::RfBulk`)
//! and Shared Radio Property Accessors (`0xE0..=0xF8`).

use super::{ptr_ref, ptr_ref_mut, ptr_slice, ptr_slice_mut};
use crate::data::ChannelRecord;
use crate::services::radio::RadioCoordinator;
use drivers::bk4829::Bk4829;
use kernel::proc::abi::{KrxRadioState, KrxSweepReq, KrxSymbol};
use kernel::service::radio::{
    CssTailMode, RfResourceMask, RfSquelch, RfSubaudible, RfToneGenerator, SubaudibleTone, VfoSlot,
};

#[inline(always)]
fn with_radio<R>(
    f: impl FnOnce(&mut RadioCoordinator, &mut Bk4829<'static, bsp::RadioBus>) -> R,
) -> R {
    let (st, radio) = crate::services::state_and_radio();
    f(&mut st.coord, radio)
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

fn active_krx_token(exclusive: bool) -> kernel::service::radio::ClientToken {
    use kernel::service::radio::{ClientId, ClientToken, LockTier};
    let pid = super::service::active_caller_pid();
    let id = if pid > 1 {
        ClientId::for_krx_pid(pid)
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

/// Result of an operation refused because the image lacks `Cap::RadioTx`.
pub const RF_TX_DENIED: usize = 0xFF;

#[inline(always)]
fn decode_vfo_slot(slot: u8) -> Option<VfoSlot> {
    match slot {
        0 => Some(VfoSlot::VfoA),
        1 => Some(VfoSlot::VfoB),
        2 => Some(VfoSlot::VfoC),
        _ => None,
    }
}

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

/// Read any live radio scalar (`0xE0..=0xF8`) through `RadioCoordinator` & `Bk4829`.
pub fn radio_prop_get(id: u8) -> usize {
    use ::abi::prop;
    with_radio(|coord, radio| match id {
        prop::RF_FREQ | prop::RF_BANDWIDTH | prop::RF_MODULATION | prop::RF_TX => {
            let ch = coord.active_channel();
            match id {
                prop::RF_FREQ => ch.rx_frequency_hz as usize,
                prop::RF_BANDWIDTH => ch.bandwidth().in_hz() as usize,
                prop::RF_MODULATION => ch.modulation as usize,
                _ => coord.is_transmitting as usize,
            }
        }
        prop::RF_RSSI => radio.get_rssi_dbm() as i32 as u32 as usize,
        prop::RF_CARRIER => usize::from(coord.carrier_active || radio.is_carrier_open()),
        prop::RF_MONITOR => coord.monitor_active as usize,
        prop::RF_GLITCH..=prop::RF_NOISE => {
            const REG_LUT: [u8; 3] = [0x63, 0x66, 0x65];
            (radio.read_reg(REG_LUT[(id - prop::RF_GLITCH) as usize]) & 0xFF) as usize
        }
        prop::RF_AFC..=prop::RF_CTCSS_SCAN => {
            const EXT_LUT: [u8; 4] = [0x6D, 0x6E, 0x6F, 0x68];
            let reg = EXT_LUT[(id - prop::RF_AFC) as usize];
            let raw = radio.read_reg(reg);
            if id == prop::RF_CTCSS_SCAN {
                drivers::bk4829::subaudible::reg_to_ctcss_deci_hz(raw) as usize
            } else {
                raw as usize
            }
        }
        prop::FM_RSSI => {
            #[cfg(feature = "fm_radio")]
            {
                fm_tuner().get_rssi() as usize
            }
            #[cfg(not(feature = "fm_radio"))]
            0
        }
        _ => 0,
    })
}

/// Write any live radio scalar (`0xE4..=0xF8`) through `RadioCoordinator` & `Bk4829`.
pub fn radio_prop_set(id: u8, val: usize) -> usize {
    use ::abi::prop;
    with_radio(|coord, radio| match id {
        prop::RF_MODULATION..=prop::RF_AGC => {
            let slot = coord.active_vfo;
            coord.set_slot_param(radio, slot, id - prop::RF_MODULATION, val as u8);
            0
        }
        prop::RF_TX => {
            if val != 0 {
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
        prop::RF_MONITOR => {
            let target = if val >= 2 {
                !coord.monitor_active
            } else {
                val != 0
            };
            coord.set_monitor(radio, target);
            usize::from(coord.monitor_active)
        }
        prop::RF_WATCH_MODE => usize::from(!coord.apply_watch_mode(radio, val as u32)),
        prop::RF_TAIL => {
            radio.send_css_tail(CssTailMode::Tone55Hz);
            0
        }
        prop::RF_UNLOCK_VFO => {
            usize::from(!coord.release_client_lock(radio, active_krx_token(false).id))
        }
        #[cfg(feature = "fm_radio")]
        prop::FM_RSSI => {
            let _ = fm_tuner().set_frequency_10khz((val as u16).saturating_mul(10));
            0
        }
        #[cfg(feature = "fm_radio")]
        prop::FM_VOLUME => {
            let _ = fm_tuner().set_volume(val as u8);
            0
        }
        #[cfg(feature = "fm_radio")]
        prop::FM_MUTE => {
            let _ = fm_tuner().set_mute(val != 0);
            0
        }
        _ => 0,
    })
}

pub unsafe fn op_rf_tune(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::rf_tune;
    use kernel::service::radio::{Bandwidth, StepSetting};
    if a0 >= 16 {
        let bandwidth = match a1 {
            0 => Bandwidth::Narrow12k5,
            1 => Bandwidth::Wide25k,
            2 => Bandwidth::Narrow6k25,
            _ => return 1,
        };
        let Some(step) = StepSetting::from_request_hz(a2 as u32) else {
            return 1;
        };
        return with_radio(|coord, radio| {
            let slot = coord.active_vfo;
            coord.tune_slot(radio, slot, a0 as u32, Some(bandwidth), Some(step));
            0
        });
    }
    let sub = a0 as u8;
    if sub == rf_tune::BURST && (a2 as u16) != 0 && !super::has_cap(::abi::Cap::RadioTx) {
        return RF_TX_DENIED;
    }
    with_radio(|coord, radio| match sub {
        rf_tune::SUBAUDIBLE => {
            let mask = a1 as u8;
            let kind = (a1 >> 8) as u8;
            coord.set_subaudible_split(
                radio,
                (mask & 1) != 0,
                (mask & 2) != 0,
                decode_subaudible(kind, a2 as u16),
            );
            0
        }
        rf_tune::DTMF => {
            RadioCoordinator::set_dtmf(radio, a1 as u8, a2 as u8);
            0
        }
        rf_tune::BURST => {
            let dur = a2 as u16;
            if dur == 0 {
                coord.stop_1750hz_burst(radio);
                0
            } else {
                usize::from(!coord.trigger_1750hz_burst(radio, dur))
            }
        }
        rf_tune::TONES_RAW | rf_tune::TONES_HZ => {
            let (t1, g1) = (a1 as u16, (a1 >> 16) as u8);
            let (t2, g2) = (a2 as u16, (a2 >> 16) as u8);
            if sub == rf_tune::TONES_HZ {
                RadioCoordinator::set_dual_tones_hz(radio, t1, g1, t2, g2);
            } else {
                radio.set_dual_tones_raw(t1, g1, t2, g2);
            }
            0
        }
        _ => 1,
    })
}

pub unsafe fn op_rf_vfo(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::rf_vfo;
    with_radio(|coord, radio| match a0 as u8 {
        rf_vfo::GET_CHANNEL | rf_vfo::SET_CHANNEL => {
            let is_get = (a0 as u8) == rf_vfo::GET_CHANNEL;
            let (Some(vfo), Some(rec)) = (decode_vfo_slot(a1 as u8), unsafe {
                ptr_ref_mut(a2 as *mut ChannelRecord)
            }) else {
                return 1;
            };
            if (a2 & 3) != 0 {
                return 1;
            }
            if is_get {
                unsafe { core::ptr::write_bytes(rec, 0, 1) };
                rec.zone_mask = 1;
                coord.save_vfo_to_channel(vfo, rec);
                let memory = coord.vfo_memory_index[vfo.index().min(2)];
                return memory.map_or(0, |i| i as usize + 1) << 8;
            }
            let memory = ((a1 >> 8) as u16).checked_sub(1);
            coord.load_channel_into_vfo(radio, vfo, memory, rec);
            0
        }
        rf_vfo::STATE => {
            let Some(out) = (unsafe { ptr_ref_mut(a1 as *mut KrxRadioState) }) else {
                return 1;
            };
            coord.fill_krx_state(active_krx_token(false), out);
            0
        }
        rf_vfo::LOCK => {
            let slot = (a1 >> 16) as u8;
            let exclusive = ((a1 >> 24) as u8) != 0;
            let Some(vfo) = decode_vfo_slot(slot) else {
                return 1;
            };
            let res = RfResourceMask(a1 as u16);
            let token = active_krx_token(exclusive);
            let lease_ms = if (a2 as u16) == 0 { 1500 } else { a2 as u16 };
            usize::from(!coord.request_resource_lock(token, res, vfo, lease_ms, false))
        }
        _ => 1,
    })
}

pub unsafe fn op_rf_bulk(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::rf_bulk;
    let sub = a0 as u8;
    if sub == rf_bulk::FSK_SEND && !super::has_cap(::abi::Cap::RadioTx) {
        return RF_TX_DENIED;
    }
    with_radio(|coord, radio| match sub {
        rf_bulk::SWEEP => {
            let Some(r) = (unsafe { ptr_ref(a1 as *const KrxSweepReq) }) else {
                return 0;
            };
            let Some(bytes) =
                (unsafe { ptr_slice_mut(r.out_rssi as *mut u8, (r.count as usize) * 2) })
            else {
                return 0;
            };
            let slice = unsafe {
                core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut i16, r.count as usize)
            };
            coord.sweep_rssi(radio, r.start_hz, r.step_hz, slice)
        }
        rf_bulk::SYMBOLS => {
            if coord.is_transmitting && !super::has_cap(::abi::Cap::RadioTx) {
                return RF_TX_DENIED;
            }
            let count = a2 as u16;
            let Some(bytes) = (unsafe { ptr_slice(a1 as *const u8, (count as usize) * 4) }) else {
                return 1;
            };
            let slice = unsafe {
                core::slice::from_raw_parts(bytes.as_ptr() as *const KrxSymbol, count as usize)
            };
            RadioCoordinator::emit_symbols(radio, slice);
            0
        }
        #[cfg(feature = "rf_fsk")]
        rf_bulk::FSK_INIT => {
            crate::services::radio::fsk_stream::FskStreamService::arm_rx_baud(radio, a1 as u16);
            0
        }
        #[cfg(feature = "rf_fsk")]
        rf_bulk::FSK_SEND => {
            let Some(slice) = (unsafe { ptr_slice(a1 as *const u8, (a2 as u16) as usize) }) else {
                return 1;
            };
            let perm =
                coord.check_tx_permission(coord.effective_tx_slot(), active_krx_token(false));
            if !perm.is_granted() {
                return (perm as usize) + 1;
            }
            usize::from(
                crate::services::radio::fsk_stream::FskStreamService::transmit_bytes(radio, slice)
                    .is_err(),
            )
        }
        #[cfg(feature = "rf_fsk")]
        rf_bulk::FSK_RECV => {
            let Some(slice) = (unsafe { ptr_slice_mut(a1 as *mut u8, (a2 as u16) as usize) })
            else {
                return 0;
            };
            crate::services::radio::fsk_stream::FskStreamService::receive_bytes(radio, slice)
        }
        #[cfg(not(feature = "rf_fsk"))]
        rf_bulk::FSK_SEND => 1,
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::abi::{Cap, CapabilitySet, Op, prop, sub};

    #[test]
    fn test_krx_vfo_slot_subaudible_and_ctcss_scale() {
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
        // BK4829 silicon-verified 20.380/Hz CTCSS scan scale: 1804 -> 885 (88.5 Hz)
        assert_eq!(drivers::bk4829::subaudible::reg_to_ctcss_deci_hz(1804), 885);
    }

    #[test]
    fn tx_gate_requires_radio_tx_capability() {
        let slot = crate::runtime::native_runner::slot_mut();
        slot.is_loaded = true;
        slot.caps = CapabilitySet::empty().with(Cap::RadioRx);

        let tx_a0 = (prop::RF_TX as usize) | ((sub::sys_prop::SET as usize) << 8);
        assert_eq!(
            unsafe { crate::runtime::syscall::kern_call(Op::SysProp.u32_id(), tx_a0, 1, 0) },
            RF_TX_DENIED
        );
        assert_eq!(
            unsafe {
                crate::runtime::syscall::kern_call(
                    Op::RfTune.u32_id(),
                    sub::rf_tune::BURST as usize,
                    0,
                    500,
                )
            },
            RF_TX_DENIED
        );
        assert_eq!(
            unsafe {
                crate::runtime::syscall::kern_call(
                    Op::RfBulk.u32_id(),
                    sub::rf_bulk::FSK_SEND as usize,
                    0,
                    0,
                )
            },
            RF_TX_DENIED
        );
        crate::runtime::native_runner::init_native_slot();
    }
}
