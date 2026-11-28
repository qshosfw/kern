//! Thin Firmware RigWire v2 Adapter.
//!
//! Delegates framing, `WireCursor`, and `CH_DATA` (`KernFS` `Fd` transfers + sector-hash sync)
//! to `kernel::wire`, routing `CH_CONTROL` over `abi::prop`, `RadioView`, and `MasterConfig`.

use crate::hardware::SystemHardware;
use abi::Status;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::service::SerialPort;
use kernel::wire::*;

pub static mut CURRENT_TRANSFER: ActiveTransfer = ActiveTransfer::empty();
static mut NEXT_TRANSFER_ID: u16 = 1;
pub static mut ACTIVE_SUBSCRIPTION: EventSubscription = EventSubscription::empty();

/// Wire-ready `CORE::CAPS` reply: `[gen: u32, next_cursor: u16, count: u8, SemanticCap[7]]`.
#[repr(C, packed)]
struct CapsReplyWire {
    generation: u32,
    next_cursor: u16,
    count: u8,
    caps: [SemanticCap; 7],
}

const CAPS_REPLY_WIRE: CapsReplyWire = CapsReplyWire {
    generation: 1,
    next_cursor: 0xFFFF,
    count: 7,
    caps: [
        SemanticCap::new(*b"TUNE", *b"BK48", 0, 1, 0x0001, 0x0000_000F),
        SemanticCap::new(*b"TXPA", *b"BK48", 0, 1, 0x0001, 0x0000_0007),
        SemanticCap::new(*b"AUDI", *b"PA00", 0, 1, 0x0004, 0x0000_0003),
        SemanticCap::new(*b"DISP", *b"ST75", 0, 1, 0x0005, 0x0000_0003),
        SemanticCap::new(*b"INPT", *b"KMAT", 0, 1, 0x0006, 0x0000_0001),
        SemanticCap::new(*b"POWR", *b"ADC0", 0, 1, 0x0007, 0x0000_0003),
        SemanticCap::new(*b"STOR", *b"KF21", 0, 1, 0x0009, 0x0000_0003),
    ],
};

/// Wire-ready `CORE::TABLES` reply: `[count: u8, TableDesc[6]]` (`1 + 6*12 = 73 B`).
#[repr(C, packed)]
struct TablesReplyWire {
    count: u8,
    tables: [TableDesc; 6],
}

const TABLES_REPLY_WIRE: TablesReplyWire = TablesReplyWire {
    count: 6,
    tables: [
        TableDesc::new(
            *b"CHAN",
            64,
            512,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"CONT",
            32,
            256,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"BNDP",
            32,
            64,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"ZONE",
            32,
            64,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"HITS",
            32,
            128,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"FMBC",
            16,
            64,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
    ],
};

#[inline(always)]
const fn as_raw_bytes<T>(val: &'static T) -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(val as *const T as *const u8, core::mem::size_of::<T>()) }
}

#[inline(always)]
fn require_write_auth(hw: &SystemHardware<'_>) -> Result<(), Status> {
    let s = &hw.wire.session;
    if s.require_auth_writes && !s.is_authenticated {
        Err(Status::PermissionDenied)
    } else {
        Ok(())
    }
}

struct FirmwareDataHost<'a, 'b> {
    hw: &'a mut SystemHardware<'b>,
}

impl<'a, 'b> WireDataHost for FirmwareDataHost<'a, 'b> {
    fn is_authenticated(&self) -> bool {
        self.hw.wire.session.is_authenticated
    }

    fn require_write_auth(&self) -> Result<(), Status> {
        require_write_auth(self.hw)
    }

    fn staging_buf(&mut self) -> &mut [u8; 512] {
        crate::data::arena::staging_buf()
    }

    fn commit_calib<D: kernfs::BlockDevice>(
        &mut self,
        fs: &mut kernfs::KernFS<'_, D>,
    ) -> Result<u32, Status> {
        let staging = crate::data::arena::staging_buf();
        let calib = crate::calibration_mut();
        crate::services::calibration::commit_calib_slice(calib, fs, staging)
            .map_err(|_| Status::SafetyBlocked)?;
        let radio = &mut self.hw.radio;
        crate::apply_active_calibration(radio);
        radio.set_xtal_freq_trim(calib.xtal_freq_low());
        Ok(crypto::crc::CrcEngine::crc32(staging))
    }

    fn on_path_mutated(&mut self, path: &str) {
        if path.starts_with("/apps/") {
            crate::apps::launcher::catalog::get_catalog().mark_dirty();
        }
    }
}

#[inline(never)]
fn handle_core_service(
    op: u8,
    c: &mut WireCursor<'_>,
    hw: &mut SystemHardware<'_>,
    now: u32,
    reboot_mode: &mut u8,
) -> Result<(), Status> {
    match op {
        OP_CORE_HELLO => {
            let _ = c.take(2)?;
            let epoch = c.rd_u32()?;
            let req_mtu = c.rd_u16()?;
            if epoch == 0 {
                return Err(Status::InvalidArg);
            }
            let mtu = req_mtu.clamp(32, MAX_PAYLOAD_LEN as u16);
            let tp = hw.wire.transport_profile;
            hw.wire.session.begin_session(epoch, mtu);
            c.wr_u8(0);
            c.wr_u16(mtu);
            c.wr_u32(1);
            c.wr_u16(tp);
            c.wr_u32(1);
            c.wr_u32(2);
        }
        OP_CORE_AUTH => {
            let inp = c.inp;
            let auth_out = unsafe { &mut *(c.out.as_mut_ptr() as *mut [u8; 20]) };
            c.pos = hw
                .wire
                .session
                .handle_auth_step(inp, py32_hal::delay::micros(), auth_out)?;
        }
        OP_CORE_PING => {
            let tok = c.rd_u32()?;
            c.wr_u32(tok);
            c.wr_u32(now);
        }
        OP_CORE_TIME_SYNC => {
            let tok = c.rd_u32()?;
            let us = py32_hal::delay::micros();
            c.wr_u32(tok);
            c.wr_u32(us);
            c.wr_u32(us);
            c.wr_u32(now);
        }
        OP_CORE_GET_TIME => {
            c.wr_u32(py32_hal::delay::micros());
            c.wr_u32(now);
        }
        OP_CORE_RUNLEVEL => {
            if !hw.wire.session.is_active() {
                return Err(Status::BadSession);
            }
            *reboot_mode = c.inp.first().copied().unwrap_or(0);
        }
        OP_CORE_CAPS => c.put(as_raw_bytes(&CAPS_REPLY_WIRE)),
        OP_CORE_TABLES => c.put(as_raw_bytes(&TABLES_REPLY_WIRE)),
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

/// Main entry point for all RigWire v2 records routed from `WireEngine`.
#[inline(never)]
pub fn dispatch_wire_record(header: &WireHeader, hw: &mut SystemHardware, now: u32) -> bool {
    let raw = hw.wire.current_payload(header);
    let plen = raw.len().min(MAX_PAYLOAD_LEN);
    let max_out = (hw.wire.session.negotiated_max_payload as usize).clamp(32, MAX_PAYLOAD_LEN);
    let inp = unsafe { core::slice::from_raw_parts(raw.as_ptr(), plen) };
    let mut wire_uninit = core::mem::MaybeUninit::<[u8; MAX_WIRE_BUF_LEN]>::uninit();
    let wire_buf = unsafe { &mut *wire_uninit.as_mut_ptr() };
    let out = unsafe {
        core::slice::from_raw_parts_mut(
            wire_buf.as_mut_ptr().add(WIRE_PAYLOAD_OFFSET),
            MAX_PAYLOAD_LEN,
        )
    };
    let mut reboot_mode = 0xFFu8;

    let res = {
        let mut c = WireCursor::new(inp, out, max_out);
        let status_opt = match header.channel {
            CH_CORE => Some(
                handle_core_service(header.op, &mut c, hw, now, &mut reboot_mode)
                    .err()
                    .unwrap_or(Status::Ok),
            ),
            CH_CONTROL => handle_control_service(header.op, &mut c, hw, now),
            CH_DATA => {
                let t = unsafe { &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER) };
                let next_tid = unsafe { &mut *core::ptr::addr_of_mut!(NEXT_TRANSFER_ID) };
                #[cfg(not(test))]
                let fs = crate::kern_fs();
                #[cfg(test)]
                let fs = unsafe {
                    &mut *(hw.filesystem
                        as *mut kernfs::KernFS<
                            '_,
                            drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>,
                        >)
                };
                let mut host = FirmwareDataHost { hw };
                dispatch_data_op(header.op, &mut c, fs, t, next_tid, now, &mut host)
            }
            CH_EVENTS => {
                let sub = unsafe { &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION) };
                match header.op {
                    OP_EVENTS_SUBSCRIBE => match c.rd_u32().and_then(|m| Ok((m, c.rd_u16()?))) {
                        Ok((m, cr)) => {
                            sub.active_mask = m;
                            sub.credits = cr;
                            Some(Status::Ok)
                        }
                        Err(e) => Some(e),
                    },
                    OP_EVENTS_UNSUBSCRIBE => {
                        sub.active_mask = 0;
                        sub.credits = 0;
                        Some(Status::Ok)
                    }
                    OP_EVENTS_CREDIT => Some(match c.rd_u16() {
                        Ok(g) => {
                            sub.credits = sub.credits.saturating_add(g);
                            Status::Ok
                        }
                        Err(e) => e,
                    }),
                    _ => None,
                }
            }
            _ => return false,
        };
        status_opt.map(|st| (st, c.pos))
    };
    let Some((status, resp_len)) = res else {
        return false;
    };
    let _ = hw
        .wire
        .send_response_in_place(header, status, resp_len, wire_buf);
    if reboot_mode != 0xFF {
        hw.wire.port.flush();
        kernel::power::PowerControl::reboot_with(kernel::power::RebootMode::from_u8(reboot_mode));
    }
    true
}

// ── CH_CONTROL (0x01): Universal Resource Handle RPC ────────────────────────

fn active_radio_slot(
    instance: u8,
) -> (
    kernel::service::radio::VfoSlot,
    &'static mut kernel::service::radio::ChannelConfig,
) {
    use kernel::service::radio::VfoSlot;
    let coord = &mut crate::radio_state().coord;
    let slot = match instance {
        RADIO_INSTANCE_VFO_A => VfoSlot::VfoA,
        RADIO_INSTANCE_VFO_B => VfoSlot::VfoB,
        _ => coord.active_vfo,
    };
    (slot, coord.vfo_channel_mut(slot))
}

#[inline(never)]
fn on_handle_get(
    handle: ResourceHandle,
    c: &mut WireCursor<'_>,
    hw: &mut SystemHardware<'_>,
    now: u32,
) -> Result<(), Status> {
    use crate::runtime::syscall::service::live_prop_get;
    use ::abi::prop;
    match (handle.domain, handle.facet) {
        (DOMAIN_SYSTEM, FACET_SYS_IDENTITY) => {
            let mut mac = [0u8; 6];
            let mut cf = [0u8; 15];
            let serial = bsp::uid::fill_device_identity(&mut mac, &mut cf);
            c.wr_u32(serial as u32);
            c.wr_u32((serial >> 32) as u32);
            c.put(&mac);
            c.put(&cf[..14]);
            let is_auth = hw.wire.session.is_authenticated;
            c.wr_u8(is_auth as u8);
            if is_auth {
                c.put(&bsp::uid::read_mcu_uid());
            } else {
                c.zeroes(12);
            }
        }
        (DOMAIN_SYSTEM, FACET_SYS_METRICS) => {
            let bk_gpio = hw.radio.read_gpio_reg();
            let status_led = if bk_gpio == 0xFFFF {
                0
            } else {
                (bk_gpio & 3) as u8
            };
            let disp = &crate::master_config_ref().display;
            c.wr_u32(now);
            c.wr_u32(live_prop_get(prop::CPU_HZ) as u32);
            c.wr_u16(live_prop_get(prop::BATT_MV) as u16);
            c.put(&[
                live_prop_get(prop::BATT_PCT) as u8,
                hw.orchestrator.active_app_id() as u8,
                hw.orchestrator.is_keypad_locked() as u8,
                live_prop_get(prop::FLASHLIGHT) as u8,
                status_led,
            ]);
            c.wr_u16(bk_gpio);
            c.put(&[
                crate::system::is_headless() as u8,
                drivers::backlight::BACKLIGHT_DUTY.load(core::sync::atomic::Ordering::Relaxed),
                disp.contrast,
                disp.inverted,
            ]);
        }
        (DOMAIN_SYSTEM, FACET_SYS_CALIBRATION) => {
            let calib = crate::calibration_mut();
            c.wr_u8((calib.is_valid() && calib.is_device_bound()) as u8);
            c.wr_u16(calib.xtal_freq_low() as u16);
            c.wr_u16(calib.battery_calib_const());
        }
        (DOMAIN_SYSTEM, FACET_SYS_AUDIO) => {
            let rem = c.max_out.saturating_sub(c.pos).min(64);
            let n = crate::system::drain_headless_notes(&mut c.out[c.pos..c.pos + rem]);
            c.pos += n;
        }
        (DOMAIN_RADIO, facet) => {
            let (slot, ch) = active_radio_slot(handle.instance);
            let coord = &crate::radio_state_ref().coord;
            match facet {
                FACET_RADIO_TUNING => {
                    c.wr_u32(ch.effective_rx_freq().raw());
                    c.wr_u32(ch.step.in_hz());
                    c.wr_u8(coord.active_vfo as u8);
                }
                FACET_RADIO_MODULATION => {
                    c.wr_u8(ch.modulation.to_u8());
                    c.wr_u32(ch.bandwidth.in_hz());
                }
                FACET_RADIO_SQUELCH => {
                    let rv = crate::services::radio::radio_view();
                    c.wr_u8(ch.squelch_level);
                    c.wr_u16(rv.raw_rssi);
                    c.wr_u8(rv.is_rx_open() as u8);
                }
                FACET_RADIO_TRANSMIT => {
                    c.put(&[coord.is_slot_tx_active(slot) as u8, ch.power as u8]);
                }
                FACET_RADIO_DIGITAL => c.zeroes(2),
                _ => return Err(Status::Unsupported),
            }
        }
        (DOMAIN_UI, FACET_UI_SURFACE) => c.put(&[128, 0, 64, 0, 1, 0, 4]),
        (DOMAIN_UI, FACET_UI_MODAL) => {
            let modal = crate::orchestrator::compositor::active_modal_mut();
            c.put(&[modal.is_active() as u8, modal.tag]);
        }
        (DOMAIN_UI, FACET_UI_VIEW) | (DOMAIN_APP, FACET_APP_LIFECYCLE) => {
            c.wr_u8(hw.orchestrator.active_app_id() as u8);
        }
        (DOMAIN_INPUT, FACET_INPUT_LOCK) => c.wr_u8(hw.orchestrator.is_keypad_locked() as u8),
        (DOMAIN_APP, FACET_APP_REGISTRY) => c.wr_u8(crate::AppRegistry::app_count() as u8),
        (DOMAIN_CONFIG, _) => {
            if handle.instance > 5 {
                return Err(Status::InvalidArg);
            }
            c.put(crate::master_config_ref().config_bytes(handle.instance as usize));
        }
        (DOMAIN_STORAGE, _) => {
            let s = hw.filesystem.stats();
            for v in [
                s.active_files,
                s.used_sectors,
                s.free_sectors,
                s.deleted_sectors,
            ] {
                c.wr_u16(v);
            }
            c.wr_u32(s.wear_cycles);
            c.wr_u16(s.total_sectors);
        }
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

#[inline(never)]
fn handle_resource_set(
    handle: ResourceHandle,
    c: &mut WireCursor<'_>,
    hw: &mut SystemHardware<'_>,
) -> Result<(), Status> {
    if (handle.domain == DOMAIN_SYSTEM && handle.facet == FACET_SYS_METRICS)
        || (handle.domain == DOMAIN_INPUT && handle.facet == FACET_INPUT_LOCK)
    {
        if handle.domain == DOMAIN_INPUT || !c.inp.is_empty() {
            let locked = c.rd_u8()? != 0;
            let _ = crate::runtime::syscall::service::live_prop_set(
                ::abi::prop::KEY_LOCKED,
                locked as usize,
            );
        }
        return Ok(());
    }
    match handle.domain {
        DOMAIN_RADIO => {
            let (slot, _) = active_radio_slot(handle.instance);
            let coord = &mut crate::radio_state().coord;
            match handle.facet {
                FACET_RADIO_TUNING => {
                    if c.inp.is_empty() {
                        coord.select_vfo(&mut hw.radio, slot);
                    } else {
                        let freq_hz = c.rd_u32()?;
                        if !(18_000_000..=1_300_000_000).contains(&freq_hz) {
                            return Err(Status::InvalidArg);
                        }
                        coord.tune_slot(&mut hw.radio, slot, freq_hz, None, None);
                    }
                }
                FACET_RADIO_MODULATION | FACET_RADIO_SQUELCH => {
                    let v = c.rd_u8()?;
                    coord.set_slot_param(
                        &mut hw.radio,
                        slot,
                        (handle.facet - FACET_RADIO_MODULATION) as u8,
                        v,
                    );
                }
                FACET_RADIO_TRANSMIT => {
                    let tx = c.rd_u8()? != 0;
                    if tx {
                        let token = kernel::service::radio::ClientToken::new(
                            kernel::service::radio::ClientId::SYSTEM,
                            kernel::service::radio::LockTier::FOREGROUND,
                        );
                        if !coord.request_ptt_start(&mut hw.radio, token).is_granted() {
                            return Err(Status::SafetyBlocked);
                        }
                    } else {
                        coord.request_ptt_stop(&mut hw.radio);
                    }
                    crate::system::set_wire_tx(coord.is_transmitting);
                }
                FACET_RADIO_DIGITAL => {
                    let offset_dhz = c.rd_u16()? as i16;
                    coord.set_vfo_sub_hz_offset(&mut hw.radio, slot, offset_dhz);
                }
                _ => return Err(Status::Unsupported),
            }
            Ok(())
        }
        DOMAIN_INPUT => match handle.facet {
            FACET_INPUT_KEY => {
                let code = KeyCode::from_u8(c.rd_u8()?).ok_or(Status::InvalidArg)?;
                let state = [KeyState::Pressed, KeyState::Held, KeyState::Released]
                    [c.inp.first().copied().unwrap_or(0).min(2) as usize];
                crate::system::enqueue_event(Event::Key { code, state });
                Ok(())
            }
            FACET_INPUT_TEXT => {
                for &b in c.inp.iter().take(32) {
                    if (0x20..=0x7E).contains(&b) {
                        crate::system::enqueue_event(Event::Char(b as char));
                    }
                }
                Ok(())
            }
            FACET_INPUT_ENCODER => {
                let delta = c.rd_u8()? as i8;
                if delta != 0 {
                    crate::system::enqueue_event(Event::Encoder(delta));
                }
                Ok(())
            }
            _ => Err(Status::Unsupported),
        },
        DOMAIN_CONFIG => {
            if handle.instance > 5 {
                return Err(Status::InvalidArg);
            }
            let idx = handle.instance as usize;
            let mcfg = crate::master_config();
            let dest = mcfg.config_bytes_mut(idx);
            if c.inp.len() != dest.len() {
                return Err(Status::BadLength);
            }
            if c.inp[0] != crate::config::MasterConfig::domain_version(idx) {
                return Err(Status::IncompatibleAbi);
            }
            dest.copy_from_slice(c.inp);
            mcfg.mark_domain_dirty(idx);
            mcfg.sanitize();
            let _ = mcfg.flush_dirty(hw.filesystem);
            Ok(())
        }
        _ => Err(Status::Unsupported),
    }
}

#[inline(never)]
fn handle_resource_invoke(
    handle: ResourceHandle,
    c: &mut WireCursor<'_>,
    hw: &mut SystemHardware<'_>,
) -> Result<(), Status> {
    let action_id = c.rd_u16()?;
    c.wr_u16(action_id);
    let arg = c.inp.first().copied().unwrap_or(0);
    match (handle.domain, action_id) {
        (DOMAIN_SYSTEM, ACTION_SYS_REBOOT) => {
            hw.wire.port.flush();
            kernel::power::PowerControl::reboot_with(kernel::power::RebootMode::from_u8(arg));
        }
        (DOMAIN_SYSTEM, ACTION_SYS_FLASHLIGHT) => {
            let mode = c.inp.first().copied().unwrap_or(2);
            let _ = crate::runtime::syscall::service::live_prop_set(
                ::abi::prop::FLASHLIGHT,
                mode as usize,
            );
        }
        (DOMAIN_SYSTEM, ACTION_SYS_HEADLESS) => {
            let mode = c.inp.first().copied().unwrap_or(2);
            let enable = match mode {
                0 => false,
                1 => true,
                _ => !crate::system::is_headless(),
            };
            crate::system::set_headless(hw, enable);
            c.wr_u8(enable as u8);
        }
        (DOMAIN_RADIO, ACTION_RADIO_VFO_TOGGLE) => {
            crate::radio_state().coord.toggle_vfo(&mut hw.radio);
        }
        (DOMAIN_UI, ACTION_MODAL_DISMISS) => {
            crate::orchestrator::compositor::active_modal_mut().dismiss();
        }
        (DOMAIN_UI, ACTION_MODAL_CONFIRM | ACTION_MODAL_CANCEL | ACTION_VIEW_LAUNCHER)
        | (DOMAIN_APP, ACTION_APP_KILL) => {
            let (code, state) = match (handle.domain, action_id) {
                (DOMAIN_UI, ACTION_MODAL_CONFIRM) => (KeyCode::Menu, KeyState::Pressed),
                (DOMAIN_UI, ACTION_MODAL_CANCEL) => (KeyCode::Exit, KeyState::Pressed),
                _ => (KeyCode::Exit, KeyState::Held),
            };
            crate::system::enqueue_event(Event::Key { code, state });
        }
        (DOMAIN_INPUT, ACTION_INPUT_LOCK_TOGGLE) => hw.orchestrator.toggle_keypad_lock(),
        (DOMAIN_APP, ACTION_APP_LAUNCH) => {
            if (arg as usize) >= crate::AppRegistry::app_count() {
                return Err(Status::InvalidArg);
            }
            crate::system::enqueue_event(Event::Sys(crate::apps::Shortcut::LaunchApp(arg).into()));
        }
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

#[inline(never)]
fn handle_ui_stream(handle: ResourceHandle, c: &mut WireCursor<'_>) -> Result<(), Status> {
    let offset = c.rd_u32()?;
    let length = c.rd_u16()?;
    if handle.domain != DOMAIN_UI || handle.facet != FACET_UI_SURFACE {
        return Err(Status::Unsupported);
    }
    let flat = crate::system_canvas().buffer_flat();
    if (offset & 0x8000_0000) != 0 {
        let prev = c.inp;
        c.wr_u32(0x8000_0000);
        c.zeroes(4);
        let mut dirty_mask = 0u16;
        let mut sent_mask = 0u16;
        for s in 0..16usize {
            let strip = &flat[s * 64..(s + 1) * 64];
            let crc = crypto::crc::CrcEngine::crc16_ccitt(strip);
            let p_off = s * 2;
            let same = prev.len() >= p_off + 2
                && u16::from_le_bytes([prev[p_off], prev[p_off + 1]]) == crc;
            if !same {
                dirty_mask |= 1 << s;
                if c.pos + 65 <= c.max_out.saturating_sub(12) {
                    c.wr_u8(64);
                    c.put(strip);
                    sent_mask |= 1 << s;
                }
            }
        }
        c.out[8..10].copy_from_slice(&dirty_mask.to_le_bytes());
        c.out[10..12].copy_from_slice(&sent_mask.to_le_bytes());
        return Ok(());
    }
    let off = (offset as usize).min(flat.len());
    let len = (length as usize)
        .min(c.max_out.saturating_sub(10).min(248))
        .min(flat.len() - off);
    c.wr_u32(off as u32);
    c.wr_u16(len as u16);
    c.put(&flat[off..off + len]);
    Ok(())
}

#[inline(never)]
fn handle_control_service(
    opcode: u8,
    c: &mut WireCursor<'_>,
    hw: &mut SystemHardware<'_>,
    now: u32,
) -> Option<Status> {
    if !(OP_HANDLE_GET..=OP_HANDLE_STREAM).contains(&opcode) {
        return None;
    }
    let Ok(hdr) = c.take(4) else {
        return Some(Status::BadLength);
    };
    let handle = ResourceHandle {
        domain: hdr[0],
        instance: hdr[1],
        facet: u16::from_le_bytes([hdr[2], hdr[3]]),
    };
    c.put(hdr);

    let res = match opcode {
        OP_HANDLE_GET => on_handle_get(handle, c, hw, now),
        OP_HANDLE_SET => require_write_auth(hw).and_then(|()| handle_resource_set(handle, c, hw)),
        OP_HANDLE_INVOKE => {
            require_write_auth(hw).and_then(|()| handle_resource_invoke(handle, c, hw))
        }
        OP_HANDLE_STREAM => handle_ui_stream(handle, c),
        _ => return None,
    };
    Some(res.err().unwrap_or(Status::Ok))
}
