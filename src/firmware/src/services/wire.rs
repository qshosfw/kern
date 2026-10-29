//! Declarative Firmware RigWire Service & Component Dispatcher.
//!
//! Bridges RigWire serial transport with microkernel traits, firmware services,
//! hardware peripherals, and hosted applications.
//!
//! Subsystems Handled:
//! - SERVICE_CONTROL (0x01): Declarative routing to System (0x00), Radio (0x01),
//!   Power (0x02), Audio (0x03), and Hosted Applications (0x10+).
//! - SERVICE_DATA (0x03): Chunked file streaming with strict pre-commit validation,
//!   automatic `/sys/calib.bak` backup, readback verification, and rollback.
//! - SERVICE_EVENTS (0x02): Host event subscriptions and asynchronous event streaming.

use crate::hardware::SystemHardware;
use crate::services::calibration::{CALIB_FILE_PATH, is_valid_calibration};
use crate::services::storage::DirEntryInfo;
use kernel::service::{PowerService, RfSquelch};
use kernel::wire::*;
use utils::units::Frequency;

pub const STAGING_BUF_CAP: usize = 512;

/// Staging buffer for atomic non-volatile commits (up to 512 bytes).
/// Reuses the shared transient arena so it incurs strictly ZERO bytes of Flash OR SRAM overhead.
#[inline(always)]
pub fn staging_buf() -> &'static mut [u8; STAGING_BUF_CAP] {
    let arena = crate::runtime::native_runner::transient_arena_bytes_mut();
    unsafe { &mut *(arena.as_mut_ptr() as *mut [u8; STAGING_BUF_CAP]) }
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

// ── SERVICE_CONTROL (0x01) ───────────────────────────────────────────────────

#[inline(never)]
fn on_control_state_get(
    header: &WireHeader,
    payload: &[u8],
    hw: &mut SystemHardware,
    now: u32,
) -> bool {
    let req = match StateGetReq::parse(payload) {
        Ok(r) => r,
        Err(status) => return send_status(hw, header, status),
    };

    let mut val_buf = [0u8; 16];
    let val_len = match req.component_id {
        COMPONENT_SYSTEM => match req.property_id {
            PROP_SYS_UPTIME_MS => {
                val_buf[..4].copy_from_slice(&now.to_le_bytes());
                4
            }
            PROP_SYS_KEYPAD_LOCKED => {
                val_buf[0] = hw.orchestrator.is_keypad_locked() as u8;
                1
            }
            PROP_SYS_FLASHLIGHT_ACTIVE => {
                val_buf[0] = bsp::pins::FLASHLIGHT.is_high() as u8;
                1
            }
            PROP_SYS_ACTIVE_APP => {
                val_buf[0] = hw.orchestrator.active_app_id() as u8;
                1
            }
            _ => 0,
        },
        COMPONENT_RADIO => {
            let radio_state = crate::radio_state_ref();
            let vfo = radio_state.selected_vfo();
            match req.property_id {
                PROP_RADIO_RX_FREQ_HZ | PROP_RADIO_TX_FREQ_HZ => {
                    val_buf[..4].copy_from_slice(&vfo.frequency.raw().to_le_bytes());
                    4
                }
                PROP_RADIO_SELECTED_VFO => {
                    val_buf[0] = radio_state.active_vfo;
                    1
                }
                PROP_RADIO_POWER_LEVEL => {
                    let p_bytes = vfo.power.as_bytes();
                    let len = p_bytes.len().min(4);
                    val_buf[..len].copy_from_slice(&p_bytes[..len]);
                    len
                }
                PROP_RADIO_SQUELCH_LEVEL => {
                    val_buf[0] = radio_state.squelch_level;
                    1
                }
                PROP_RADIO_PTT_ACTIVE => {
                    val_buf[0] = vfo.tx_active as u8;
                    1
                }
                PROP_RADIO_RSSI_DBM => {
                    let rssi = hw.radio.get_rssi();
                    val_buf[..2].copy_from_slice(&rssi.to_le_bytes());
                    2
                }
                _ => 0,
            }
        }
        COMPONENT_POWER => {
            let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
            match req.property_id {
                PROP_RADIO_BATTERY_MV => {
                    val_buf[..2].copy_from_slice(&pwr.battery_voltage_mv().to_le_bytes());
                    2
                }
                _ => 0,
            }
        }
        COMPONENT_AUDIO => match req.property_id {
            PROP_AUDIO_VOLUME => {
                val_buf[0] = crate::master_config_ref().audio.volume;
                1
            }
            _ => 0,
        },
        _ => 0,
    };

    if val_len == 0 {
        return send_status(hw, header, WireStatus::UnsupportedField);
    }

    let mut resp = [0u8; 24];
    resp[0] = req.component_id;
    resp[1] = 0; // flags
    resp[2..6].copy_from_slice(&1u32.to_le_bytes()); // revision
    resp[6..8].copy_from_slice(&req.property_id.to_le_bytes());
    resp[8..8 + val_len].copy_from_slice(&val_buf[..val_len]);

    send_resp(hw, header, WireStatus::Ok, &resp[..8 + val_len])
}

#[inline(never)]
fn on_control_state_set(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match StateSetReq::parse(payload) {
        Ok(r) => r,
        Err(status) => return send_status(hw, header, status),
    };

    let val = req.value_bytes;
    let status = match req.component_id {
        COMPONENT_SYSTEM => match req.property_id {
            PROP_SYS_FLASHLIGHT_ACTIVE => {
                if let Some(&b) = val.first() {
                    if b != 0 {
                        bsp::pins::FLASHLIGHT.set_high();
                    } else {
                        bsp::pins::FLASHLIGHT.set_low();
                    }
                    WireStatus::Ok
                } else {
                    WireStatus::BadLength
                }
            }
            PROP_SYS_KEYPAD_LOCKED => {
                if let Some(&b) = val.first() {
                    let should_lock = b != 0;
                    if should_lock != hw.orchestrator.is_keypad_locked() {
                        hw.orchestrator.toggle_keypad_lock();
                    }
                    WireStatus::Ok
                } else {
                    WireStatus::BadLength
                }
            }
            _ => WireStatus::UnsupportedField,
        },
        COMPONENT_RADIO => {
            let radio_state = crate::radio_state();
            match req.property_id {
                PROP_RADIO_RX_FREQ_HZ | PROP_RADIO_TX_FREQ_HZ => {
                    if val.len() >= 4 {
                        let freq_hz = u32::from_le_bytes([val[0], val[1], val[2], val[3]]);
                        if (18_000_000..=1_300_000_000).contains(&freq_hz) {
                            let vfo = radio_state.selected_vfo_mut();
                            vfo.frequency = Frequency::new(freq_hz);
                            let _ = hw.radio.set_frequency(Frequency::new(freq_hz));
                            WireStatus::Ok
                        } else {
                            WireStatus::BadRange
                        }
                    } else {
                        WireStatus::BadLength
                    }
                }
                PROP_RADIO_SELECTED_VFO => {
                    if let Some(&vfo_idx) = val.first() {
                        if vfo_idx <= 1 {
                            radio_state.active_vfo = vfo_idx;
                            WireStatus::Ok
                        } else {
                            WireStatus::BadRange
                        }
                    } else {
                        WireStatus::BadLength
                    }
                }
                PROP_RADIO_SQUELCH_LEVEL => {
                    if let Some(&sql) = val.first() {
                        let sql = sql.min(9);
                        radio_state.squelch_level = sql;
                        hw.radio.set_squelch_level(sql);
                        WireStatus::Ok
                    } else {
                        WireStatus::BadLength
                    }
                }
                _ => WireStatus::UnsupportedField,
            }
        }
        COMPONENT_AUDIO => match req.property_id {
            PROP_AUDIO_VOLUME => {
                if let Some(&vol) = val.first() {
                    hw.audio.set_volume(vol.min(63));
                    WireStatus::Ok
                } else {
                    WireStatus::BadLength
                }
            }
            _ => WireStatus::UnsupportedField,
        },
        _ => WireStatus::Unsupported,
    };

    let mut resp = [0u8; 8];
    resp[0] = req.component_id;
    resp[1] = 0;
    resp[2..6].copy_from_slice(&2u32.to_le_bytes()); // new revision
    resp[6..8].copy_from_slice(&req.property_id.to_le_bytes());

    send_resp(hw, header, status, &resp)
}

#[inline(never)]
fn on_control_action_invoke(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match ActionInvokeReq::parse(payload) {
        Ok(r) => r,
        Err(status) => return send_status(hw, header, status),
    };

    let status = match req.component_id {
        COMPONENT_SYSTEM => match req.action_id {
            ACTION_FLASHLIGHT_TOGGLE => {
                if bsp::pins::FLASHLIGHT.is_high() {
                    bsp::pins::FLASHLIGHT.set_low();
                } else {
                    bsp::pins::FLASHLIGHT.set_high();
                }
                WireStatus::Ok
            }
            ACTION_KEYPAD_LOCK_TOGGLE => {
                hw.orchestrator.toggle_keypad_lock();
                WireStatus::Ok
            }
            ACTION_REBOOT => {
                kernel::sys::SystemDiagnostics::reboot();
            }
            _ => WireStatus::Unsupported,
        },
        COMPONENT_RADIO => match req.action_id {
            ACTION_VFO_TOGGLE => {
                let radio_state = crate::radio_state();
                radio_state.active_vfo = 1 - radio_state.active_vfo;
                WireStatus::Ok
            }
            _ => WireStatus::Unsupported,
        },
        COMPONENT_AUDIO => match req.action_id {
            ACTION_PLAY_BEEP => {
                hw.radio.play_tone(1000, 100);
                WireStatus::Ok
            }
            _ => WireStatus::Unsupported,
        },
        _ => WireStatus::Unsupported,
    };

    let mut resp = [0u8; 4];
    resp[0] = req.component_id;
    resp[1..3].copy_from_slice(&req.action_id.to_le_bytes());

    send_resp(hw, header, status, &resp)
}

#[inline(never)]
fn handle_control_service(
    header: &WireHeader,
    payload: &[u8],
    hw: &mut SystemHardware,
    now: u32,
) -> bool {
    match header.opcode {
        OP_CONTROL_STATE_GET => on_control_state_get(header, payload, hw, now),
        OP_CONTROL_STATE_SET => on_control_state_set(header, payload, hw),
        OP_CONTROL_ACTION_INVOKE => on_control_action_invoke(header, payload, hw),
        _ => false,
    }
}

// ── SERVICE_DATA (0x03) ──────────────────────────────────────────────────────

#[inline(never)]
fn on_transfer_open(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferOpenRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    let path_bytes = req.path.as_bytes();
    if path_bytes.is_empty()
        || path_bytes.len() > 32
        || !req.path.starts_with('/')
        || req.path.contains("..")
    {
        return send_status(hw, header, WireStatus::BadRange);
    }

    if req.mode == TRANSFER_MODE_WRITE && req.total_size as usize > STAGING_BUF_CAP {
        return send_status(hw, header, WireStatus::BadLength);
    }

    let existing_size = hw.filesystem.file_size(req.path).unwrap_or(0) as u32;

    let tid = unsafe {
        let next_ptr = core::ptr::addr_of_mut!(NEXT_TRANSFER_ID);
        let id = *next_ptr;
        *next_ptr = (*next_ptr).wrapping_add(1).max(1);
        id
    };

    let chunk_size = req.chunk_size.clamp(16, 200);

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

    let transfer = unsafe { &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER) };
    if !transfer.active || transfer.transfer_id != req.transfer_id {
        return send_status(hw, header, WireStatus::BadSession);
    }

    if transfer.mode == TRANSFER_MODE_WRITE {
        let off = req.offset as usize;
        let chunk_len = req.data.len();
        if off + chunk_len > STAGING_BUF_CAP {
            return send_status(hw, header, WireStatus::BadLength);
        }

        let staging = staging_buf();
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

        let mut file_buf = [0u8; 256];
        let read_sz = hw.filesystem.read_file(path, &mut file_buf).unwrap_or(0);

        let chunk = if off < read_sz {
            let end = (off + len).min(read_sz);
            &file_buf[off..end]
        } else {
            &[]
        };

        let mut resp = [0u8; 8 + 180];
        resp[0..2].copy_from_slice(&req.transfer_id.to_le_bytes());
        resp[2..6].copy_from_slice(&(off as u32).to_le_bytes());
        resp[6..8].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
        resp[8..8 + chunk.len()].copy_from_slice(chunk);

        send_resp(hw, header, WireStatus::Ok, &resp[..8 + chunk.len()])
    }
}

#[inline(never)]
fn on_transfer_close(header: &WireHeader, payload: &[u8], hw: &mut SystemHardware) -> bool {
    let req = match TransferCloseRequest::parse(payload) {
        Some(r) => r,
        None => return send_status(hw, header, WireStatus::BadLength),
    };

    let transfer = unsafe { &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER) };
    if !transfer.active || transfer.transfer_id != req.transfer_id {
        return send_status(hw, header, WireStatus::BadSession);
    }

    transfer.active = false;
    let mut resp = [0u8; 6];
    resp[0..2].copy_from_slice(&req.transfer_id.to_le_bytes());

    if transfer.mode == TRANSFER_MODE_WRITE {
        let total_len = transfer.current_len as usize;
        let staging = staging_buf();
        let actual_crc = crypto::crc::CrcEngine::crc32(&staging[..total_len]);

        if req.expected_crc32 != 0 && req.expected_crc32 != actual_crc {
            return send_status(hw, header, WireStatus::Failed);
        }

        let path = transfer.path_str();

        // ── HARDENED CALIBRATION COMMIT WITH VALIDATION & ROLLBACK ──
        if path == CALIB_FILE_PATH {
            if total_len != 512 || !is_valid_calibration(staging) {
                return send_status(hw, header, WireStatus::SafetyDenied);
            }

            let mcu_uid = bsp::uid::read_mcu_uid();
            let calib = unsafe { &mut *core::ptr::addr_of_mut!(crate::CALIBRATION) };

            if calib
                .commit_with_backup(hw.filesystem, staging, &mcu_uid)
                .is_err()
            {
                return send_status(hw, header, WireStatus::IoError);
            }

            // Dynamically reload calibrated parameters into live silicon
            let pwr = unsafe { &mut *core::ptr::addr_of_mut!(crate::POWER_SERVICE) };
            pwr.battery_driver.calib_const = calib.battery_calib_const();
            hw.radio.set_xtal_freq_trim(calib.xtal_freq_low());
        } else {
            let res = hw.filesystem.write_file(path, &staging[..total_len]);
            if res.is_err() {
                return send_status(hw, header, WireStatus::IoError);
            }
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

    let transfer = unsafe { &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER) };
    if transfer.active && transfer.transfer_id == req.transfer_id {
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

            let mut buf = [0u8; 256];
            let rlen = hw.filesystem.read_file(req.path, &mut buf).unwrap_or(0);
            let crc = crypto::crc::CrcEngine::crc32(&buf[..rlen]);
            reply[6..10].copy_from_slice(&crc.to_le_bytes());
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
