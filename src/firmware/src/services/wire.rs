//! Declarative Firmware RigWire Service & Component Dispatcher.
//!
//! Bridges RigWire serial transport with microkernel traits, firmware services,
//! hardware peripherals, and hosted applications using universal Resource Handles.

use crate::hardware::SystemHardware;
use crate::services::calibration::{CALIB_FILE_PATH, is_valid_calibration};
use kernel::event::{Event, KeyCode, KeyState};
use kernel::service::{ModulationMode, PowerService, RfFineTuning, RfModulation, RfSquelch};
use kernel::wire::*;
use utils::units::Frequency;

/// Single-indirect KernFS page limit: 8 L1 index pages x 128 pages x 256 B = 256 KiB.
const MAX_WIRE_FILE_SIZE: usize = 8 * 128 * 256;

/// Active transfer session state. Coalesces writes in the 512 B `staging_buf`
/// so arbitrary-size files stream straight to KernFS with zero RAM limit.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ActiveTransfer {
    pub active: bool,
    pub mode: u8,
    pub path_len: u8,
    pub transfer_id: u16,
    pub chunk_size: u16,
    pub buf_len: u16,
    pub total_size: u32,
    pub current_len: u32,
    pub buf_offset: u32,
    pub path: [u8; 32],
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
            buf_offset: 0,
            buf_len: 0,
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
    out: &'a mut [u8; 128],
    pos: usize,
    now: u32,
}

impl WireCtx<'_, '_> {
    #[inline(never)]
    fn write_bytes(&mut self, data: &[u8]) {
        let end = (self.pos + data.len()).min(128);
        let n = end - self.pos;
        self.out[self.pos..end].copy_from_slice(&data[..n]);
        self.pos = end;
    }

    #[inline(never)]
    fn write_zeroes(&mut self, len: usize) {
        let end = (self.pos + len).min(128);
        self.out[self.pos..end].fill(0);
        self.pos = end;
    }

    #[inline(never)]
    fn write_le(&mut self, mut v: u32, n: usize) {
        let mut i = 0;
        while i < n && self.pos < 128 {
            self.out[self.pos] = v as u8;
            self.pos += 1;
            v >>= 8;
            i += 1;
        }
    }

    #[inline(always)]
    fn write_u8(&mut self, v: u8) {
        self.write_le(v as u32, 1);
    }

    #[inline(always)]
    fn write_u16(&mut self, v: u16) {
        self.write_le(v as u32, 2);
    }

    #[inline(always)]
    fn write_u32(&mut self, v: u32) {
        self.write_le(v, 4);
    }
}

/// Main entry point for non-core RigWire records routed from the transport driver.
#[inline(never)]
pub fn dispatch_wire_record(header: &WireHeader, hw: &mut SystemHardware, now: u32) -> bool {
    let raw = hw.wire.current_payload(header);
    let plen = raw.len().min(128);
    // `handle_*_service` only reads `hw.wire.session` and never touches `rx_buf` until `send_response`.
    let payload = unsafe { core::slice::from_raw_parts(raw.as_ptr(), plen) };

    let out: &mut [u8; 128] = unsafe {
        &mut *(crate::data::arena::scratch_tail_bytes_mut()[1900..].as_mut_ptr() as *mut [u8; 128])
    };

    let mut ctx = WireCtx {
        hw,
        out,
        pos: 0,
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
    let resp_len = if status == WireStatus::Ok { ctx.pos } else { 0 };
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
    if val.len() < 4 {
        Err(WireStatus::BadLength)
    } else {
        Ok(u32::from_le_bytes([val[0], val[1], val[2], val[3]]))
    }
}

#[inline(always)]
fn get_u16_le(val: &[u8]) -> Result<u16, WireStatus> {
    if val.len() < 2 {
        Err(WireStatus::BadLength)
    } else {
        Ok(u16::from_le_bytes([val[0], val[1]]))
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

fn config_domain_idx(instance: u8) -> Result<usize, WireStatus> {
    let idx = instance as usize;
    if idx > 5 {
        Err(WireStatus::BadRange)
    } else {
        Ok(idx)
    }
}

#[inline(never)]
fn read_sys_facet(facet: u16, ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    match facet {
        FACET_SYS_IDENTITY => {
            let serial = bsp::uid::get_hardware_serial64();
            ctx.write_bytes(&serial.to_le_bytes());
            ctx.write_bytes(&bsp::uid::mac_from_serial64(serial));
            ctx.write_bytes(&bsp::uid::encode_crockford64(serial)[..14]);
            let is_auth = ctx.hw.wire.session.is_authenticated;
            ctx.write_u8(is_auth as u8);
            if is_auth {
                ctx.write_bytes(&bsp::uid::read_mcu_uid());
            } else {
                ctx.write_zeroes(12);
            }
        }
        FACET_SYS_METRICS => {
            ctx.write_u32(ctx.now);
            ctx.write_u32(py32_hal::rcc::sysclk_hz());
            let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
            ctx.write_u16(pwr.battery_voltage_mv());
            ctx.write_bytes(&[
                pwr.battery_percentage(),
                ctx.hw.orchestrator.active_app_id() as u8,
                ctx.hw.orchestrator.is_keypad_locked() as u8,
                bsp::pins::FLASHLIGHT.is_high() as u8,
            ]);
        }
        FACET_SYS_CALIBRATION => {
            let calib = unsafe { &*core::ptr::addr_of!(crate::CALIBRATION) };
            ctx.write_u8(calib.is_valid() as u8);
            ctx.write_u16(calib.xtal_freq_low() as u16);
            ctx.write_u16(calib.battery_calib_const());
        }
        _ => return Err(WireStatus::UnsupportedField),
    }
    Ok(())
}

#[inline(never)]
fn read_radio_facet(facet: u16, instance: u8, ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let (slot, ch) = active_radio_slot(instance);
    let coord = &crate::radio_state_ref().coord;
    match facet {
        FACET_RADIO_TUNING => {
            ctx.write_u32(ch.effective_rx_freq().raw());
            ctx.write_u32(ch.step.in_hz());
            ctx.write_u8(coord.active_vfo as u8);
        }
        FACET_RADIO_MODULATION => {
            ctx.write_u8(ch.modulation.to_u8());
            ctx.write_u32(ch.bandwidth.in_hz());
        }
        FACET_RADIO_SQUELCH => {
            let rssi = ctx.hw.radio.get_rssi();
            let open = ctx.hw.radio.is_squelch_open() as u8;
            ctx.write_u8(ch.squelch_level);
            ctx.write_u16(rssi);
            ctx.write_u8(open);
        }
        FACET_RADIO_TRANSMIT => {
            ctx.write_bytes(&[coord.is_slot_tx_active(slot) as u8, ch.power as u8]);
        }
        FACET_RADIO_DIGITAL => {
            ctx.write_zeroes(2);
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
            ctx.write_bytes(&UI_SURFACE_INFO);
        }
        (DOMAIN_UI, FACET_UI_MODAL) => {
            let modal =
                unsafe { &*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL) };
            ctx.write_bytes(&[modal.is_active() as u8, modal.tag]);
        }
        (DOMAIN_UI, FACET_UI_VIEW) | (DOMAIN_APP, FACET_APP_LIFECYCLE) => {
            ctx.write_u8(ctx.hw.orchestrator.active_app_id() as u8);
        }
        (DOMAIN_INPUT, FACET_INPUT_LOCK) => {
            ctx.write_u8(ctx.hw.orchestrator.is_keypad_locked() as u8);
        }
        (DOMAIN_APP, FACET_APP_REGISTRY) => {
            ctx.write_u8(crate::AppRegistry::app_count() as u8);
        }
        (DOMAIN_CONFIG, _) => {
            let domain_idx = config_domain_idx(handle.instance)?;
            ctx.write_bytes(crate::master_config_ref().config_bytes(domain_idx));
        }
        (DOMAIN_STORAGE, _) => {
            let stats = ctx.hw.filesystem.stats();
            ctx.write_u16(stats.active_files);
            ctx.write_u16(stats.used_sectors);
            ctx.write_u16(stats.free_sectors);
            ctx.write_u16(stats.deleted_sectors);
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
            let arg = params.first().copied().unwrap_or(0);
            if arg == 3 {
                panic!("Wire test panic");
            }
            if arg == 4 {
                #[cfg(target_arch = "arm")]
                unsafe {
                    core::arch::asm!("udf #0x7F", options(noreturn));
                }
                #[cfg(not(target_arch = "arm"))]
                panic!("Wire test fault");
            }
            let mode = match arg {
                1 => kernel::power::RebootMode::Bootloader,
                2 => kernel::power::RebootMode::SafeRecovery,
                _ => kernel::power::RebootMode::Firmware,
            };
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
            let (code, state) = match (handle.domain, action_id) {
                (DOMAIN_UI, ACTION_MODAL_CONFIRM) => (KeyCode::Menu, KeyState::Pressed),
                (DOMAIN_UI, ACTION_MODAL_CANCEL) => (KeyCode::Exit, KeyState::Pressed),
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
    ctx.write_bytes(&payload[..4]);

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
                ctx.write_bytes(&rest[..2]);
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
            if rest.len() < 6 {
                Err(WireStatus::BadLength)
            } else if handle.domain == DOMAIN_UI && handle.facet == FACET_UI_SURFACE {
                let offset = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
                let length = u16::from_le_bytes([rest[4], rest[5]]);
                let flat = crate::system_canvas().buffer_flat();
                let off = (offset as usize).min(flat.len());
                let len = (length as usize).min(100).min(flat.len() - off);
                ctx.write_u32(off as u32);
                ctx.write_u16(len as u16);
                ctx.write_bytes(&flat[off..off + len]);
                Ok(())
            } else {
                Err(WireStatus::Unsupported)
            }
        }
        _ => return None,
    };
    match res {
        Ok(()) => Some(WireStatus::Ok),
        Err(status) => {
            ctx.pos = 0;
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

#[inline(never)]
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

#[inline(never)]
fn check_regular_file(
    fs: &mut crate::services::SystemStorage,
    path: &str,
    is_write: bool,
    allow_missing_or_calib: bool,
) -> Result<u32, WireStatus> {
    validate_file_path(path, is_write, allow_missing_or_calib)?;
    match fs.stat_info(path) {
        Ok((_, true)) => Err(WireStatus::BadRange),
        Ok((sz, false)) => Ok(sz as u32),
        Err(_) if allow_missing_or_calib => Ok(0),
        Err(_) => Err(WireStatus::NotFound),
    }
}

#[inline(never)]
fn flush_transfer_buf(
    transfer: &mut ActiveTransfer,
    fs: &mut crate::services::SystemStorage,
    flush_all: bool,
) -> Result<(), WireStatus> {
    let staging = crate::data::arena::staging_buf();
    let path = utils::format::ascii_str(&transfer.path[..transfer.path_len as usize]).unwrap_or("");
    loop {
        let len = transfer.buf_len as usize;
        let write_len = if len >= 256 {
            256
        } else if flush_all && len > 0 {
            len
        } else {
            return Ok(());
        };
        if fs
            .fs
            .write_at(path, transfer.buf_offset as usize, &staging[..write_len])
            .is_err()
        {
            return Err(WireStatus::IoError);
        }
        let rem = len - write_len;
        if rem > 0 {
            let (dst, src) = staging.split_at_mut(write_len);
            let n = rem.min(dst.len()).min(src.len());
            dst[..n].copy_from_slice(&src[..n]);
        }
        transfer.buf_offset += write_len as u32;
        transfer.buf_len = rem as u16;
    }
}

#[inline(never)]
fn stream_file_crc32(fs: &mut crate::services::SystemStorage, path: &str, size: usize) -> u32 {
    let mut crc_val = 0xFFFF_FFFFu32;
    let mut cur_off = 0usize;
    let buf = &mut crate::data::arena::staging_buf()[..256];
    while cur_off < size {
        let to_read = (size - cur_off).min(buf.len());
        match fs.read_file_at(path, cur_off, &mut buf[..to_read]) {
            Ok(n) if n > 0 => {
                crc_val = crypto::crc::CrcEngine::crc32_update(crc_val, &buf[..n]);
                cur_off += n;
            }
            _ => break,
        }
    }
    !crc_val
}

#[inline(never)]
fn on_transfer_open(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let req = TransferOpenRequest::parse(payload).ok_or(WireStatus::BadLength)?;
    if req.mode > TRANSFER_MODE_WRITE {
        return Err(WireStatus::BadRange);
    }
    let is_write = req.mode == TRANSFER_MODE_WRITE;
    let existing_size = check_regular_file(ctx.hw.filesystem, req.path, is_write, is_write)?;
    if is_write {
        let req_sz = req.total_size as usize;
        if req_sz > MAX_WIRE_FILE_SIZE {
            return Err(WireStatus::BadLength);
        }
        let is_calib = req.path == CALIB_FILE_PATH;
        if is_calib && req_sz != 512 {
            return Err(WireStatus::SafetyDenied);
        }
        let free_bytes = (ctx.hw.filesystem.stats().free_sectors as usize) * 15 * 256;
        if req_sz > free_bytes + existing_size as usize {
            return Err(WireStatus::Failed);
        }
        if !is_calib && ctx.hw.filesystem.write_file(req.path, &[]).is_err() {
            return Err(WireStatus::IoError);
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
        t.buf_offset = 0;
        t.buf_len = 0;
        t.path[..path_bytes.len()].copy_from_slice(path_bytes);
        t.path_len = path_bytes.len() as u8;
    }

    ctx.write_u16(tid);
    ctx.write_u16(chunk_size);
    ctx.write_u32(existing_size);
    Ok(())
}

#[inline(never)]
fn on_transfer_chunk(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let req = TransferChunkRequest::parse(payload).ok_or(WireStatus::BadLength)?;
    let transfer = active_transfer_mut(req.transfer_id).ok_or(WireStatus::BadSession)?;

    let off = req.offset as usize;
    ctx.write_u16(req.transfer_id);
    if transfer.mode == TRANSFER_MODE_WRITE {
        let chunk_len = req.data.len();
        let end = off.checked_add(chunk_len).ok_or(WireStatus::BadRange)?;
        if end > MAX_WIRE_FILE_SIZE {
            return Err(WireStatus::BadLength);
        }
        let is_calib = transfer.path_str() == CALIB_FILE_PATH;
        let staging = crate::data::arena::staging_buf();
        if is_calib {
            if end > staging.len() {
                return Err(WireStatus::BadLength);
            }
            staging[off..end].copy_from_slice(req.data);
        } else {
            let expected_off = transfer.buf_offset as usize + transfer.buf_len as usize;
            if off != expected_off || transfer.buf_len as usize + chunk_len > staging.len() {
                flush_transfer_buf(transfer, ctx.hw.filesystem, true)?;
                transfer.buf_offset = off as u32;
            }
            let pos = transfer.buf_len as usize;
            staging[pos..pos + chunk_len].copy_from_slice(req.data);
            transfer.buf_len = (pos + chunk_len) as u16;
            flush_transfer_buf(transfer, ctx.hw.filesystem, false)?;
        }
        let new_len = end as u32;
        transfer.current_len = transfer.current_len.max(new_len);
        ctx.write_u16(chunk_len as u16);
        ctx.write_u32(new_len);
    } else {
        let len = (req.length as usize).min(120);
        let read_sz = ctx
            .hw
            .filesystem
            .read_file_at(transfer.path_str(), off, &mut ctx.out[8..8 + len])
            .unwrap_or(0);
        ctx.write_u32(off as u32);
        ctx.write_u16(read_sz as u16);
        ctx.pos = 8 + read_sz;
    }
    Ok(())
}

#[inline(never)]
fn commit_calibration_payload(
    staging: &[u8; 512],
    total_len: usize,
    hw: &mut SystemHardware,
) -> Result<(), WireStatus> {
    if total_len != 512 || !is_valid_calibration(staging) {
        return Err(WireStatus::SafetyDenied);
    }

    let mcu_uid = bsp::uid::read_mcu_uid();
    let calib = unsafe { &mut *core::ptr::addr_of_mut!(crate::CALIBRATION) };
    if calib
        .commit_with_backup(hw.filesystem, staging, &mcu_uid)
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

#[inline(never)]
fn on_transfer_close(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let req = TransferCloseRequest::parse(payload).ok_or(WireStatus::BadLength)?;
    let transfer = active_transfer_mut(req.transfer_id).ok_or(WireStatus::BadSession)?;

    transfer.active = false;
    let mut actual_crc = 0u32;
    if transfer.mode == TRANSFER_MODE_WRITE {
        let total_len = transfer.current_len as usize;
        if transfer.path_str() == CALIB_FILE_PATH {
            let staging = crate::data::arena::staging_buf();
            commit_calibration_payload(staging, total_len, ctx.hw)?;
            actual_crc = crypto::crc::CrcEngine::crc32(staging);
            if req.expected_crc32 != 0 && req.expected_crc32 != actual_crc {
                return Err(WireStatus::Failed);
            }
        } else {
            flush_transfer_buf(transfer, ctx.hw.filesystem, true)?;
            let path = transfer.path_str();
            actual_crc = stream_file_crc32(ctx.hw.filesystem, path, total_len);
            if req.expected_crc32 != 0 && req.expected_crc32 != actual_crc {
                let _ = ctx.hw.filesystem.remove(path);
                return Err(WireStatus::Failed);
            }
        }
        invalidate_apps_if_needed(transfer.path_str());
    }

    ctx.write_u16(req.transfer_id);
    ctx.write_u32(actual_crc);
    Ok(())
}

#[inline(never)]
fn on_transfer_stat(payload: &[u8], ctx: &mut WireCtx<'_, '_>) -> Result<(), WireStatus> {
    let req = TransferStatRequest::parse(payload).ok_or(WireStatus::BadLength)?;

    let Ok((sz, is_dir)) = stat_or_root(ctx.hw.filesystem, req.path) else {
        ctx.write_zeroes(10);
        return Ok(());
    };

    let sz = if is_dir { 0 } else { sz };
    let crc = if sz > 0 {
        stream_file_crc32(ctx.hw.filesystem, req.path, sz)
    } else {
        0
    };
    ctx.write_u16(1 | (u16::from(is_dir) << 8));
    ctx.write_u32(sz as u32);
    ctx.write_u32(crc);
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

    let start = req.cursor;
    let mut idx = 0u16;
    let mut next_cursor = 0xFFFFu16;
    let mut written = 0u8;
    let out = &mut *ctx.out;
    let mut pos = 3usize;

    let _ = ctx
        .hw
        .filesystem
        .fs
        .list_dir(path, |name, size, _perm, is_dir| {
            if next_cursor == 0xFFFF {
                if idx >= start {
                    let b = name.as_bytes();
                    let name_len = b.len().min(24);
                    if pos + 6 + name_len > out.len() {
                        next_cursor = idx;
                        return;
                    }
                    out[pos] = is_dir as u8;
                    out[pos + 1..pos + 5].copy_from_slice(&(size as u32).to_le_bytes());
                    out[pos + 5] = name_len as u8;
                    out[pos + 6..pos + 6 + name_len].copy_from_slice(&b[..name_len]);
                    pos += 6 + name_len;
                    written += 1;
                }
                idx += 1;
            }
        });

    out[0..2].copy_from_slice(&next_cursor.to_le_bytes());
    out[2] = written;
    ctx.pos = pos;
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
        OP_TRANSFER_CHUNK => on_transfer_chunk(payload, ctx),
        OP_TRANSFER_CLOSE => on_transfer_close(payload, ctx),
        OP_TRANSFER_ABORT => match TransferAbortRequest::parse(payload) {
            Some(req) => {
                if let Some(transfer) = active_transfer_mut(req.transfer_id) {
                    transfer.active = false;
                    let path = transfer.path_str();
                    if transfer.mode == TRANSFER_MODE_WRITE && path != CALIB_FILE_PATH {
                        let _ = ctx.hw.filesystem.remove(path);
                        invalidate_apps_if_needed(path);
                    }
                }
                ctx.write_u16(req.transfer_id);
                Ok(())
            }
            None => Err(WireStatus::BadLength),
        },
        OP_TRANSFER_STAT => on_transfer_stat(payload, ctx),
        OP_TRANSFER_LIST => on_transfer_list(payload, ctx),
        OP_TRANSFER_REMOVE => match TransferRemoveRequest::parse(payload) {
            Some(req) => {
                check_regular_file(ctx.hw.filesystem, req.path, true, false).and_then(|_| {
                    ctx.hw
                        .filesystem
                        .remove(req.path)
                        .map_err(|_| WireStatus::NotFound)?;
                    invalidate_apps_if_needed(req.path);
                    Ok(())
                })
            }
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
            payload[..2].copy_from_slice(&event_id.to_le_bytes());
            let d_len = data.len().min(30);
            payload[2..2 + d_len].copy_from_slice(&data[..d_len]);
            let _ = hw
                .wire
                .send_event(SERVICE_EVENTS, OP_EVENTS_EMIT, 0, &payload[..2 + d_len]);
        }
    }
}
