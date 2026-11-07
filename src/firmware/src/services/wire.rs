//! Declarative Firmware RigWire Service & Component Dispatcher.
//!
//! Bridges RigWire serial transport with microkernel traits, firmware services,
//! hardware peripherals, and hosted applications using universal Resource Handles.
//!
//! Subsystems Handled:
//! - SERVICE_CONTROL (0x01): Universal microkernel Resource Handle routing to System, Radio,
//!   UI, Input, Config, App, and Storage facets.
//! - SERVICE_DATA (0x03): Hardened chunked file streaming with strict path canonicalization,
//!   8 KiB write ceiling, sector quota guards, automatic `/sys/calib.bak` backup,
//!   and streaming CRC32 verification.
//! - SERVICE_EVENTS (0x02): Host event subscriptions and asynchronous event streaming.

use crate::hardware::SystemHardware;
use crate::services::calibration::{CALIB_FILE_PATH, is_valid_calibration};
use crate::services::storage::DirEntryInfo;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::service::{
    ModulationMode, PowerService, RfFineTuning, RfModulation, RfPowerLevel, RfPowerSequencer,
    RfSquelch,
};
use kernel::wire::*;
use utils::units::Frequency;

/// Maximum upload size accepted over RigWire (8 KiB, strictly aligned to transient arena).
pub const MAX_WIRE_FILE_SIZE: usize = crate::data::arena::TRANSIENT_ARENA_BYTES;

/// Reuses the shared transient arena so it incurs strictly ZERO bytes of Flash OR SRAM overhead.
#[inline(always)]
pub fn wire_file_staging_buf() -> &'static mut [u8; MAX_WIRE_FILE_SIZE] {
    crate::data::arena::transient_arena_bytes_mut()
}

/// Active transfer session state
#[derive(Clone, Copy)]
pub struct ActiveTransfer {
    pub active: bool,
    pub transfer_id: u16,
    pub mode: u8,
    pub total_size: u32,
    pub current_len: u32,
    pub chunk_size: u16,
    pub path: [u8; 32],
    pub path_len: u8,
}

impl ActiveTransfer {
    pub const fn empty() -> Self {
        Self {
            active: false,
            transfer_id: 0,
            mode: TRANSFER_MODE_READ,
            total_size: 0,
            current_len: 0,
            chunk_size: 0,
            path: [0u8; 32],
            path_len: 0,
        }
    }

    pub fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len as usize]).unwrap_or("")
    }
}

pub static mut CURRENT_TRANSFER: ActiveTransfer = ActiveTransfer::empty();
static mut NEXT_TRANSFER_ID: u16 = 1;

#[inline(always)]
fn active_transfer_mut(transfer_id: u16) -> Option<&'static mut ActiveTransfer> {
    let transfer = unsafe { &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER) };
    if transfer.active && transfer.transfer_id == transfer_id {
        Some(transfer)
    } else {
        None
    }
}

/// Event streaming subscription state (§8.9)
#[derive(Clone, Copy)]
pub struct EventSubscription {
    pub active_mask: u32,
    pub credits: u16,
}

impl EventSubscription {
    pub const fn empty() -> Self {
        Self {
            active_mask: 0,
            credits: 0,
        }
    }
}

pub static mut ACTIVE_SUBSCRIPTION: EventSubscription = EventSubscription::empty();

#[inline(never)]
fn send_resp(
    hw: &mut SystemHardware,
    header: &WireHeader,
    status: WireStatus,
    payload: &[u8],
) -> bool {
    let _ = hw.wire.send_response(
        header.service,
        header.opcode,
        header.sequence,
        status,
        payload,
    );
    true
}

#[inline(always)]
fn send_status(hw: &mut SystemHardware, header: &WireHeader, status: WireStatus) -> bool {
    send_resp(hw, header, status, &[])
}

/// Main entry point for non-core RigWire records routed from the transport driver.
#[inline(never)]
pub fn dispatch_wire_record(header: &WireHeader, hw: &mut SystemHardware, now: u32) -> bool {
    let mut payload_buf = [0u8; 128];
    let plen = (header.payload_len as usize).min(128);
    payload_buf[..plen].copy_from_slice(hw.wire.current_payload(header));
    let payload = &payload_buf[..plen];

    match header.service {
        SERVICE_CONTROL => handle_control_service(header, payload, hw, now),
        SERVICE_DATA => handle_data_service(header, payload, hw),
        SERVICE_EVENTS => handle_events_service(header, payload, hw),
        _ => false,
    }
}

// ── SERVICE_CONTROL (0x01): Universal Handle RPC ────────────────────────────

#[inline(always)]
fn get_u8(val: &[u8]) -> Result<u8, WireStatus> {
    val.first().copied().ok_or(WireStatus::BadLength)
}

#[inline(always)]
fn get_u32_le(val: &[u8]) -> Result<u32, WireStatus> {
    if val.len() >= 4 {
        Ok(u32::from_le_bytes([val[0], val[1], val[2], val[3]]))
    } else {
        Err(WireStatus::BadLength)
    }
}

#[inline(always)]
fn get_i16_le(val: &[u8]) -> Result<i16, WireStatus> {
    if val.len() >= 2 {
        Ok(i16::from_le_bytes([val[0], val[1]]))
    } else {
        Err(WireStatus::BadLength)
    }
}

#[inline(never)]
fn read_sys_facet(
    facet: u16,
    hw: &SystemHardware,
    now: u32,
    out: &mut [u8],
) -> Result<usize, WireStatus> {
    match facet {
        FACET_SYS_IDENTITY => {
            // Serial64 (8B)
            out[..8].copy_from_slice(&bsp::uid::get_hardware_serial64().to_le_bytes());
            // MAC address (6B)
            out[8..14].copy_from_slice(&bsp::uid::read_mac_address());
            // Crockford serial string (14B)
            out[14..28].copy_from_slice(&bsp::uid::get_crockford_serial()[..14]);
            // Auth status (1B)
            let is_auth = hw.wire.session.is_authenticated;
            out[28] = is_auth as u8;
            // MCU UID (12B if authenticated, otherwise 0)
            if is_auth {
                out[29..41].copy_from_slice(&bsp::uid::read_mcu_uid());
            } else {
                out[29..41].fill(0);
            }
            Ok(41)
        }
        FACET_SYS_METRICS => {
            out[..4].copy_from_slice(&now.to_le_bytes());
            out[4..8].copy_from_slice(&py32_hal::rcc::sysclk_hz().to_le_bytes());
            let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
            out[8..10].copy_from_slice(&pwr.battery_voltage_mv().to_le_bytes());
            out[10] = pwr.battery_percentage();
            out[11] = hw.orchestrator.active_app_id() as u8;
            out[12] = hw.orchestrator.is_keypad_locked() as u8;
            out[13] = bsp::pins::FLASHLIGHT.is_high() as u8;
            Ok(14)
        }
        FACET_SYS_CALIBRATION => {
            let calib = unsafe { &*core::ptr::addr_of!(crate::CALIBRATION) };
            out[0] = is_valid_calibration(calib.raw_bytes()) as u8;
            out[1..3].copy_from_slice(&calib.xtal_freq_low().to_le_bytes());
            out[3..5].copy_from_slice(&calib.battery_calib_const().to_le_bytes());
            Ok(5)
        }
        _ => Err(WireStatus::UnsupportedField),
    }
}

#[inline(never)]
fn read_radio_facet(
    facet: u16,
    instance: u8,
    hw: &mut SystemHardware,
    out: &mut [u8],
) -> Result<usize, WireStatus> {
    let radio_state = crate::radio_state_ref();
    let (vfo, ch) = match instance {
        RADIO_INSTANCE_VFO_A => (&radio_state.vfo_a, &radio_state.coord.channels[0]),
        RADIO_INSTANCE_VFO_B => (&radio_state.vfo_b, &radio_state.coord.channels[1]),
        _ => (
            radio_state.selected_vfo(),
            radio_state.coord.active_channel(),
        ),
    };
    match facet {
        FACET_RADIO_TUNING => {
            out[..4].copy_from_slice(&vfo.frequency.raw().to_le_bytes());
            out[4..8].copy_from_slice(&((radio_state.step_khz as u32) * 1000).to_le_bytes());
            out[8] = radio_state.active_vfo;
            Ok(9)
        }
        FACET_RADIO_MODULATION => {
            out[0] = ch.modulation.to_u8();
            let bw_hz: u32 = ch.bandwidth.in_hz();
            out[1..5].copy_from_slice(&bw_hz.to_le_bytes());
            Ok(5)
        }
        FACET_RADIO_SQUELCH => {
            out[0] = radio_state.squelch_level;
            out[1..3].copy_from_slice(&hw.radio.get_rssi().to_le_bytes());
            out[3] = hw.radio.is_squelch_open() as u8;
            Ok(4)
        }
        FACET_RADIO_TRANSMIT => {
            out[0] = vfo.tx_active as u8;
            let p = match ch.power {
                RfPowerLevel::High => 2,
                RfPowerLevel::Medium => 1,
                _ => 0,
            };
            out[1] = p;
            Ok(2)
        }
        FACET_RADIO_DIGITAL => {
            out[..2].copy_from_slice(&0i16.to_le_bytes());
            Ok(2)
        }
        _ => Err(WireStatus::UnsupportedField),
    }
}

#[inline(never)]
fn read_ui_facet(facet: u16, hw: &SystemHardware, out: &mut [u8]) -> Result<usize, WireStatus> {
    match facet {
        FACET_UI_SURFACE => {
            out[..2].copy_from_slice(&128u16.to_le_bytes());
            out[2..4].copy_from_slice(&64u16.to_le_bytes());
            out[4] = 1;
            out[5..7].copy_from_slice(&1024u16.to_le_bytes());
            Ok(7)
        }
        FACET_UI_MODAL => {
            let modal =
                unsafe { &*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL) };
            out[0] = modal.is_active() as u8;
            out[1] = modal.tag;
            Ok(2)
        }
        FACET_UI_VIEW => {
            out[0] = hw.orchestrator.active_app_id() as u8;
            Ok(1)
        }
        _ => Err(WireStatus::UnsupportedField),
    }
}

#[inline(never)]
fn on_handle_get(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware, now: u32) -> bool {
    let req = match HandleGetReq::parse(payload) {
        Ok(r) => r,
        Err(status) => return send_status(hw, header, status),
    };

    let mut resp = [0u8; 100];
    let val_len = match req.handle.domain {
        DOMAIN_SYSTEM => read_sys_facet(req.handle.facet, hw, now, &mut resp[4..]),
        DOMAIN_RADIO => read_radio_facet(req.handle.facet, req.handle.instance, hw, &mut resp[4..]),
        DOMAIN_UI => read_ui_facet(req.handle.facet, hw, &mut resp[4..]),
        DOMAIN_INPUT => match req.handle.facet {
            FACET_INPUT_LOCK => {
                resp[4] = hw.orchestrator.is_keypad_locked() as u8;
                Ok(1)
            }
            _ => Err(WireStatus::UnsupportedField),
        },
        DOMAIN_CONFIG => {
            let domain_idx = req.handle.instance as usize;
            if domain_idx > 5 {
                return send_status(hw, header, WireStatus::BadRange);
            }
            let cfg_slice = crate::master_config_ref().config_bytes(domain_idx);
            let len = cfg_slice.len().min(resp.len() - 4);
            resp[4..4 + len].copy_from_slice(&cfg_slice[..len]);
            Ok(len)
        }
        DOMAIN_APP => match req.handle.facet {
            FACET_APP_REGISTRY => {
                resp[4] = crate::AppRegistry::app_count() as u8;
                Ok(1)
            }
            FACET_APP_LIFECYCLE => {
                resp[4] = hw.orchestrator.active_app_id() as u8;
                Ok(1)
            }
            _ => Err(WireStatus::UnsupportedField),
        },
        DOMAIN_STORAGE => {
            let stats = hw.filesystem.stats();
            resp[4..6].copy_from_slice(&stats.active_files.to_le_bytes());
            resp[6..8].copy_from_slice(&stats.used_sectors.to_le_bytes());
            resp[8..10].copy_from_slice(&stats.free_sectors.to_le_bytes());
            resp[10..12].copy_from_slice(&stats.deleted_sectors.to_le_bytes());
            resp[12..16].copy_from_slice(&stats.wear_cycles.to_le_bytes());
            resp[16..18].copy_from_slice(&stats.total_sectors.to_le_bytes());
            Ok(14)
        }
        _ => Err(WireStatus::Unsupported),
    };

    let len = match val_len {
        Ok(l) => l,
        Err(status) => return send_status(hw, header, status),
    };

    req.handle.write_bytes(&mut resp[..4]);
    send_resp(hw, header, WireStatus::Ok, &resp[..4 + len])
}

#[inline(never)]
fn set_radio_facet(
    facet: u16,
    instance: u8,
    val: &[u8],
    hw: &mut SystemHardware,
) -> Result<(), WireStatus> {
    let radio_state = crate::radio_state();
    let vfo = match instance {
        RADIO_INSTANCE_VFO_A => &mut radio_state.vfo_a,
        RADIO_INSTANCE_VFO_B => &mut radio_state.vfo_b,
        _ => radio_state.selected_vfo_mut(),
    };
    match facet {
        FACET_RADIO_TUNING => {
            let freq_hz = get_u32_le(val)?;
            if !(18_000_000..=1_300_000_000).contains(&freq_hz) {
                return Err(WireStatus::BadRange);
            }
            vfo.frequency = Frequency::new(freq_hz);
            let _ = hw.radio.set_frequency(Frequency::new(freq_hz));
            Ok(())
        }
        FACET_RADIO_MODULATION => {
            let mode_u8 = get_u8(val)?;
            let mode = ModulationMode::from_u8(mode_u8);
            vfo.mod_type = mode.as_str();
            hw.radio.set_modulation(mode);
            Ok(())
        }
        FACET_RADIO_SQUELCH => {
            let sql = get_u8(val)?.min(9);
            radio_state.squelch_level = sql;
            hw.radio.set_squelch_level(sql);
            Ok(())
        }
        FACET_RADIO_TRANSMIT => {
            let should_tx = get_u8(val)? != 0;
            vfo.tx_active = should_tx;
            if should_tx {
                hw.radio.tx_prepare();
                hw.radio.tx_ramp_up(1);
                hw.radio.tx_unmute();
            } else {
                hw.radio.tx_mute();
                hw.radio.tx_ramp_down();
                hw.radio.tx_conclude();
            }
            Ok(())
        }
        FACET_RADIO_DIGITAL => {
            let offset_dhz = get_i16_le(val)?;
            let freq = vfo.frequency.raw();
            hw.radio.set_sub_hz_offset(freq, offset_dhz);
            Ok(())
        }
        _ => Err(WireStatus::UnsupportedField),
    }
}

#[inline(never)]
fn handle_resource_set(req: &HandleSetReq, hw: &mut SystemHardware) -> Result<(), WireStatus> {
    match req.handle.domain {
        DOMAIN_SYSTEM => match req.handle.facet {
            FACET_SYS_METRICS => {
                if let Some(&lock) = req.value.first() {
                    let should_lock = lock != 0;
                    if should_lock != hw.orchestrator.is_keypad_locked() {
                        hw.orchestrator.toggle_keypad_lock();
                    }
                }
                Ok(())
            }
            _ => Err(WireStatus::UnsupportedField),
        },
        DOMAIN_RADIO => set_radio_facet(req.handle.facet, req.handle.instance, req.value, hw),
        DOMAIN_INPUT => match req.handle.facet {
            FACET_INPUT_KEY => {
                if req.value.is_empty() {
                    return Err(WireStatus::BadLength);
                }
                let code_u8 = req.value[0];
                let state_u8 = req.value.get(1).copied().unwrap_or(0);
                if let Some(code) = KeyCode::from_u8(code_u8) {
                    let state = match state_u8 {
                        0 => KeyState::Pressed,
                        1 => KeyState::Held,
                        _ => KeyState::Released,
                    };
                    crate::system::enqueue_event(Event::Key { code, state });
                    Ok(())
                } else {
                    Err(WireStatus::BadRange)
                }
            }
            FACET_INPUT_LOCK => {
                let should_lock = get_u8(req.value)? != 0;
                if should_lock != hw.orchestrator.is_keypad_locked() {
                    hw.orchestrator.toggle_keypad_lock();
                }
                Ok(())
            }
            _ => Err(WireStatus::UnsupportedField),
        },
        DOMAIN_CONFIG => {
            let domain_idx = req.handle.instance as usize;
            if domain_idx > 5 {
                return Err(WireStatus::BadRange);
            }
            let mcfg = crate::master_config();
            let dest = mcfg.config_bytes_mut(domain_idx);
            let copy_len = req.value.len().min(dest.len());
            dest[..copy_len].copy_from_slice(&req.value[..copy_len]);
            mcfg.seal(domain_idx);
            mcfg.dirty |= 1 << domain_idx;
            let _ = mcfg.flush_dirty(hw.filesystem);
            Ok(())
        }
        _ => Err(WireStatus::Unsupported),
    }
}

#[inline(never)]
fn on_handle_set(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match HandleSetReq::parse(payload) {
        Ok(r) => r,
        Err(status) => return send_status(hw, header, status),
    };

    let status = match handle_resource_set(&req, hw) {
        Ok(()) => WireStatus::Ok,
        Err(s) => s,
    };

    let mut resp = [0u8; 4];
    req.handle.write_bytes(&mut resp);
    send_resp(hw, header, status, &resp)
}

#[inline(never)]
fn on_handle_invoke(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match HandleInvokeReq::parse(payload) {
        Ok(r) => r,
        Err(status) => return send_status(hw, header, status),
    };

    let status = match req.handle.domain {
        DOMAIN_SYSTEM => match req.action_id {
            ACTION_SYS_REBOOT => {
                kernel::sys::SystemDiagnostics::reboot();
            }
            ACTION_SYS_FLASHLIGHT => {
                let mode = req.params.first().copied().unwrap_or(2);
                match mode {
                    0 => bsp::pins::FLASHLIGHT.set_low(),
                    1 => bsp::pins::FLASHLIGHT.set_high(),
                    _ => {
                        if bsp::pins::FLASHLIGHT.is_high() {
                            bsp::pins::FLASHLIGHT.set_low();
                        } else {
                            bsp::pins::FLASHLIGHT.set_high();
                        }
                    }
                }
                WireStatus::Ok
            }
            _ => WireStatus::Unsupported,
        },
        DOMAIN_RADIO => match req.action_id {
            ACTION_RADIO_VFO_TOGGLE => {
                let radio_state = crate::radio_state();
                radio_state.active_vfo = 1 - radio_state.active_vfo;
                WireStatus::Ok
            }
            _ => WireStatus::Unsupported,
        },
        DOMAIN_UI => match req.action_id {
            ACTION_MODAL_DISMISS => {
                unsafe {
                    (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                        .dismiss();
                }
                WireStatus::Ok
            }
            ACTION_MODAL_CONFIRM => {
                crate::system::enqueue_event(Event::Key {
                    code: KeyCode::Menu,
                    state: KeyState::Pressed,
                });
                WireStatus::Ok
            }
            ACTION_MODAL_CANCEL => {
                crate::system::enqueue_event(Event::Key {
                    code: KeyCode::Exit,
                    state: KeyState::Pressed,
                });
                WireStatus::Ok
            }
            ACTION_VIEW_LAUNCHER => {
                crate::system::enqueue_event(Event::Key {
                    code: KeyCode::Exit,
                    state: KeyState::Held,
                });
                WireStatus::Ok
            }
            _ => WireStatus::Unsupported,
        },
        DOMAIN_INPUT => match req.action_id {
            ACTION_INPUT_LOCK_TOGGLE => {
                hw.orchestrator.toggle_keypad_lock();
                WireStatus::Ok
            }
            _ => WireStatus::Unsupported,
        },
        DOMAIN_APP => match req.action_id {
            ACTION_APP_LAUNCH => {
                let app_id = req.params.first().copied().unwrap_or(0);
                if (app_id as usize) < crate::AppRegistry::app_count() {
                    WireStatus::Ok
                } else {
                    WireStatus::BadRange
                }
            }
            ACTION_APP_KILL => {
                crate::system::enqueue_event(Event::Key {
                    code: KeyCode::Exit,
                    state: KeyState::Held,
                });
                WireStatus::Ok
            }
            _ => WireStatus::Unsupported,
        },
        _ => WireStatus::Unsupported,
    };

    let mut resp = [0u8; 6];
    req.handle.write_bytes(&mut resp[..4]);
    resp[4..6].copy_from_slice(&req.action_id.to_le_bytes());

    send_resp(hw, header, status, &resp)
}

#[inline(never)]
fn on_handle_stream(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match HandleStreamReq::parse(payload) {
        Ok(r) => r,
        Err(status) => return send_status(hw, header, status),
    };

    if req.handle.domain == DOMAIN_UI && req.handle.facet == FACET_UI_SURFACE {
        let canvas = crate::system_canvas();
        let flat = canvas.buffer_flat();
        let total_sz = flat.len();
        let off = (req.offset as usize).min(total_sz);
        let len = (req.length as usize).min(100).min(total_sz - off);

        let mut resp = [0u8; 10 + 100];
        req.handle.write_bytes(&mut resp[..4]);
        resp[4..8].copy_from_slice(&(off as u32).to_le_bytes());
        resp[8..10].copy_from_slice(&(len as u16).to_le_bytes());
        resp[10..10 + len].copy_from_slice(&flat[off..off + len]);

        send_resp(hw, header, WireStatus::Ok, &resp[..10 + len])
    } else {
        send_status(hw, header, WireStatus::Unsupported)
    }
}

#[inline(never)]
fn handle_control_service(
    header: &WireHeader,
    payload: &[u8],
    hw: &mut SystemHardware,
    now: u32,
) -> bool {
    match header.opcode {
        OP_HANDLE_GET => on_handle_get(header, payload, hw, now),
        OP_HANDLE_SET => on_handle_set(header, payload, hw),
        OP_HANDLE_INVOKE => on_handle_invoke(header, payload, hw),
        OP_HANDLE_STREAM => on_handle_stream(header, payload, hw),
        _ => false,
    }
}

// ── SERVICE_DATA (0x03): Hardened KernFS Streaming ─────────────────────────

#[inline(never)]
fn validate_file_path(path: &str, is_write: bool) -> Result<(), WireStatus> {
    let b = path.as_bytes();
    if b.is_empty() || b.len() > 32 || b[0] != b'/' {
        return Err(WireStatus::BadRange);
    }
    for i in 1..b.len() {
        if (b[i - 1] == b'.' && b[i] == b'.') || (b[i - 1] == b'/' && b[i] == b'/') {
            return Err(WireStatus::BadRange);
        }
    }
    let is_sys = path.starts_with("/sys/");
    if !(path.starts_with("/user/")
        || path.starts_with("/pref/")
        || path.starts_with("/apps/")
        || is_sys)
    {
        return Err(WireStatus::SafetyDenied);
    }
    if is_write && is_sys && path != CALIB_FILE_PATH {
        return Err(WireStatus::SafetyDenied);
    }
    Ok(())
}

#[inline(never)]
fn on_transfer_open(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferOpenRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    if let Err(status) = validate_file_path(req.path, req.mode == TRANSFER_MODE_WRITE) {
        return send_status(hw, header, status);
    }

    if req.mode == TRANSFER_MODE_WRITE {
        if req.total_size as usize > MAX_WIRE_FILE_SIZE {
            return send_status(hw, header, WireStatus::BadLength);
        }
        let stats = hw.filesystem.stats();
        if stats.free_sectors < 4 {
            return send_status(hw, header, WireStatus::Failed);
        }
    }

    let existing_size = hw.filesystem.file_size(req.path).unwrap_or(0) as u32;

    let tid = unsafe {
        let next_ptr = core::ptr::addr_of_mut!(NEXT_TRANSFER_ID);
        let id = *next_ptr;
        *next_ptr = (*next_ptr).wrapping_add(1).max(1);
        id
    };

    let chunk_size = req.chunk_size.clamp(16, 200);

    let path_bytes = req.path.as_bytes();
    let mut path_buf = [0u8; 32];
    path_buf[..path_bytes.len()].copy_from_slice(path_bytes);

    unsafe {
        *core::ptr::addr_of_mut!(CURRENT_TRANSFER) = ActiveTransfer {
            active: true,
            transfer_id: tid,
            mode: req.mode,
            total_size: req.total_size,
            current_len: 0,
            chunk_size,
            path: path_buf,
            path_len: path_bytes.len() as u8,
        };
    }

    let mut resp = [0u8; 8];
    resp[0..2].copy_from_slice(&tid.to_le_bytes());
    resp[2..4].copy_from_slice(&chunk_size.to_le_bytes());
    resp[4..8].copy_from_slice(&existing_size.to_le_bytes());

    send_resp(hw, header, WireStatus::Ok, &resp)
}

#[inline(never)]
fn on_transfer_chunk(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferChunkRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    let transfer = match active_transfer_mut(req.transfer_id) {
        Some(t) => t,
        None => return send_status(hw, header, WireStatus::BadSession),
    };

    if transfer.mode == TRANSFER_MODE_WRITE {
        let off = req.offset as usize;
        let chunk_len = req.data.len();
        if off + chunk_len > MAX_WIRE_FILE_SIZE {
            return send_status(hw, header, WireStatus::BadLength);
        }

        let staging = wire_file_staging_buf();
        staging[off..off + chunk_len].copy_from_slice(req.data);
        transfer.current_len = transfer.current_len.max((off + chunk_len) as u32);

        let mut resp = [0u8; 8];
        resp[0..2].copy_from_slice(&req.transfer_id.to_le_bytes());
        resp[2..4].copy_from_slice(&(chunk_len as u16).to_le_bytes());
        resp[4..8].copy_from_slice(&((off + chunk_len) as u32).to_le_bytes());

        send_resp(hw, header, WireStatus::Ok, &resp)
    } else {
        let path = transfer.path_str();
        let off = req.offset as usize;
        let len = (req.length as usize).min(180);

        let mut resp = [0u8; 8 + 180];
        let read_sz = hw
            .filesystem
            .read_file_at(path, off, &mut resp[8..8 + len])
            .unwrap_or(0);
        resp[0..2].copy_from_slice(&req.transfer_id.to_le_bytes());
        resp[2..6].copy_from_slice(&(off as u32).to_le_bytes());
        resp[6..8].copy_from_slice(&(read_sz as u16).to_le_bytes());

        send_resp(hw, header, WireStatus::Ok, &resp[..8 + read_sz])
    }
}

#[inline(never)]
fn commit_calibration_payload(
    staging: &[u8],
    total_len: usize,
    hw: &mut SystemHardware,
) -> Result<(), WireStatus> {
    if total_len != 512 {
        return Err(WireStatus::SafetyDenied);
    }
    let calib_slice: &[u8; 512] = match (&staging[..512]).try_into() {
        Ok(s) => s,
        Err(_) => return Err(WireStatus::SafetyDenied),
    };
    if !is_valid_calibration(calib_slice) {
        return Err(WireStatus::SafetyDenied);
    }

    let mcu_uid = bsp::uid::read_mcu_uid();
    let calib = unsafe { &mut *core::ptr::addr_of_mut!(crate::CALIBRATION) };

    if calib
        .commit_with_backup(hw.filesystem, calib_slice, &mcu_uid)
        .is_err()
    {
        return Err(WireStatus::IoError);
    }

    // Dynamically reload calibrated parameters into live silicon
    let pwr = unsafe { &mut *core::ptr::addr_of_mut!(crate::POWER_SERVICE) };
    pwr.battery_driver.calib_const = calib.battery_calib_const();
    pwr.battery_driver
        .load_calibration_curve(calib.battery_curve());
    hw.radio.attach_calibration(calib.raw_bytes());
    hw.radio.set_xtal_freq_trim(calib.xtal_freq_low());
    Ok(())
}

fn on_transfer_close(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferCloseRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    let transfer = match active_transfer_mut(req.transfer_id) {
        Some(t) => t,
        None => return send_status(hw, header, WireStatus::BadSession),
    };

    transfer.active = false;
    let mut resp = [0u8; 6];
    resp[0..2].copy_from_slice(&req.transfer_id.to_le_bytes());

    if transfer.mode == TRANSFER_MODE_WRITE {
        let total_len = transfer.current_len as usize;
        let staging = wire_file_staging_buf();
        let actual_crc = crypto::crc::CrcEngine::crc32(&staging[..total_len]);

        if req.expected_crc32 != 0 && req.expected_crc32 != actual_crc {
            return send_status(hw, header, WireStatus::Failed);
        }

        let path = transfer.path_str();

        // ── HARDENED CALIBRATION COMMIT WITH VALIDATION & ROLLBACK ──
        if path == CALIB_FILE_PATH {
            if let Err(err) = commit_calibration_payload(staging, total_len, hw) {
                return send_status(hw, header, err);
            }
        } else if hw
            .filesystem
            .write_file(path, &staging[..total_len])
            .is_err()
        {
            return send_status(hw, header, WireStatus::IoError);
        }

        resp[2..6].copy_from_slice(&actual_crc.to_le_bytes());
    }

    send_resp(hw, header, WireStatus::Ok, &resp)
}

#[inline(never)]
fn on_transfer_abort(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferAbortRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    if let Some(transfer) = active_transfer_mut(req.transfer_id) {
        transfer.active = false;
    }

    let mut resp = [0u8; 2];
    resp.copy_from_slice(&req.transfer_id.to_le_bytes());
    send_resp(hw, header, WireStatus::Ok, &resp)
}

#[inline(never)]
fn on_transfer_stat(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferStatRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    let mut reply = [0u8; 10];
    let status = match hw.filesystem.file_size(req.path) {
        Ok(sz) => {
            reply[0] = 1; // exists
            reply[1] = 0; // is_dir = false
            reply[2..6].copy_from_slice(&(sz as u32).to_le_bytes());

            // Compute CRC-32 by streaming chunks across full file
            let mut crc_val = 0xFFFF_FFFFu32;
            let mut cur_off = 0usize;
            let mut chunk_buf = [0u8; 128];
            while cur_off < sz {
                let to_read = (sz - cur_off).min(chunk_buf.len());
                match hw
                    .filesystem
                    .read_file_at(req.path, cur_off, &mut chunk_buf[..to_read])
                {
                    Ok(n) if n > 0 => {
                        crc_val = crypto::crc::CrcEngine::crc32_update(crc_val, &chunk_buf[..n]);
                        cur_off += n;
                    }
                    _ => break,
                }
            }
            crc_val = !crc_val;
            reply[6..10].copy_from_slice(&crc_val.to_le_bytes());
            WireStatus::Ok
        }
        Err(_) => {
            reply[0] = 0; // not found
            WireStatus::NotFound
        }
    };
    send_resp(hw, header, status, &reply)
}

#[inline(never)]
fn on_transfer_list(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferListRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    let mut entries = [const { DirEntryInfo::empty() }; 12];
    let count = hw.filesystem.list_dir(req.path, &mut entries).unwrap_or(0);

    let mut out = [0u8; 160];
    let next_cursor = 0xFFFFu16;
    out[0..2].copy_from_slice(&next_cursor.to_le_bytes());
    out[2] = count as u8;
    let mut off = 3;

    for entry in &entries[..count] {
        let name = entry.path.as_str();
        let b = name.as_bytes();
        let name_len = b.len().min(24);
        if off + 6 + name_len > out.len() {
            break;
        }
        out[off] = entry.is_dir as u8;
        out[off + 1..off + 5].copy_from_slice(&(entry.size as u32).to_le_bytes());
        out[off + 5] = name_len as u8;
        out[off + 6..off + 6 + name_len].copy_from_slice(&b[..name_len]);
        off += 6 + name_len;
    }

    send_resp(hw, header, WireStatus::Ok, &out[..off])
}

#[inline(never)]
fn on_transfer_remove(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferRemoveRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    if let Err(status) = validate_file_path(req.path, true) {
        return send_status(hw, header, status);
    }
    if req.path == CALIB_FILE_PATH {
        return send_status(hw, header, WireStatus::SafetyDenied);
    }

    let status = match hw.filesystem.remove(req.path) {
        Ok(_) => WireStatus::Ok,
        Err(_) => WireStatus::NotFound,
    };

    send_status(hw, header, status)
}

#[inline(never)]
fn handle_data_service(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    match header.opcode {
        OP_TRANSFER_OPEN => on_transfer_open(header, payload, hw),
        OP_TRANSFER_CHUNK => on_transfer_chunk(header, payload, hw),
        OP_TRANSFER_CLOSE => on_transfer_close(header, payload, hw),
        OP_TRANSFER_ABORT => on_transfer_abort(header, payload, hw),
        OP_TRANSFER_STAT => on_transfer_stat(header, payload, hw),
        OP_TRANSFER_LIST => on_transfer_list(header, payload, hw),
        OP_TRANSFER_REMOVE => on_transfer_remove(header, payload, hw),
        _ => false,
    }
}

// ── SERVICE_EVENTS (0x02) ────────────────────────────────────────────────────
#[inline(never)]
fn handle_events_service(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    match header.opcode {
        OP_EVENTS_SUBSCRIBE => {
            let req = match EventSubscribeReq::parse(payload) {
                Ok(r) => r,
                Err(status) => return send_status(hw, header, status),
            };

            unsafe {
                let sub = &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION);
                sub.active_mask = req.event_mask;
                sub.credits = req.initial_credits;
            }

            send_status(hw, header, WireStatus::Ok)
        }

        OP_EVENTS_UNSUBSCRIBE => {
            unsafe {
                let sub = &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION);
                sub.active_mask = 0;
                sub.credits = 0;
            }

            send_status(hw, header, WireStatus::Ok)
        }

        OP_EVENTS_CREDIT => {
            if payload.len() >= 2 {
                let grant = u16::from_le_bytes([payload[0], payload[1]]);
                unsafe {
                    let sub = &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION);
                    sub.credits = sub.credits.saturating_add(grant);
                }
                send_status(hw, header, WireStatus::Ok)
            } else {
                send_status(hw, header, WireStatus::BadLength)
            }
        }

        _ => false,
    }
}

/// Asynchronously emit an event frame over RigWire if the host holds a valid subscription with credits.
pub fn emit_event(hw: &mut SystemHardware, event_id: u16, data: &[u8]) {
    unsafe {
        let sub = &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION);
        let bit = 1u32 << (event_id & 0x1F);
        if (sub.active_mask & bit) != 0 && sub.credits > 0 {
            sub.credits -= 1;
            let mut payload = [0u8; 32];
            payload[0..2].copy_from_slice(&event_id.to_le_bytes());
            let d_len = data.len().min(30);
            payload[2..2 + d_len].copy_from_slice(&data[..d_len]);
            let _ = hw
                .wire
                .send_event(SERVICE_EVENTS, OP_EVENTS_EMIT, 0, &payload[..2 + d_len]);
        }
    }
}
