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
use utils::bytes::{ByteReader, ByteWriter};
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

#[inline(never)]
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

struct WireCtx<'a, 'b> {
    hw: &'a mut SystemHardware<'b>,
    w: ByteWriter<'a>,
    now: u32,
}

impl WireCtx<'_, '_> {
    #[inline(always)]
    fn write_u16(&mut self, v: u16) {
        self.w.write_u16_le(v);
    }

    #[inline(always)]
    fn write_u32(&mut self, v: u32) {
        self.w.write_u32_le(v);
    }
}

/// Main entry point for non-core RigWire records routed from the transport driver.
#[inline(never)]
pub fn dispatch_wire_record(header: &WireHeader, hw: &mut SystemHardware, now: u32) -> bool {
    let raw = hw.wire.current_payload(header);
    let plen = raw.len().min(128);
    // `handle_*_service` only reads `hw.wire.session` and never touches `rx_buf` until `send_response`.
    let payload = unsafe { core::slice::from_raw_parts(raw.as_ptr(), plen) };

    let mut out_uninit = core::mem::MaybeUninit::<[u8; 128]>::uninit();
    let out = unsafe { &mut *out_uninit.as_mut_ptr() };

    let (status, resp_len) = if header.service == SERVICE_DATA && header.opcode == OP_TRANSFER_CHUNK
    {
        on_transfer_chunk(payload, hw, out)
    } else {
        let mut ctx = WireCtx {
            hw,
            w: ByteWriter::new(&mut out[..]),
            now,
        };
        let res = match header.service {
            SERVICE_CONTROL => handle_control_service(header.opcode, payload, &mut ctx),
            SERVICE_DATA => handle_data_service(header.opcode, payload, &mut ctx),
            SERVICE_EVENTS => handle_events_service(header.opcode, payload),
            _ => return false,
        };
        let Some(status) = res else {
            return false;
        };
        (status, ctx.w.position())
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
    val.get(..4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(WireStatus::BadLength)
}

#[inline(always)]
fn get_u16_le(val: &[u8]) -> Result<u16, WireStatus> {
    val.get(..2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(WireStatus::BadLength)
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

fn config_domain_idx(instance: u8) -> Result<usize, WireStatus> {
    let idx = instance as usize;
    if idx > 5 {
        Err(WireStatus::BadRange)
    } else {
        Ok(idx)
    }
}

fn read_sys_facet(facet: u16, ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    match facet {
        FACET_SYS_IDENTITY => {
            let serial = bsp::uid::get_hardware_serial64();
            ctx.w.write_bytes(&serial.to_le_bytes());
            ctx.w.write_bytes(&bsp::uid::read_mac_address());
            ctx.w
                .write_bytes(&bsp::uid::encode_crockford64(serial)[..14]);
            let is_auth = ctx.hw.wire.session.is_authenticated;
            ctx.w.write_u8(is_auth as u8);
            if is_auth {
                ctx.w.write_bytes(&bsp::uid::read_mcu_uid());
            } else {
                ctx.w.write_zeroes(12);
            }
        }
        FACET_SYS_METRICS => {
            ctx.write_u32(ctx.now);
            ctx.write_u32(py32_hal::rcc::sysclk_hz());
            let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
            ctx.write_u16(pwr.battery_voltage_mv());
            ctx.w.write_bytes(&[
                pwr.battery_percentage(),
                ctx.hw.orchestrator.active_app_id() as u8,
                ctx.hw.orchestrator.is_keypad_locked() as u8,
                bsp::pins::FLASHLIGHT.is_high() as u8,
            ]);
        }
        FACET_SYS_CALIBRATION => {
            let calib = unsafe { &*core::ptr::addr_of!(crate::CALIBRATION) };
            ctx.w.write_u8(calib.is_valid() as u8);
            ctx.write_u16(calib.xtal_freq_low() as u16);
            ctx.write_u16(calib.battery_calib_const());
        }
        _ => return Err(WireStatus::UnsupportedField),
    }
    Ok(())
}

fn read_radio_facet(facet: u16, instance: u8, ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let (slot, ch) = active_radio_slot(instance);
    let coord = &crate::radio_state_ref().coord;
    match facet {
        FACET_RADIO_TUNING => {
            ctx.write_u32(ch.effective_rx_freq().raw());
            ctx.write_u32(ch.step.in_hz());
            ctx.w.write_u8(coord.active_vfo as u8);
        }
        FACET_RADIO_MODULATION => {
            ctx.w.write_u8(ch.modulation.to_u8());
            ctx.write_u32(ch.bandwidth.in_hz());
        }
        FACET_RADIO_SQUELCH => {
            let rssi = ctx.hw.radio.get_rssi();
            let open = ctx.hw.radio.is_squelch_open() as u8;
            ctx.w.write_u8(ch.squelch_level);
            ctx.write_u16(rssi);
            ctx.w.write_u8(open);
        }
        FACET_RADIO_TRANSMIT => {
            ctx.w
                .write_bytes(&[coord.is_slot_tx_active(slot) as u8, ch.power as u8]);
        }
        FACET_RADIO_DIGITAL => {
            ctx.w.write_zeroes(2);
        }
        _ => return Err(WireStatus::UnsupportedField),
    }
    Ok(())
}

#[inline(never)]
fn on_handle_get(handle: ResourceHandle, ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    if handle.domain > DOMAIN_STORAGE {
        return Err(WireStatus::Unsupported);
    }
    match (handle.domain, handle.facet) {
        (DOMAIN_SYSTEM, facet) => return read_sys_facet(facet, ctx),
        (DOMAIN_RADIO, facet) => return read_radio_facet(facet, handle.instance, ctx),
        (DOMAIN_UI, FACET_UI_SURFACE) => {
            const UI_SURFACE_INFO: [u8; 7] = [128, 0, 64, 0, 1, 0, 4];
            ctx.w.write_bytes(&UI_SURFACE_INFO);
        }
        (DOMAIN_UI, FACET_UI_MODAL) => {
            let modal =
                unsafe { &*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL) };
            ctx.w.write_bytes(&[modal.is_active() as u8, modal.tag]);
        }
        (DOMAIN_UI, FACET_UI_VIEW) | (DOMAIN_APP, FACET_APP_LIFECYCLE) => {
            ctx.w.write_u8(ctx.hw.orchestrator.active_app_id() as u8);
        }
        (DOMAIN_INPUT, FACET_INPUT_LOCK) => {
            ctx.w.write_u8(ctx.hw.orchestrator.is_keypad_locked() as u8);
        }
        (DOMAIN_APP, FACET_APP_REGISTRY) => {
            ctx.w.write_u8(crate::AppRegistry::app_count() as u8);
        }
        (DOMAIN_CONFIG, _) => {
            let domain_idx = config_domain_idx(handle.instance)?;
            ctx.w
                .write_bytes(crate::master_config_ref().config_bytes(domain_idx));
        }
        (DOMAIN_STORAGE, _) => {
            let stats = ctx.hw.filesystem.stats();
            for v in [
                stats.active_files,
                stats.used_sectors,
                stats.free_sectors,
                stats.deleted_sectors,
            ] {
                ctx.write_u16(v);
            }
            ctx.write_u32(stats.wear_cycles);
            ctx.write_u16(stats.total_sectors);
        }
        _ => return Err(WireStatus::UnsupportedField),
    }
    Ok(())
}

#[inline(never)]
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
    if (handle.domain == DOMAIN_SYSTEM && handle.facet == FACET_SYS_METRICS)
        || (handle.domain == DOMAIN_INPUT && handle.facet == FACET_INPUT_LOCK)
    {
        if handle.domain == DOMAIN_INPUT || !val.is_empty() {
            set_keypad_lock(hw, get_u8(val)? != 0);
        }
        return Ok(());
    }
    match handle.domain {
        DOMAIN_RADIO => {
            let (_slot, ch) = active_radio_slot(handle.instance);
            match handle.facet {
                FACET_RADIO_TUNING => {
                    let freq_hz = get_u32_le(val)?;
                    if freq_hz < 18_000_000 || freq_hz > 1_300_000_000 {
                        return Err(WireStatus::BadRange);
                    }
                    let freq = Frequency::new(freq_hz);
                    ch.rx_freq = freq;
                    let _ = hw.radio.set_frequency(freq);
                }
                FACET_RADIO_MODULATION => {
                    let mode = ModulationMode::from_u8(get_u8(val)?);
                    ch.modulation = mode;
                    hw.radio.set_modulation(mode);
                }
                FACET_RADIO_SQUELCH => {
                    let sql = get_u8(val)?.min(9);
                    ch.squelch_level = sql;
                    hw.radio.set_squelch_level(sql);
                }
                FACET_RADIO_TRANSMIT => {
                    let should_tx = get_u8(val)? != 0;
                    let coord = &mut crate::radio_state().coord;
                    if should_tx {
                        coord.start_tx(&mut hw.radio);
                    } else {
                        coord.stop_tx(&mut hw.radio);
                    }
                }
                FACET_RADIO_DIGITAL => {
                    let offset_dhz = get_u16_le(val)? as i16;
                    hw.radio.set_sub_hz_offset(ch.rx_freq.raw(), offset_dhz);
                }
                _ => return Err(WireStatus::UnsupportedField),
            }
            Ok(())
        }
        DOMAIN_INPUT if handle.facet == FACET_INPUT_KEY => {
            let Some(code) = KeyCode::from_u8(get_u8(val)?) else {
                return Err(WireStatus::BadRange);
            };
            let state = [KeyState::Pressed, KeyState::Held, KeyState::Released]
                [val.get(1).copied().unwrap_or(0).min(2) as usize];
            crate::system::enqueue_event(Event::Key { code, state });
            Ok(())
        }
        DOMAIN_CONFIG => {
            let domain_idx = config_domain_idx(handle.instance)?;
            let mcfg = crate::master_config();
            let dest = mcfg.config_bytes_mut(domain_idx);
            let copy_len = val.len().min(dest.len());
            dest[..copy_len].copy_from_slice(&val[..copy_len]);
            mcfg.mark_domain_dirty(domain_idx);
            let _ = mcfg.flush_dirty(hw.filesystem);
            Ok(())
        }
        DOMAIN_SYSTEM | DOMAIN_INPUT => Err(WireStatus::UnsupportedField),
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
            let mode = [
                kernel::power::RebootMode::Firmware,
                kernel::power::RebootMode::Bootloader,
                kernel::power::RebootMode::SafeRecovery,
            ][params.first().copied().unwrap_or(0).min(2) as usize];
            kernel::power::PowerControl::reboot_with(mode);
        }
        (DOMAIN_SYSTEM, ACTION_SYS_FLASHLIGHT) => {
            let high = match params.first().copied().unwrap_or(2) {
                0 => false,
                1 => true,
                _ => !bsp::pins::FLASHLIGHT.is_high(),
            };
            if high {
                bsp::pins::FLASHLIGHT.set_high();
            } else {
                bsp::pins::FLASHLIGHT.set_low();
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
            let (code, state) = if handle.domain == DOMAIN_UI && action_id == ACTION_MODAL_CONFIRM {
                (KeyCode::Menu, KeyState::Pressed)
            } else if handle.domain == DOMAIN_UI && action_id == ACTION_MODAL_CANCEL {
                (KeyCode::Exit, KeyState::Pressed)
            } else {
                (KeyCode::Exit, KeyState::Held)
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
    ctx: &mut WireCtx<'_, '_>,
) -> Option<WireStatus> {
    if opcode < OP_HANDLE_GET || opcode > OP_HANDLE_STREAM {
        return None;
    }
    if payload.len() < 4 {
        return Some(WireStatus::BadLength);
    }
    let handle = ResourceHandle {
        domain: payload[0],
        instance: payload[1],
        facet: u16::from_le_bytes([payload[2], payload[3]]),
    };
    let rest = &payload[4..];
    ctx.w.write_bytes(&payload[..4]);

    let res = match opcode {
        OP_HANDLE_GET => on_handle_get(handle, ctx),
        OP_HANDLE_SET => {
            return Some(
                handle_resource_set(handle, rest, ctx.hw)
                    .err()
                    .unwrap_or(WireStatus::Ok),
            );
        }
        OP_HANDLE_INVOKE => {
            if rest.len() < 2 {
                Err(WireStatus::BadLength)
            } else {
                ctx.w.write_bytes(&rest[..2]);
                let action_id = u16::from_le_bytes([rest[0], rest[1]]);
                return Some(handle_resource_invoke(
                    handle,
                    action_id,
                    &rest[2..],
                    ctx.hw,
                ));
            }
        }
        OP_HANDLE_STREAM => {
            let mut r = ByteReader::new(rest);
            if let (Some(offset), Some(length)) = (r.read_u32_le(), r.read_u16_le()) {
                if handle.domain == DOMAIN_UI && handle.facet == FACET_UI_SURFACE {
                    let flat = crate::system_canvas().buffer_flat();
                    let off = (offset as usize).min(flat.len());
                    let len = (length as usize).min(100).min(flat.len() - off);
                    ctx.write_u32(off as u32);
                    ctx.write_u16(len as u16);
                    ctx.w.write_bytes(&flat[off..off + len]);
                    Ok(())
                } else {
                    Err(WireStatus::Unsupported)
                }
            } else {
                Err(WireStatus::BadLength)
            }
        }
        _ => return None,
    };
    match res {
        Ok(()) => Some(WireStatus::Ok),
        Err(status) => {
            ctx.w.reset();
            Some(status)
        }
    }
}

// ── SERVICE_DATA (0x03): Hardened KernFS Streaming ─────────────────────────

#[inline(never)]
fn validate_file_path(path: &str, is_write: bool, allow_calib: bool) -> Result<(), WireStatus> {
    let b = path.as_bytes();
    if b.len() > 32 || !utils::path::is_clean_path(b) {
        return Err(WireStatus::BadRange);
    }
    let root = utils::path::root_bit(b);
    if root == 0
        || (is_write && root == utils::path::ROOT_SYS && (!allow_calib || path != CALIB_FILE_PATH))
    {
        return Err(WireStatus::SafetyDenied);
    }
    Ok(())
}

fn invalidate_apps_if_needed(path: &str) {
    if path.starts_with("/apps/") {
        crate::apps::launcher::catalog::get_catalog().mark_dirty();
    }
}

#[inline(always)]
fn trim_trailing_slash(path: &str) -> &str {
    let b = path.as_bytes();
    if b.len() > 1 && b[b.len() - 1] == b'/' {
        &path[..b.len() - 1]
    } else {
        path
    }
}

fn stat_or_root(
    fs: &mut crate::services::SystemStorage,
    path: &str,
) -> Result<(usize, bool), crate::services::StorageError> {
    let path = trim_trailing_slash(path);
    if path == "/" {
        Ok((0, true))
    } else {
        fs.stat_info(path)
    }
}

fn on_transfer_open(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let req = TransferOpenRequest::parse(payload).ok_or(WireStatus::BadLength)?;
    if req.mode > TRANSFER_MODE_WRITE {
        return Err(WireStatus::BadRange);
    }
    let is_write = req.mode == TRANSFER_MODE_WRITE;
    validate_file_path(req.path, is_write, true)?;
    let stat = ctx.hw.filesystem.stat_info(req.path);
    let existing_size = match stat {
        Ok((sz, is_dir)) => {
            if is_dir {
                return Err(WireStatus::BadRange);
            }
            sz as u32
        }
        Err(_) if !is_write => return Err(WireStatus::NotFound),
        Err(_) => 0,
    };
    if is_write {
        if req.total_size as usize > MAX_WIRE_FILE_SIZE {
            return Err(WireStatus::BadLength);
        }
        if ctx.hw.filesystem.stats().free_sectors < 4 {
            return Err(WireStatus::Failed);
        }
    }

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

    ctx.write_u16(tid);
    ctx.write_u16(chunk_size);
    ctx.write_u32(existing_size);
    Ok(())
}

#[inline(never)]
fn on_transfer_chunk(
    payload: &[u8],
    hw: &mut SystemHardware,
    out: &mut [u8; 128],
) -> (WireStatus, usize) {
    let Some(req) = TransferChunkRequest::parse(payload) else {
        return (WireStatus::BadLength, 0);
    };
    let Some(transfer) = active_transfer_mut(req.transfer_id) else {
        return (WireStatus::BadSession, 0);
    };

    let off = req.offset as usize;
    let mut w = ByteWriter::new(&mut out[..]);
    w.write_u16_le(req.transfer_id);
    if transfer.mode == TRANSFER_MODE_WRITE {
        let chunk_len = req.data.len();
        let Some(end) = off.checked_add(chunk_len) else {
            return (WireStatus::BadRange, 0);
        };
        if end > MAX_WIRE_FILE_SIZE {
            return (WireStatus::BadLength, 0);
        }
        wire_file_staging_buf()[off..end].copy_from_slice(req.data);
        let new_len = end as u32;
        transfer.current_len = transfer.current_len.max(new_len);
        w.write_u16_le(chunk_len as u16);
        w.write_u32_le(new_len);
        (WireStatus::Ok, 8)
    } else {
        let len = (req.length as usize).min(120);
        let read_sz = hw
            .filesystem
            .read_file_at(transfer.path_str(), off, &mut out[8..8 + len])
            .unwrap_or(0);
        let mut w = ByteWriter::new(&mut out[2..8]);
        w.write_u32_le(off as u32);
        w.write_u16_le(read_sz as u16);
        (WireStatus::Ok, 8 + read_sz)
    }
}

#[inline(never)]
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
    crate::apply_active_calibration(&mut hw.radio);
    hw.radio.set_xtal_freq_trim(calib.xtal_freq_low());
    Ok(())
}

fn on_transfer_close(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
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
            commit_calibration_payload(staging, total_len, ctx.hw)?;
        } else if ctx
            .hw
            .filesystem
            .write_file(path, &staging[..total_len])
            .is_err()
        {
            return Err(WireStatus::IoError);
        }
        invalidate_apps_if_needed(path);
    }

    ctx.write_u16(req.transfer_id);
    ctx.write_u32(actual_crc);
    Ok(())
}

#[inline(never)]
fn on_transfer_stat(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let req = TransferStatRequest::parse(payload).ok_or(WireStatus::BadLength)?;

    let Ok((sz, is_dir)) = stat_or_root(ctx.hw.filesystem, req.path) else {
        ctx.w.write_zeroes(10);
        return Ok(());
    };

    let sz = if is_dir { 0 } else { sz };
    let mut crc_val = 0xFFFF_FFFFu32;
    let mut cur_off = 0usize;
    let mut uninit = core::mem::MaybeUninit::<[u8; 32]>::uninit();
    let chunk_buf = unsafe { &mut *uninit.as_mut_ptr() };
    while cur_off < sz {
        let to_read = (sz - cur_off).min(chunk_buf.len());
        match ctx
            .hw
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
    ctx.write_u16(1 | (u16::from(is_dir) << 8));
    ctx.write_u32(sz as u32);
    ctx.write_u32(!crc_val);
    Ok(())
}

#[inline(never)]
fn on_transfer_list(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let req = TransferListRequest::parse(payload).ok_or(WireStatus::BadLength)?;
    let path = trim_trailing_slash(req.path);

    let active_upload = unsafe {
        let t = &*core::ptr::addr_of!(CURRENT_TRANSFER);
        t.active && t.mode == TRANSFER_MODE_WRITE
    };
    if active_upload {
        return Err(WireStatus::Busy);
    }

    match stat_or_root(ctx.hw.filesystem, path) {
        Ok((_, true)) => {}
        Ok((_, false)) => return Err(WireStatus::BadRange),
        Err(_) => return Err(WireStatus::NotFound),
    }

    const MAX_LIST_ENTRIES: usize = (crate::data::arena::TRANSIENT_ARENA_BYTES
        - crate::data::arena::TRANSIENT_SCRATCH_OFFSET)
        / core::mem::size_of::<DirEntryInfo>();
    const {
        assert!(MAX_LIST_ENTRIES >= 16);
        assert!(
            core::mem::align_of::<DirEntryInfo>()
                <= core::mem::align_of::<crate::data::arena::AlignedArena>()
        );
    }
    let entries = unsafe {
        let ptr = crate::data::arena::scratch_tail_bytes_mut().as_mut_ptr() as *mut DirEntryInfo;
        core::ptr::write_bytes(ptr, 0, MAX_LIST_ENTRIES);
        core::slice::from_raw_parts_mut(ptr, MAX_LIST_ENTRIES)
    };
    let count = ctx
        .hw
        .filesystem
        .list_dir(path, entries)
        .unwrap_or(0)
        .min(MAX_LIST_ENTRIES);

    let start = req.cursor as usize;
    let mut next_cursor = 0xFFFFu16;
    let mut written = 0u8;
    ctx.w.write_zeroes(3);

    for (idx, entry) in entries[..count].iter().enumerate().skip(start) {
        let b = entry.path.as_str().as_bytes();
        let name_len = b.len().min(24);
        if ctx.w.remaining() < 6 + name_len {
            next_cursor = idx as u16;
            break;
        }
        ctx.w.write_u8(entry.is_dir as u8);
        ctx.write_u32(entry.size as u32);
        ctx.w.write_u8(name_len as u8);
        ctx.w.write_bytes(&b[..name_len]);
        written += 1;
    }

    let hdr = ctx.w.as_mut_slice();
    let nc = next_cursor.to_le_bytes();
    hdr[0] = nc[0];
    hdr[1] = nc[1];
    hdr[2] = written;
    Ok(())
}

#[inline(never)]
fn handle_data_service(
    opcode: u8,
    payload: &[u8],
    ctx: &mut WireCtx<'_, '_>,
) -> Option<WireStatus> {
    let res = match opcode {
        OP_TRANSFER_OPEN => on_transfer_open(payload, ctx),
        OP_TRANSFER_CLOSE => on_transfer_close(payload, ctx),
        OP_TRANSFER_ABORT => match TransferAbortRequest::parse(payload) {
            Some(req) => {
                if let Some(transfer) = active_transfer_mut(req.transfer_id) {
                    transfer.active = false;
                }
                ctx.write_u16(req.transfer_id);
                Ok(())
            }
            None => Err(WireStatus::BadLength),
        },
        OP_TRANSFER_STAT => on_transfer_stat(payload, ctx),
        OP_TRANSFER_LIST => on_transfer_list(payload, ctx),
        OP_TRANSFER_REMOVE => match TransferRemoveRequest::parse(payload) {
            Some(req) => validate_file_path(req.path, true, false).and_then(|()| {
                match ctx.hw.filesystem.stat_info(req.path) {
                    Ok((_, true)) => return Err(WireStatus::BadRange),
                    Ok((_, false)) => {}
                    Err(_) => return Err(WireStatus::NotFound),
                }
                ctx.hw
                    .filesystem
                    .remove(req.path)
                    .map_err(|_| WireStatus::NotFound)?;
                invalidate_apps_if_needed(req.path);
                Ok(())
            }),
            None => Err(WireStatus::BadLength),
        },
        _ => return None,
    };
    Some(res.err().unwrap_or(WireStatus::Ok))
}

// ── SERVICE_EVENTS (0x02) ────────────────────────────────────────────────────
#[inline(never)]
fn handle_events_service(opcode: u8, payload: &[u8]) -> Option<WireStatus> {
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
    Some(status)
}

/// Asynchronously emit an event frame over RigWire if the host holds a valid subscription with credits.
pub fn emit_event(hw: &mut SystemHardware, event_id: u16, data: &[u8]) {
    unsafe {
        let sub = &mut *core::ptr::addr_of_mut!(ACTIVE_SUBSCRIPTION);
        let bit = 1u32 << (event_id & 0x1F);
        if (sub.active_mask & bit) != 0 && sub.credits > 0 {
            sub.credits -= 1;
            let mut payload_uninit = core::mem::MaybeUninit::<[u8; 32]>::uninit();
            let payload = &mut *payload_uninit.as_mut_ptr();
            let mut w = ByteWriter::new(payload);
            w.write_u16_le(event_id);
            let d_len = data.len().min(30);
            w.write_bytes(&data[..d_len]);
            let _ = hw
                .wire
                .send_event(SERVICE_EVENTS, OP_EVENTS_EMIT, 0, w.as_slice());
        }
    }
}
