//! Declarative Firmware RigWire Service & Component Dispatcher.
//!
//! Bridges RigWire serial transport with microkernel traits, firmware services,
//! hardware peripherals, and hosted applications using universal Resource Handles.

use crate::hardware::SystemHardware;
use crate::services::calibration::{CALIB_FILE_PATH, is_valid_calibration};
use crate::services::storage::DirEntryInfo;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::service::{ModulationMode, PowerService, RfFineTuning, RfModulation, RfSquelch};
use kernel::wire::*;
use utils::bytes::ByteWriter;
use utils::units::Frequency;

/// Maximum upload size accepted over RigWire (aligned to transient arena).
pub const MAX_WIRE_FILE_SIZE: usize = crate::data::arena::TRANSIENT_ARENA_BYTES;

/// Reuses the shared transient arena so it incurs zero static SRAM overhead.
#[inline(always)]
pub fn wire_file_staging_buf() -> &'static mut [u8; MAX_WIRE_FILE_SIZE] {
    crate::data::arena::transient_arena_bytes_mut()
}

/// Active transfer session state.
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
        utils::format::ascii_str(&self.path[..self.path_len as usize]).unwrap_or("")
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

/// Event streaming subscription state (§8.9).
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

#[inline(always)]
fn ok_bytes(len: usize) -> (WireStatus, usize) {
    (WireStatus::Ok, len)
}

#[inline(always)]
fn err_empty(status: WireStatus) -> (WireStatus, usize) {
    (status, 0)
}

/// Main entry point for non-core RigWire records routed from the transport driver.
#[inline(never)]
pub fn dispatch_wire_record(header: &WireHeader, hw: &mut SystemHardware, now: u32) -> bool {
    let mut payload_buf = [0u8; 128];
    let plen = (header.payload_len as usize).min(128);
    payload_buf[..plen].copy_from_slice(hw.wire.current_payload(header));
    let payload = &payload_buf[..plen];

    let mut out = [0u8; 192];

    let res = match header.service {
        SERVICE_CONTROL => handle_control_service(header.opcode, payload, hw, now, &mut out),
        SERVICE_DATA => handle_data_service(header.opcode, payload, hw, &mut out),
        SERVICE_EVENTS => handle_events_service(header.opcode, payload, &mut out),
        _ => return false,
    };

    let Some((status, resp_len)) = res else {
        return false;
    };
    let _ = hw.wire.send_response(header, status, &out[..resp_len]);
    true
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
fn read_sys_facet(
    facet: u16,
    hw: &SystemHardware,
    now: u32,
    w: &mut ByteWriter<'_>,
) -> Result<(), WireStatus> {
    match facet {
        FACET_SYS_IDENTITY => {
            w.write_u64_le(bsp::uid::get_hardware_serial64());
            w.write_bytes(&bsp::uid::read_mac_address());
            w.write_bytes(&bsp::uid::get_crockford_serial()[..14]);
            let is_auth = hw.wire.session.is_authenticated;
            w.write_u8(is_auth as u8);
            if is_auth {
                w.write_bytes(&bsp::uid::read_mcu_uid());
            } else {
                w.write_zeroes(12);
            }
        }
        FACET_SYS_METRICS => {
            w.write_u32_le(now);
            w.write_u32_le(py32_hal::rcc::sysclk_hz());
            let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
            w.write_u16_le(pwr.battery_voltage_mv());
            w.write_u8(pwr.battery_percentage());
            w.write_u8(hw.orchestrator.active_app_id() as u8);
            w.write_u8(hw.orchestrator.is_keypad_locked() as u8);
            w.write_u8(bsp::pins::FLASHLIGHT.is_high() as u8);
        }
        FACET_SYS_CALIBRATION => {
            let calib = unsafe { &*core::ptr::addr_of!(crate::CALIBRATION) };
            w.write_u8(is_valid_calibration(calib.raw_bytes()) as u8);
            w.write_i16_le(calib.xtal_freq_low());
            w.write_u16_le(calib.battery_calib_const());
        }
        _ => return Err(WireStatus::UnsupportedField),
    }
    Ok(())
}

#[inline(never)]
fn read_radio_facet(
    facet: u16,
    instance: u8,
    hw: &mut SystemHardware,
    w: &mut ByteWriter<'_>,
) -> Result<(), WireStatus> {
    let (slot, ch) = active_radio_slot(instance);
    let coord = &crate::radio_state_ref().coord;
    match facet {
        FACET_RADIO_TUNING => {
            w.write_u32_le(ch.effective_rx_freq().raw());
            w.write_u32_le(ch.step.in_hz());
            w.write_u8(coord.active_vfo as u8);
        }
        FACET_RADIO_MODULATION => {
            w.write_u8(ch.modulation.to_u8());
            w.write_u32_le(ch.bandwidth.in_hz());
        }
        FACET_RADIO_SQUELCH => {
            w.write_u8(ch.squelch_level);
            w.write_u16_le(hw.radio.get_rssi());
            w.write_u8(hw.radio.is_squelch_open() as u8);
        }
        FACET_RADIO_TRANSMIT => {
            w.write_u8(coord.is_slot_tx_active(slot) as u8);
            w.write_u8(ch.power as u8);
        }
        FACET_RADIO_DIGITAL => {
            w.write_i16_le(0);
        }
        _ => return Err(WireStatus::UnsupportedField),
    }
    Ok(())
}

#[inline(never)]
fn on_handle_get(
    handle: ResourceHandle,
    hw: &mut SystemHardware,
    now: u32,
    out: &mut [u8],
) -> Result<usize, WireStatus> {
    let mut w = ByteWriter::new(out);
    match handle.domain {
        DOMAIN_SYSTEM => read_sys_facet(handle.facet, hw, now, &mut w)?,
        DOMAIN_RADIO => read_radio_facet(handle.facet, handle.instance, hw, &mut w)?,
        DOMAIN_UI => match handle.facet {
            FACET_UI_SURFACE => {
                const UI_SURFACE_INFO: [u8; 7] = [128, 0, 64, 0, 1, 0, 4];
                w.write_bytes(&UI_SURFACE_INFO);
            }
            FACET_UI_MODAL => {
                let modal =
                    unsafe { &*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL) };
                w.write_u8(modal.is_active() as u8);
                w.write_u8(modal.tag);
            }
            FACET_UI_VIEW => {
                w.write_u8(hw.orchestrator.active_app_id() as u8);
            }
            _ => return Err(WireStatus::UnsupportedField),
        },
        DOMAIN_INPUT if handle.facet == FACET_INPUT_LOCK => {
            w.write_u8(hw.orchestrator.is_keypad_locked() as u8);
        }
        DOMAIN_INPUT => return Err(WireStatus::UnsupportedField),
        DOMAIN_CONFIG => {
            let domain_idx = handle.instance as usize;
            if domain_idx > 5 {
                return Err(WireStatus::BadRange);
            }
            w.write_bytes(crate::master_config_ref().config_bytes(domain_idx));
        }
        DOMAIN_APP => match handle.facet {
            FACET_APP_REGISTRY => {
                w.write_u8(crate::AppRegistry::app_count() as u8);
            }
            FACET_APP_LIFECYCLE => {
                w.write_u8(hw.orchestrator.active_app_id() as u8);
            }
            _ => return Err(WireStatus::UnsupportedField),
        },
        DOMAIN_STORAGE => {
            let stats = hw.filesystem.stats();
            w.write_u16_le(stats.active_files);
            w.write_u16_le(stats.used_sectors);
            w.write_u16_le(stats.free_sectors);
            w.write_u16_le(stats.deleted_sectors);
            w.write_u32_le(stats.wear_cycles);
            w.write_u16_le(stats.total_sectors);
        }
        _ => return Err(WireStatus::Unsupported),
    }
    Ok(w.position())
}

fn set_keypad_lock(hw: &mut SystemHardware, locked: bool) {
    if locked != hw.orchestrator.is_keypad_locked() {
        hw.orchestrator.toggle_keypad_lock();
    }
}

#[inline(never)]
fn handle_resource_set(
    handle: ResourceHandle,
    val: &[u8],
    hw: &mut SystemHardware,
) -> Result<(), WireStatus> {
    match handle.domain {
        DOMAIN_SYSTEM if handle.facet == FACET_SYS_METRICS => {
            if let Some(&lock) = val.first() {
                set_keypad_lock(hw, lock != 0);
            }
            Ok(())
        }
        DOMAIN_RADIO => {
            let (slot, ch) = active_radio_slot(handle.instance);
            match handle.facet {
                FACET_RADIO_TUNING => {
                    let freq_hz = get_u32_le(val)?;
                    if !(18_000_000..=1_300_000_000).contains(&freq_hz) {
                        return Err(WireStatus::BadRange);
                    }
                    ch.rx_freq = Frequency::new(freq_hz);
                    let _ = hw.radio.set_frequency(Frequency::new(freq_hz));
                    Ok(())
                }
                FACET_RADIO_MODULATION => {
                    let mode = ModulationMode::from_u8(get_u8(val)?);
                    ch.modulation = mode;
                    hw.radio.set_modulation(mode);
                    Ok(())
                }
                FACET_RADIO_SQUELCH => {
                    let sql = get_u8(val)?.min(9);
                    ch.squelch_level = sql;
                    hw.radio.set_squelch_level(sql);
                    Ok(())
                }
                FACET_RADIO_TRANSMIT => {
                    let should_tx = get_u8(val)? != 0;
                    let coord = &mut crate::radio_state().coord;
                    if should_tx {
                        coord.start_tx(&mut hw.radio);
                    } else {
                        coord.stop_tx(&mut hw.radio);
                    }
                    let _ = slot;
                    Ok(())
                }
                FACET_RADIO_DIGITAL => {
                    let offset_dhz = get_i16_le(val)?;
                    hw.radio.set_sub_hz_offset(ch.rx_freq.raw(), offset_dhz);
                    Ok(())
                }
                _ => Err(WireStatus::UnsupportedField),
            }
        }
        DOMAIN_INPUT => match handle.facet {
            FACET_INPUT_KEY => {
                let code_u8 = get_u8(val)?;
                let state_u8 = val.get(1).copied().unwrap_or(0);
                let Some(code) = KeyCode::from_u8(code_u8) else {
                    return Err(WireStatus::BadRange);
                };
                let state = match state_u8 {
                    0 => KeyState::Pressed,
                    1 => KeyState::Held,
                    _ => KeyState::Released,
                };
                crate::system::enqueue_event(Event::Key { code, state });
                Ok(())
            }
            FACET_INPUT_LOCK => {
                set_keypad_lock(hw, get_u8(val)? != 0);
                Ok(())
            }
            _ => Err(WireStatus::UnsupportedField),
        },
        DOMAIN_CONFIG => {
            let domain_idx = handle.instance as usize;
            if domain_idx > 5 {
                return Err(WireStatus::BadRange);
            }
            let mcfg = crate::master_config();
            let dest = mcfg.config_bytes_mut(domain_idx);
            let copy_len = val.len().min(dest.len());
            dest[..copy_len].copy_from_slice(&val[..copy_len]);
            mcfg.mark_domain_dirty(domain_idx);
            let _ = mcfg.flush_dirty(hw.filesystem);
            Ok(())
        }
        DOMAIN_SYSTEM => Err(WireStatus::UnsupportedField),
        _ => Err(WireStatus::Unsupported),
    }
}

#[inline(never)]
fn handle_resource_invoke(
    handle: ResourceHandle,
    action_id: u16,
    params: &[u8],
    hw: &mut SystemHardware,
) -> WireStatus {
    match (handle.domain, action_id) {
        (DOMAIN_SYSTEM, ACTION_SYS_REBOOT) => {
            let mode = match params.first().copied() {
                Some(1) => kernel::power::RebootMode::Bootloader,
                Some(2) => kernel::power::RebootMode::SafeRecovery,
                _ => kernel::power::RebootMode::Firmware,
            };
            kernel::power::PowerControl::reboot_with(mode);
        }
        (DOMAIN_SYSTEM, ACTION_SYS_FLASHLIGHT) => {
            match params.first().copied().unwrap_or(2) {
                0 => bsp::pins::FLASHLIGHT.set_low(),
                1 => bsp::pins::FLASHLIGHT.set_high(),
                _ if bsp::pins::FLASHLIGHT.is_high() => bsp::pins::FLASHLIGHT.set_low(),
                _ => bsp::pins::FLASHLIGHT.set_high(),
            }
            WireStatus::Ok
        }
        (DOMAIN_RADIO, ACTION_RADIO_VFO_TOGGLE) => {
            crate::radio_state().coord.toggle_vfo(&mut hw.radio);
            WireStatus::Ok
        }
        (DOMAIN_UI, ACTION_MODAL_DISMISS) => {
            unsafe {
                (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL)).dismiss();
            }
            WireStatus::Ok
        }
        (DOMAIN_UI, ACTION_MODAL_CONFIRM | ACTION_MODAL_CANCEL | ACTION_VIEW_LAUNCHER)
        | (DOMAIN_APP, ACTION_APP_KILL) => {
            let (code, state) = match action_id {
                ACTION_MODAL_CONFIRM if handle.domain == DOMAIN_UI => {
                    (KeyCode::Menu, KeyState::Pressed)
                }
                ACTION_MODAL_CANCEL if handle.domain == DOMAIN_UI => {
                    (KeyCode::Exit, KeyState::Pressed)
                }
                _ => (KeyCode::Exit, KeyState::Held),
            };
            crate::system::enqueue_event(Event::Key { code, state });
            WireStatus::Ok
        }
        (DOMAIN_INPUT, ACTION_INPUT_LOCK_TOGGLE) => {
            hw.orchestrator.toggle_keypad_lock();
            WireStatus::Ok
        }
        (DOMAIN_APP, ACTION_APP_LAUNCH) => {
            let app_id = params.first().copied().unwrap_or(0);
            if (app_id as usize) < crate::AppRegistry::app_count() {
                WireStatus::Ok
            } else {
                WireStatus::BadRange
            }
        }
        _ => WireStatus::Unsupported,
    }
}

#[inline(never)]
fn handle_control_service(
    opcode: u8,
    payload: &[u8],
    hw: &mut SystemHardware,
    now: u32,
    out: &mut [u8; 192],
) -> Option<(WireStatus, usize)> {
    if !matches!(
        opcode,
        OP_HANDLE_GET | OP_HANDLE_SET | OP_HANDLE_INVOKE | OP_HANDLE_STREAM
    ) {
        return None;
    }
    let Some(handle) = ResourceHandle::parse(payload) else {
        return Some(err_empty(WireStatus::BadLength));
    };
    handle.write_bytes(&mut out[..4]);

    let res = match opcode {
        OP_HANDLE_GET => match on_handle_get(handle, hw, now, &mut out[4..100]) {
            Ok(len) => ok_bytes(4 + len),
            Err(status) => err_empty(status),
        },
        OP_HANDLE_SET => {
            let status = handle_resource_set(handle, &payload[4..], hw)
                .err()
                .unwrap_or(WireStatus::Ok);
            (status, 4)
        }
        OP_HANDLE_INVOKE => {
            let Ok(req) = HandleInvokeReq::parse(payload) else {
                return Some(err_empty(WireStatus::BadLength));
            };
            out[4..6].copy_from_slice(&req.action_id.to_le_bytes());
            let status = handle_resource_invoke(handle, req.action_id, req.params, hw);
            (status, 6)
        }
        OP_HANDLE_STREAM => {
            let Ok(req) = HandleStreamReq::parse(payload) else {
                return Some(err_empty(WireStatus::BadLength));
            };
            if handle.domain != DOMAIN_UI || handle.facet != FACET_UI_SURFACE {
                return Some(err_empty(WireStatus::Unsupported));
            }
            let flat = crate::system_canvas().buffer_flat();
            let total_sz = flat.len();
            let off = (req.offset as usize).min(total_sz);
            let len = (req.length as usize).min(100).min(total_sz - off);
            let mut w = ByteWriter::new(&mut out[4..10 + len]);
            w.write_u32_le(off as u32);
            w.write_u16_le(len as u16);
            w.write_bytes(&flat[off..off + len]);
            ok_bytes(4 + w.position())
        }
        _ => return None,
    };
    Some(res)
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
    let is_sys = b.starts_with(b"/sys/");
    let allowed =
        is_sys || b.starts_with(b"/user/") || b.starts_with(b"/pref/") || b.starts_with(b"/apps/");
    if !allowed || (is_write && is_sys && path != CALIB_FILE_PATH) {
        return Err(WireStatus::SafetyDenied);
    }
    Ok(())
}

#[inline(never)]
fn on_transfer_open(
    payload: &[u8],
    hw: &mut SystemHardware,
    w: &mut ByteWriter<'_>,
) -> Result<(), WireStatus> {
    let req = TransferOpenRequest::parse(payload).ok_or(WireStatus::BadLength)?;
    validate_file_path(req.path, req.mode == TRANSFER_MODE_WRITE)?;
    if req.mode == TRANSFER_MODE_WRITE {
        if req.total_size as usize > MAX_WIRE_FILE_SIZE {
            return Err(WireStatus::BadLength);
        }
        if hw.filesystem.stats().free_sectors < 4 {
            return Err(WireStatus::Failed);
        }
    }

    let existing_size = hw.filesystem.file_size(req.path).unwrap_or(0) as u32;
    let tid = unsafe {
        let next_ptr = core::ptr::addr_of_mut!(NEXT_TRANSFER_ID);
        let id = *next_ptr;
        *next_ptr = (*next_ptr).wrapping_add(1).max(1);
        id
    };
    let chunk_size = req.chunk_size.clamp(16, 120);
    let path_bytes = req.path.as_bytes();
    unsafe {
        let t = &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER);
        t.active = true;
        t.transfer_id = tid;
        t.mode = req.mode;
        t.total_size = req.total_size;
        t.current_len = 0;
        t.chunk_size = chunk_size;
        t.path[..path_bytes.len()].copy_from_slice(path_bytes);
        t.path_len = path_bytes.len() as u8;
    }

    w.write_u16_le(tid);
    w.write_u16_le(chunk_size);
    w.write_u32_le(existing_size);
    Ok(())
}

#[inline(never)]
fn on_transfer_chunk(
    payload: &[u8],
    hw: &mut SystemHardware,
    out: &mut [u8; 192],
) -> (WireStatus, usize) {
    let Some(req) = TransferChunkRequest::parse(payload) else {
        return err_empty(WireStatus::BadLength);
    };
    let Some(transfer) = active_transfer_mut(req.transfer_id) else {
        return err_empty(WireStatus::BadSession);
    };

    if transfer.mode == TRANSFER_MODE_WRITE {
        let off = req.offset as usize;
        let chunk_len = req.data.len();
        let Some(end) = off.checked_add(chunk_len) else {
            return err_empty(WireStatus::BadRange);
        };
        if end > MAX_WIRE_FILE_SIZE {
            return err_empty(WireStatus::BadLength);
        }
        wire_file_staging_buf()[off..end].copy_from_slice(req.data);
        let new_len = end as u32;
        transfer.current_len = transfer.current_len.max(new_len);

        let mut w = ByteWriter::new(out);
        w.write_u16_le(req.transfer_id);
        w.write_u16_le(chunk_len as u16);
        w.write_u32_le(new_len);
        ok_bytes(w.position())
    } else {
        let path = transfer.path_str();
        let off = req.offset as usize;
        let len = (req.length as usize).min(120);
        let read_sz = hw
            .filesystem
            .read_file_at(path, off, &mut out[8..8 + len])
            .unwrap_or(0);
        let mut w = ByteWriter::new(out);
        w.write_u16_le(req.transfer_id);
        w.write_u32_le(off as u32);
        w.write_u16_le(read_sz as u16);
        ok_bytes(8 + read_sz)
    }
}

fn commit_calibration_payload(
    staging: &[u8; MAX_WIRE_FILE_SIZE],
    total_len: usize,
    hw: &mut SystemHardware,
) -> Result<(), WireStatus> {
    if total_len != 512 {
        return Err(WireStatus::SafetyDenied);
    }
    let calib_slice: &[u8; 512] = unsafe { &*(staging.as_ptr() as *const [u8; 512]) };
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

    let pwr = unsafe { &mut *core::ptr::addr_of_mut!(crate::POWER_SERVICE) };
    pwr.battery_driver
        .set_calib_const(calib.battery_calib_const());
    pwr.battery_driver
        .load_calibration_curve(calib.battery_curve());
    hw.radio.attach_calibration(calib.raw_bytes());
    hw.radio.set_xtal_freq_trim(calib.xtal_freq_low());
    Ok(())
}

#[inline(never)]
fn on_transfer_close(
    payload: &[u8],
    hw: &mut SystemHardware,
    w: &mut ByteWriter<'_>,
) -> Result<(), WireStatus> {
    let req = TransferCloseRequest::parse(payload).ok_or(WireStatus::BadLength)?;
    let transfer = active_transfer_mut(req.transfer_id).ok_or(WireStatus::BadSession)?;

    transfer.active = false;
    let mut actual_crc = 0u32;
    if transfer.mode == TRANSFER_MODE_WRITE {
        let staging = wire_file_staging_buf();
        let total_len = (transfer.current_len as usize).min(staging.len());
        actual_crc = crypto::crc::CrcEngine::crc32(&staging[..total_len]);
        if req.expected_crc32 != 0 && req.expected_crc32 != actual_crc {
            return Err(WireStatus::Failed);
        }
        let path = transfer.path_str();
        if path == CALIB_FILE_PATH {
            commit_calibration_payload(staging, total_len, hw)?;
        } else if hw
            .filesystem
            .write_file(path, &staging[..total_len])
            .is_err()
        {
            return Err(WireStatus::IoError);
        }
    }

    w.write_u16_le(req.transfer_id);
    w.write_u32_le(actual_crc);
    Ok(())
}

fn on_transfer_stat(
    payload: &[u8],
    hw: &mut SystemHardware,
    w: &mut ByteWriter<'_>,
) -> Result<(), WireStatus> {
    let req = TransferStatRequest::parse(payload).ok_or(WireStatus::BadLength)?;

    match hw.filesystem.file_size(req.path) {
        Ok(sz) => {
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
            w.write_u16_le(1);
            w.write_u32_le(sz as u32);
            w.write_u32_le(!crc_val);
            Ok(())
        }
        Err(_) => {
            w.write_zeroes(10);
            Err(WireStatus::NotFound)
        }
    }
}

#[inline(never)]
fn on_transfer_list(
    payload: &[u8],
    hw: &mut SystemHardware,
    w: &mut ByteWriter<'_>,
) -> Result<(), WireStatus> {
    let req = TransferListRequest::parse(payload).ok_or(WireStatus::BadLength)?;

    let active_upload = unsafe {
        let t = &*core::ptr::addr_of!(CURRENT_TRANSFER);
        t.active && t.mode == TRANSFER_MODE_WRITE
    };
    if active_upload {
        return Err(WireStatus::Busy);
    }

    let entries = unsafe {
        let ptr = crate::runtime::arena_base_ptr() as *mut DirEntryInfo;
        core::ptr::write_bytes(ptr, 0, 12);
        core::slice::from_raw_parts_mut(ptr, 12)
    };
    let count = hw.filesystem.list_dir(req.path, entries).unwrap_or(0);

    w.write_u16_le(0xFFFFu16);
    w.write_u8(count as u8);

    for entry in &entries[..count] {
        let b = entry.path.as_str().as_bytes();
        let name_len = b.len().min(24);
        if w.remaining() < 6 + name_len {
            break;
        }
        w.write_u8(entry.is_dir as u8);
        w.write_u32_le(entry.size as u32);
        w.write_u8(name_len as u8);
        w.write_bytes(&b[..name_len]);
    }
    Ok(())
}

#[inline(never)]
fn handle_data_service(
    opcode: u8,
    payload: &[u8],
    hw: &mut SystemHardware,
    out: &mut [u8; 192],
) -> Option<(WireStatus, usize)> {
    if opcode == OP_TRANSFER_CHUNK {
        return Some(on_transfer_chunk(payload, hw, out));
    }
    let mut w = ByteWriter::new(&mut out[..160]);
    let res = match opcode {
        OP_TRANSFER_OPEN => on_transfer_open(payload, hw, &mut w),
        OP_TRANSFER_CLOSE => on_transfer_close(payload, hw, &mut w),
        OP_TRANSFER_ABORT => match TransferAbortRequest::parse(payload) {
            Some(req) => {
                if let Some(transfer) = active_transfer_mut(req.transfer_id) {
                    transfer.active = false;
                }
                w.write_u16_le(req.transfer_id);
                Ok(())
            }
            None => Err(WireStatus::BadLength),
        },
        OP_TRANSFER_STAT => on_transfer_stat(payload, hw, &mut w),
        OP_TRANSFER_LIST => on_transfer_list(payload, hw, &mut w),
        OP_TRANSFER_REMOVE => match TransferRemoveRequest::parse(payload) {
            Some(req) => validate_file_path(req.path, true).and_then(|()| {
                if req.path == CALIB_FILE_PATH {
                    Err(WireStatus::SafetyDenied)
                } else {
                    hw.filesystem
                        .remove(req.path)
                        .map_err(|_| WireStatus::NotFound)
                }
            }),
            None => Err(WireStatus::BadLength),
        },
        _ => return None,
    };
    let status = res.err().unwrap_or(WireStatus::Ok);
    Some((status, w.position()))
}

// ── SERVICE_EVENTS (0x02) ────────────────────────────────────────────────────
#[inline(never)]
fn handle_events_service(
    opcode: u8,
    payload: &[u8],
    _out: &mut [u8; 192],
) -> Option<(WireStatus, usize)> {
    let sub = unsafe { &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION) };
    let status = match opcode {
        OP_EVENTS_SUBSCRIBE => match EventSubscribeReq::parse(payload) {
            Ok(r) => {
                sub.active_mask = r.event_mask;
                sub.credits = r.initial_credits;
                WireStatus::Ok
            }
            Err(status) => status,
        },
        OP_EVENTS_UNSUBSCRIBE => {
            sub.active_mask = 0;
            sub.credits = 0;
            WireStatus::Ok
        }
        OP_EVENTS_CREDIT => {
            if payload.len() >= 2 {
                let grant = u16::from_le_bytes([payload[0], payload[1]]);
                sub.credits = sub.credits.saturating_add(grant);
                WireStatus::Ok
            } else {
                WireStatus::BadLength
            }
        }
        _ => return None,
    };
    Some((status, 0))
}

/// Asynchronously emit an event frame over RigWire if the host holds a valid subscription with credits.
pub fn emit_event(hw: &mut SystemHardware, event_id: u16, data: &[u8]) {
    unsafe {
        let sub = &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION);
        let bit = 1u32 << (event_id & 0x1F);
        if (sub.active_mask & bit) != 0 && sub.credits > 0 {
            sub.credits -= 1;
            let mut payload = [0u8; 32];
            let mut w = ByteWriter::new(&mut payload);
            w.write_u16_le(event_id);
            let d_len = data.len().min(30);
            w.write_bytes(&data[..d_len]);
            let _ = hw
                .wire
                .send_event(SERVICE_EVENTS, OP_EVENTS_EMIT, 0, w.as_slice());
        }
    }
}
