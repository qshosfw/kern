//! Declarative Firmware RigWire v2 Service & Component Dispatcher.
//!
//! Bridges RigWire v2 serial transport with microkernel services, radio control,
//! display mirroring, structured table sector-hash delta sync, and streaming KernFS
//! transfers using a single-context zero-copy cursor (`WireCtx`) and `abi::Status`.

use crate::hardware::SystemHardware;
use crate::services::calibration::CALIB_FILE_PATH;
use crate::services::primitives;
use abi::Status;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::service::{PowerService, RfFineTuning, SerialPort};
use kernel::wire::*;
use utils::units::Frequency;

/// Single-indirect KernFS page limit: 8 L1 index pages × 128 pages × 256 B = 256 KiB.
const MAX_WIRE_FILE_SIZE: usize = 8 * 128 * 256;

/// Active transfer session state coalescing writes in the 512 B `staging_buf`.
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
    pub last_activity_ms: u32,
    pub path: [u8; 32],
}

impl ActiveTransfer {
    pub const fn empty() -> Self {
        Self {
            active: false,
            mode: TRANSFER_MODE_READ,
            path_len: 0,
            transfer_id: 0,
            chunk_size: 0,
            buf_len: 0,
            total_size: 0,
            current_len: 0,
            buf_offset: 0,
            last_activity_ms: 0,
            path: [0u8; 32],
        }
    }

    #[inline(always)]
    pub fn path_str(&self) -> &str {
        utils::format::ascii_str(&self.path[..self.path_len as usize]).unwrap_or("")
    }
}

pub static mut CURRENT_TRANSFER: ActiveTransfer = ActiveTransfer::empty();
static mut NEXT_TRANSFER_ID: u16 = 1;

#[inline(always)]
fn transfer_mut() -> &'static mut ActiveTransfer {
    unsafe { &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER) }
}

#[inline(never)]
fn active_transfer_mut(transfer_id: u16) -> Result<&'static mut ActiveTransfer, Status> {
    let t = transfer_mut();
    if t.active && t.transfer_id == transfer_id {
        Ok(t)
    } else {
        Err(Status::BadSession)
    }
}

/// Event streaming subscription state.
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

// ── Unified Single-Register Wire Cursor ─────────────────────────────────────

struct WireCtx<'a, 'b> {
    inp: &'a [u8],
    out: &'a mut [u8; 128],
    pos: usize,
    now: u32,
    #[cfg(test)]
    hw: &'a mut SystemHardware<'b>,
    #[cfg(not(test))]
    _b: core::marker::PhantomData<&'b ()>,
}

impl<'a, 'b> WireCtx<'a, 'b> {
    #[inline(always)]
    fn hw(&mut self) -> &mut SystemHardware<'b> {
        #[cfg(not(test))]
        {
            unsafe {
                &mut *(crate::kern_hw() as *mut SystemHardware<'static> as *mut SystemHardware<'b>)
            }
        }
        #[cfg(test)]
        {
            self.hw
        }
    }

    #[inline(always)]
    fn fs(&mut self) -> &mut crate::services::SystemStorage<'b> {
        #[cfg(not(test))]
        {
            unsafe {
                &mut *(crate::kern_fs() as *mut crate::services::SystemStorage<'static>
                    as *mut crate::services::SystemStorage<'b>)
            }
        }
        #[cfg(test)]
        {
            self.hw.filesystem
        }
    }

    #[inline(never)]
    fn take(&mut self, n: usize) -> Result<&'a [u8], Status> {
        if self.inp.len() < n {
            return Err(Status::BadLength);
        }
        let (head, tail) = self.inp.split_at(n);
        self.inp = tail;
        Ok(head)
    }

    #[inline(never)]
    fn rd_le(&mut self, n: usize) -> Result<u32, Status> {
        let s = self.take(n)?;
        let mut v = 0u32;
        for &b in s.iter().rev() {
            v = (v << 8) | b as u32;
        }
        Ok(v)
    }

    #[inline(never)]
    fn rd_path(&mut self) -> Result<&'a str, Status> {
        let len = self.rd_u8()? as usize;
        utils::format::ascii_str(self.take(len)?).ok_or(Status::BadLength)
    }

    #[inline(always)]
    fn rd_u8(&mut self) -> Result<u8, Status> {
        Ok(self.rd_le(1)? as u8)
    }

    #[inline(always)]
    fn rd_u16(&mut self) -> Result<u16, Status> {
        Ok(self.rd_le(2)? as u16)
    }

    #[inline(always)]
    fn rd_u32(&mut self) -> Result<u32, Status> {
        self.rd_le(4)
    }

    #[inline(never)]
    fn put(&mut self, data: &[u8]) {
        let n = data.len().min(128 - self.pos);
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), self.out.as_mut_ptr().add(self.pos), n);
        }
        self.pos += n;
    }

    #[inline(never)]
    fn zeroes(&mut self, len: usize) {
        let n = len.min(128 - self.pos);
        unsafe {
            core::ptr::write_bytes(self.out.as_mut_ptr().add(self.pos), 0, n);
        }
        self.pos += n;
    }

    #[inline(never)]
    fn wr_le(&mut self, mut v: u32, n: usize) {
        let count = n.min(128 - self.pos);
        let p = unsafe { self.out.as_mut_ptr().add(self.pos) };
        for i in 0..count {
            unsafe { *p.add(i) = v as u8 };
            v >>= 8;
        }
        self.pos += count;
    }

    #[inline(always)]
    fn wr_u8(&mut self, v: u8) {
        self.wr_le(v as u32, 1);
    }

    #[inline(always)]
    fn wr_u16(&mut self, v: u16) {
        self.wr_le(v as u32, 2);
    }

    #[inline(always)]
    fn wr_u32(&mut self, v: u32) {
        self.wr_le(v, 4);
    }
}

/// Main entry point for non-core RigWire v2 records routed from `WireDriver`.
#[inline(never)]
pub fn dispatch_wire_record(header: &WireHeader, hw: &mut SystemHardware, now: u32) -> bool {
    let raw = hw.wire.current_payload(header);
    let plen = raw.len().min(128);
    // Handlers only read `hw.wire.session` and never mutate `rx_buf` until `send_response`.
    let inp = unsafe { core::slice::from_raw_parts(raw.as_ptr(), plen) };

    #[cfg(not(test))]
    let out: &mut [u8; 128] = unsafe {
        static mut WIRE_OUT_BUF: [u8; 128] = [0u8; 128];
        &mut *core::ptr::addr_of_mut!(WIRE_OUT_BUF)
    };
    #[cfg(test)]
    let mut out_buf = [0u8; 128];
    #[cfg(test)]
    let out = &mut out_buf;

    let mut c = WireCtx {
        inp,
        out,
        pos: 0,
        now,
        #[cfg(test)]
        hw,
        #[cfg(not(test))]
        _b: core::marker::PhantomData,
    };

    let res = match header.channel {
        CH_CONTROL => handle_control_service(header.op, &mut c),
        CH_DATA => handle_data_service(header.op, &mut c),
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
    let Some(status) = res else {
        return false;
    };
    let resp_len = if status.is_ok() { c.pos } else { 0 };
    let _ = hw.wire.send_response(header, status, &out[..resp_len]);
    true
}

// ── CH_CONTROL (0x01): Universal Resource Handle RPC ────────────────────────

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

#[inline(always)]
fn config_domain_idx(instance: u8) -> Result<usize, Status> {
    if instance > 5 {
        Err(Status::InvalidArg)
    } else {
        Ok(instance as usize)
    }
}

#[inline(never)]
fn read_sys_facet(facet: u16, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    match facet {
        FACET_SYS_IDENTITY => {
            let serial = bsp::uid::get_hardware_serial64();
            c.wr_u32(serial as u32);
            c.wr_u32((serial >> 32) as u32);
            let mut mac = [0u8; 6];
            bsp::uid::mac_from_serial64_into(serial, &mut mac);
            c.put(&mac);
            let mut cf = [0u8; 15];
            bsp::uid::encode_crockford64_into(serial, &mut cf);
            c.put(&cf[..14]);
            let is_auth = c.hw().wire.session.is_authenticated;
            c.wr_u8(is_auth as u8);
            if is_auth {
                c.put(&bsp::uid::read_mcu_uid());
            } else {
                c.zeroes(12);
            }
        }
        FACET_SYS_METRICS => {
            let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
            let now = c.now;
            c.wr_u32(now);
            c.wr_u32(py32_hal::rcc::sysclk_hz());
            c.wr_u16(pwr.battery_voltage_mv());
            let app_id = c.hw().orchestrator.active_app_id() as u8;
            let locked = c.hw().orchestrator.is_keypad_locked() as u8;
            let bk_gpio = c.hw().radio.read_gpio_reg();
            let status_led = if bk_gpio == 0xFFFF { 0 } else { (bk_gpio & 3) as u8 };
            c.put(&[
                pwr.battery_percentage(),
                app_id,
                locked,
                bsp::pins::FLASHLIGHT.is_high() as u8,
                status_led,
            ]);
            c.wr_u16(bk_gpio);
            let disp = &crate::master_config_ref().display;
            let bl_duty = drivers::backlight::BACKLIGHT_DUTY
                .load(core::sync::atomic::Ordering::Relaxed);
            c.put(&[
                crate::system::is_headless() as u8,
                bl_duty,
                disp.contrast,
                disp.inverted,
            ]);
        }
        FACET_SYS_CALIBRATION => {
            let calib = unsafe { &*core::ptr::addr_of!(crate::CALIBRATION) };
            c.wr_u8(calib.is_valid() as u8);
            c.wr_u16(calib.xtal_freq_low() as u16);
            c.wr_u16(calib.battery_calib_const());
        }
        FACET_SYS_AUDIO => {
            let rem = c.out.len().saturating_sub(c.pos).min(64);
            let n = crate::system::drain_headless_notes(&mut c.out[c.pos..c.pos + rem]);
            c.pos += n;
        }
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

#[inline(never)]
fn read_radio_facet(facet: u16, instance: u8, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let (slot, ch) = active_radio_slot(instance);
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
            let rssi = c.hw().radio.get_rssi();
            let open = c.hw().radio.is_squelch_open() as u8;
            c.wr_u8(ch.squelch_level);
            c.wr_u16(rssi);
            c.wr_u8(open);
        }
        FACET_RADIO_TRANSMIT => {
            c.put(&[coord.is_slot_tx_active(slot) as u8, ch.power as u8]);
        }
        FACET_RADIO_DIGITAL => c.zeroes(2),
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

#[inline(never)]
fn on_handle_get(handle: ResourceHandle, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    match (handle.domain, handle.facet) {
        (DOMAIN_SYSTEM, facet) => return read_sys_facet(facet, c),
        (DOMAIN_RADIO, facet) => return read_radio_facet(facet, handle.instance, c),
        (DOMAIN_UI, FACET_UI_SURFACE) => c.put(&[128, 0, 64, 0, 1, 0, 4]),
        (DOMAIN_UI, FACET_UI_MODAL) => {
            let modal =
                unsafe { &*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL) };
            c.put(&[modal.is_active() as u8, modal.tag]);
        }
        (DOMAIN_UI, FACET_UI_VIEW) | (DOMAIN_APP, FACET_APP_LIFECYCLE) => {
            let id = c.hw().orchestrator.active_app_id() as u8;
            c.wr_u8(id);
        }
        (DOMAIN_INPUT, FACET_INPUT_LOCK) => {
            let locked = c.hw().orchestrator.is_keypad_locked() as u8;
            c.wr_u8(locked);
        }
        (DOMAIN_APP, FACET_APP_REGISTRY) => c.wr_u8(crate::AppRegistry::app_count() as u8),
        (DOMAIN_CONFIG, _) => {
            let domain_idx = config_domain_idx(handle.instance)?;
            c.put(crate::master_config_ref().config_bytes(domain_idx));
        }
        (DOMAIN_STORAGE, _) => {
            let s = c.fs().stats();
            #[repr(C, packed)]
            struct StatsWire(u16, u16, u16, u16, u32, u16);
            let w = StatsWire(
                s.active_files,
                s.used_sectors,
                s.free_sectors,
                s.deleted_sectors,
                s.wear_cycles,
                s.total_sectors,
            );
            c.put(unsafe { core::slice::from_raw_parts(&w as *const _ as *const u8, 14) });
        }
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

#[inline(never)]
fn handle_resource_set(handle: ResourceHandle, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    if (handle.domain == DOMAIN_SYSTEM && handle.facet == FACET_SYS_METRICS)
        || (handle.domain == DOMAIN_INPUT && handle.facet == FACET_INPUT_LOCK)
    {
        if handle.domain == DOMAIN_INPUT || !c.inp.is_empty() {
            let locked = c.rd_u8()? != 0;
            let orch = &mut c.hw().orchestrator;
            if locked != orch.is_keypad_locked() {
                orch.toggle_keypad_lock();
            }
        }
        return Ok(());
    }
    match handle.domain {
        DOMAIN_RADIO => {
            let (_slot, ch) = active_radio_slot(handle.instance);
            let coord = &mut crate::radio_state().coord;
            match handle.facet {
                FACET_RADIO_TUNING => {
                    let freq_hz = c.rd_u32()?;
                    if !(18_000_000..=1_300_000_000).contains(&freq_hz) {
                        return Err(Status::InvalidArg);
                    }
                    let freq = Frequency::new(freq_hz);
                    ch.rx_freq = freq;
                    let _ = c.hw().radio.set_frequency(freq);
                }
                FACET_RADIO_MODULATION | FACET_RADIO_SQUELCH => {
                    let v = c.rd_u8()?;
                    let param = (handle.facet - FACET_RADIO_MODULATION) as u8;
                    primitives::rf_set_param(coord, &mut c.hw().radio, param, v);
                }
                FACET_RADIO_TRANSMIT => {
                    let tx = c.rd_u8()? != 0;
                    let radio = &mut c.hw().radio;
                    if tx {
                        let token = kernel::service::radio::ClientToken::new(
                            kernel::service::radio::ClientId::SYSTEM,
                            kernel::service::radio::LockTier::FOREGROUND,
                        );
                        if !coord
                            .check_tx_permission(coord.effective_tx_slot(), token)
                            .is_granted()
                        {
                            return Err(Status::SafetyBlocked);
                        }
                        coord.start_tx(radio);
                        crate::system::set_wire_tx(coord.is_transmitting);
                    } else {
                        coord.stop_tx(radio);
                        crate::system::set_wire_tx(false);
                    }
                }
                FACET_RADIO_DIGITAL => {
                    let offset_dhz = c.rd_u16()? as i16;
                    c.hw().radio.set_sub_hz_offset(ch.rx_freq.raw(), offset_dhz);
                }
                _ => return Err(Status::Unsupported),
            }
            Ok(())
        }
        DOMAIN_INPUT if handle.facet == FACET_INPUT_KEY => {
            let code = KeyCode::from_u8(c.rd_u8()?).ok_or(Status::InvalidArg)?;
            let state = [KeyState::Pressed, KeyState::Held, KeyState::Released]
                [c.inp.first().copied().unwrap_or(0).min(2) as usize];
            crate::system::enqueue_event(Event::Key { code, state });
            Ok(())
        }
        DOMAIN_INPUT if handle.facet == FACET_INPUT_TEXT => {
            for &b in c.inp.iter().take(32) {
                if (0x20..=0x7E).contains(&b) {
                    crate::system::enqueue_event(Event::Char(b as char));
                }
            }
            Ok(())
        }
        DOMAIN_INPUT if handle.facet == FACET_INPUT_ENCODER => {
            let delta = c.rd_u8()? as i8;
            if delta != 0 {
                crate::system::enqueue_event(Event::Encoder(delta));
            }
            Ok(())
        }
        DOMAIN_CONFIG => {
            let domain_idx = config_domain_idx(handle.instance)?;
            let mcfg = crate::master_config();
            let dest = mcfg.config_bytes_mut(domain_idx);
            if c.inp.len() != dest.len() {
                return Err(Status::BadLength);
            }
            if c.inp[0] != crate::config::MasterConfig::domain_version(domain_idx) {
                return Err(Status::IncompatibleAbi);
            }
            dest.copy_from_slice(c.inp);
            mcfg.mark_domain_dirty(domain_idx);
            mcfg.sanitize();
            let _ = mcfg.flush_dirty(c.fs());
            Ok(())
        }
        _ => Err(Status::Unsupported),
    }
}

#[inline(never)]
fn handle_resource_invoke(handle: ResourceHandle, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let action_id = c.rd_u16()?;
    c.wr_u16(action_id);
    let arg = c.inp.first().copied().unwrap_or(0);
    match (handle.domain, action_id) {
        (DOMAIN_SYSTEM, ACTION_SYS_REBOOT) => {
            let mode = match arg {
                1 => kernel::power::RebootMode::Bootloader,
                2 => kernel::power::RebootMode::SafeRecovery,
                _ => kernel::power::RebootMode::Firmware,
            };
            c.hw().wire.port.flush();
            kernel::power::PowerControl::reboot_with(mode);
        }
        (DOMAIN_SYSTEM, ACTION_SYS_FLASHLIGHT | ACTION_SYS_HEADLESS) => {
            let is_fl = action_id == ACTION_SYS_FLASHLIGHT;
            let cur = if is_fl {
                bsp::pins::FLASHLIGHT.is_high()
            } else {
                crate::system::is_headless()
            };
            let enable = match c.inp.first().copied().unwrap_or(2) {
                0 => false,
                1 => true,
                _ => !cur,
            };
            if is_fl {
                if enable {
                    bsp::pins::FLASHLIGHT.set_high();
                } else {
                    bsp::pins::FLASHLIGHT.set_low();
                }
            } else {
                crate::system::set_headless(c.hw(), enable);
                c.wr_u8(enable as u8);
            }
        }
        (DOMAIN_RADIO, ACTION_RADIO_VFO_TOGGLE) => {
            crate::radio_state().coord.toggle_vfo(&mut c.hw().radio);
        }
        (DOMAIN_UI, ACTION_MODAL_DISMISS) => unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL)).dismiss();
        },
        (DOMAIN_UI, ACTION_MODAL_CONFIRM | ACTION_MODAL_CANCEL | ACTION_VIEW_LAUNCHER)
        | (DOMAIN_APP, ACTION_APP_KILL) => {
            let (code, state) = match (handle.domain, action_id) {
                (DOMAIN_UI, ACTION_MODAL_CONFIRM) => (KeyCode::Menu, KeyState::Pressed),
                (DOMAIN_UI, ACTION_MODAL_CANCEL) => (KeyCode::Exit, KeyState::Pressed),
                _ => (KeyCode::Exit, KeyState::Held),
            };
            crate::system::enqueue_event(Event::Key { code, state });
        }
        (DOMAIN_INPUT, ACTION_INPUT_LOCK_TOGGLE) => {
            c.hw().orchestrator.toggle_keypad_lock();
        }
        (DOMAIN_APP, ACTION_APP_LAUNCH) => {
            if (arg as usize) >= crate::AppRegistry::app_count() {
                return Err(Status::InvalidArg);
            }
        }
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

#[inline(never)]
fn handle_control_service(opcode: u8, c: &mut WireCtx<'_, '_>) -> Option<Status> {
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
        OP_HANDLE_GET => on_handle_get(handle, c),
        OP_HANDLE_SET => handle_resource_set(handle, c),
        OP_HANDLE_INVOKE => handle_resource_invoke(handle, c),
        OP_HANDLE_STREAM => c.rd_u32().and_then(|offset| {
            let length = c.rd_u16()?;
            if handle.domain != DOMAIN_UI || handle.facet != FACET_UI_SURFACE {
                return Err(Status::Unsupported);
            }
            let flat = crate::system_canvas().buffer_flat();
            if (offset & 0x8000_0000) != 0 {
                let prev = c.inp;
                c.wr_u32(0x8000_0000);
                c.wr_u16(0);
                c.wr_u16(0);
                let mut dirty_mask = 0u16;
                let mut sent_mask = 0u16;
                #[cfg(feature = "app_hibernation")]
                let mut rle_buf = [0u8; 64];
                for s in 0..16usize {
                    let strip = &flat[s * 64..(s + 1) * 64];
                    let crc = crypto::crc::CrcEngine::crc16_ccitt(strip);
                    let p_off = s * 2;
                    let same = prev.len() >= p_off + 2
                        && u16::from_le_bytes([prev[p_off], prev[p_off + 1]]) == crc;
                    if !same {
                        dirty_mask |= 1 << s;
                        #[cfg(feature = "app_hibernation")]
                        let (src, slen) = {
                            let rle_len =
                                kernel::runtime::packbits_compress(strip, &mut rle_buf).unwrap_or(0);
                            if rle_len > 0 && rle_len < 64 {
                                (&rle_buf[..rle_len], rle_len)
                            } else {
                                (strip, 64)
                            }
                        };
                        #[cfg(not(feature = "app_hibernation"))]
                        let (src, slen) = (strip, 64usize);
                        if c.pos + 1 + slen <= 116 {
                            c.wr_u8(slen as u8);
                            c.put(src);
                            sent_mask |= 1 << s;
                        }
                    }
                }
                c.out[8] = dirty_mask as u8;
                c.out[9] = (dirty_mask >> 8) as u8;
                c.out[10] = sent_mask as u8;
                c.out[11] = (sent_mask >> 8) as u8;
                return Ok(());
            }
            let off = (offset as usize).min(flat.len());
            let len = (length as usize).min(100).min(flat.len() - off);
            c.wr_u32(off as u32);
            c.wr_u16(len as u16);
            c.put(&flat[off..off + len]);
            Ok(())
        }),
        _ => return None,
    };
    Some(res.err().unwrap_or(Status::Ok))
}

// ── CH_DATA (0x03): Hardened KernFS Streaming & Sector Hash Sync ────────────

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

#[inline(never)]
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
) -> Result<u32, Status> {
    let b = path.as_bytes();
    if b.len() > 32 || !utils::path::is_clean_path(b) {
        return Err(Status::InvalidArg);
    }
    let root = utils::path::root_bit(b);
    if root == 0
        || (is_write
            && root == utils::path::ROOT_SYS
            && (!allow_missing_or_calib || path != CALIB_FILE_PATH))
    {
        return Err(Status::SafetyBlocked);
    }
    match fs.stat_info(path) {
        Ok((_, true)) => Err(Status::InvalidArg),
        Ok((sz, false)) => Ok(sz as u32),
        Err(_) if allow_missing_or_calib => Ok(0),
        Err(_) => Err(Status::NotFound),
    }
}

#[inline(never)]
fn flush_transfer_buf(
    transfer: &mut ActiveTransfer,
    fs: &mut crate::services::SystemStorage,
    flush_all: bool,
) -> Result<(), Status> {
    let staging = crate::data::arena::staging_buf();
    loop {
        let len = transfer.buf_len as usize;
        let write_len = if len >= 256 {
            256
        } else if flush_all && len > 0 {
            len
        } else {
            return Ok(());
        };
        fs.write_at(
            transfer.path_str(),
            transfer.buf_offset as usize,
            &staging[..write_len],
        )
        .map_err(|_| Status::IoError)?;
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
fn on_transfer_open(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let mode = c.rd_u8()?;
    let total_size = c.rd_u32()?;
    let req_chunk = c.rd_u16()?;
    let path = c.rd_path()?;
    if mode > TRANSFER_MODE_WRITE {
        return Err(Status::InvalidArg);
    }
    let is_write = mode == TRANSFER_MODE_WRITE;
    let existing_size = check_regular_file(c.fs(), path, is_write, is_write)?;
    if is_write {
        let req_sz = total_size as usize;
        if req_sz > MAX_WIRE_FILE_SIZE {
            return Err(Status::BadLength);
        }
        let is_calib = path == CALIB_FILE_PATH;
        if is_calib && req_sz != 512 {
            return Err(Status::SafetyBlocked);
        }
        if req_sz > c.fs().stats().avail_bytes() + existing_size as usize {
            return Err(Status::NoSpace);
        }
        if !is_calib && c.fs().write_file(path, &[]).is_err() {
            return Err(Status::IoError);
        }
    }

    let tid = unsafe {
        let p = core::ptr::addr_of_mut!(NEXT_TRANSFER_ID);
        let id = *p;
        *p = id.wrapping_add(1).max(1);
        id
    };
    let chunk_size = req_chunk.clamp(16, 120);
    let path_bytes = path.as_bytes();
    let t = transfer_mut();
    *t = ActiveTransfer::empty();
    t.active = true;
    t.mode = mode;
    t.transfer_id = tid;
    t.chunk_size = chunk_size;
    t.total_size = total_size;
    t.last_activity_ms = c.now;
    unsafe {
        core::ptr::copy_nonoverlapping(path_bytes.as_ptr(), t.path.as_mut_ptr(), path_bytes.len());
    }
    t.path_len = path_bytes.len() as u8;

    c.wr_u16(tid);
    c.wr_u16(chunk_size);
    c.wr_u32(existing_size);
    Ok(())
}

#[inline(never)]
fn on_transfer_chunk(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let tid = c.rd_u16()?;
    let off = c.rd_u32()? as usize;
    let length = c.rd_u16()?;
    let data = c.inp;
    let transfer = active_transfer_mut(tid)?;
    transfer.last_activity_ms = c.now;

    c.wr_u16(tid);
    if transfer.mode == TRANSFER_MODE_WRITE {
        let chunk_len = data.len();
        let end = off.checked_add(chunk_len).ok_or(Status::InvalidArg)?;
        if end > MAX_WIRE_FILE_SIZE {
            return Err(Status::BadLength);
        }
        let staging = crate::data::arena::staging_buf();
        if transfer.path_str() == CALIB_FILE_PATH {
            if end > staging.len() {
                return Err(Status::BadLength);
            }
            staging[off..end].copy_from_slice(data);
        } else {
            let expected_off = transfer.buf_offset as usize + transfer.buf_len as usize;
            if off != expected_off || transfer.buf_len as usize + chunk_len > staging.len() {
                flush_transfer_buf(transfer, c.fs(), true)?;
                transfer.buf_offset = off as u32;
            }
            let pos = transfer.buf_len as usize;
            staging[pos..pos + chunk_len].copy_from_slice(data);
            transfer.buf_len = (pos + chunk_len) as u16;
            flush_transfer_buf(transfer, c.fs(), false)?;
        }
        let new_len = end as u32;
        transfer.current_len = transfer.current_len.max(new_len);
        c.wr_u16(chunk_len as u16);
        c.wr_u32(new_len);
    } else {
        let len = (length as usize).min(120);
        let read_sz = {
            let out_ptr = c.out.as_mut_ptr();
            let dst = unsafe { core::slice::from_raw_parts_mut(out_ptr.add(8), len) };
            c.fs()
                .read_file_at(transfer.path_str(), off, dst)
                .unwrap_or(0)
        };
        c.wr_u32(off as u32);
        c.wr_u16(read_sz as u16);
        c.pos = 8 + read_sz;
    }
    Ok(())
}

#[inline(never)]
fn on_transfer_close(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let tid = c.rd_u16()?;
    let expected_crc = c.rd_u32()?;
    let transfer = active_transfer_mut(tid)?;
    transfer.last_activity_ms = c.now;

    transfer.active = false;
    let mut actual_crc = 0u32;
    if transfer.mode == TRANSFER_MODE_WRITE {
        let total_len = transfer.current_len as usize;
        if transfer.path_str() == CALIB_FILE_PATH {
            let staging = crate::data::arena::staging_buf();
            if total_len != 512 {
                return Err(Status::SafetyBlocked);
            }
            let calib = unsafe { &mut *core::ptr::addr_of_mut!(crate::CALIBRATION) };
            calib
                .commit_slice(c.fs(), staging)
                .map_err(|_| Status::SafetyBlocked)?;
            let radio = &mut c.hw().radio;
            crate::apply_active_calibration(radio);
            radio.set_xtal_freq_trim(calib.xtal_freq_low());
            actual_crc = crypto::crc::CrcEngine::crc32(staging);
            if expected_crc != 0 && expected_crc != actual_crc {
                return Err(Status::CorruptData);
            }
        } else {
            flush_transfer_buf(transfer, c.fs(), true)?;
            let path = transfer.path_str();
            actual_crc = stream_file_crc32(c.fs(), path, total_len);
            if expected_crc != 0 && expected_crc != actual_crc {
                let _ = c.fs().remove(path);
                return Err(Status::CorruptData);
            }
        }
        invalidate_apps_if_needed(transfer.path_str());
    }

    c.wr_u16(tid);
    c.wr_u32(actual_crc);
    Ok(())
}

#[inline(never)]
fn on_transfer_abort(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let tid = c.rd_u16()?;
    let _reason = c.rd_u8()?;
    if let Ok(transfer) = active_transfer_mut(tid) {
        transfer.active = false;
        let path = transfer.path_str();
        if transfer.mode == TRANSFER_MODE_WRITE && path != CALIB_FILE_PATH {
            let _ = c.fs().remove(path);
            invalidate_apps_if_needed(path);
        }
    }
    c.wr_u16(tid);
    Ok(())
}

#[inline(never)]
fn on_transfer_stat(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let path = c.rd_path()?;
    let Ok((sz, is_dir)) = stat_or_root(c.fs(), path) else {
        c.zeroes(10);
        return Ok(());
    };
    let sz = if is_dir { 0 } else { sz };
    let crc = if sz > 0 {
        stream_file_crc32(c.fs(), path, sz)
    } else {
        0
    };
    c.wr_u16(1 | (u16::from(is_dir) << 8));
    c.wr_u32(sz as u32);
    c.wr_u32(crc);
    Ok(())
}

#[inline(never)]
fn on_transfer_list(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let start = c.rd_u16()?;
    let path = trim_trailing_slash(c.rd_path()?);

    let t = transfer_mut();
    if t.active && c.now.wrapping_sub(t.last_activity_ms) >= 5_000 {
        t.active = false;
    }
    if t.active && t.mode == TRANSFER_MODE_WRITE {
        return Err(Status::Busy);
    }
    match stat_or_root(c.fs(), path) {
        Ok((_, true)) => {}
        Ok((_, false)) => return Err(Status::InvalidArg),
        Err(_) => return Err(Status::NotFound),
    }

    let mut idx = 0u16;
    let mut next_cursor = 0xFFFFu16;
    let mut written = 0u8;
    let out_ptr = c.out.as_mut_ptr();
    let mut pos = 3usize;

    let _ = c.fs().for_each_entry(path, |name, size, _perm, is_dir| {
        if next_cursor == 0xFFFF {
            if idx >= start {
                let b = name.as_bytes();
                let name_len = b.len().min(24);
                if pos + 6 + name_len > 128 {
                    next_cursor = idx;
                    return;
                }
                unsafe {
                    let p = out_ptr.add(pos);
                    *p = is_dir as u8;
                    let sz = (size as u32).to_le_bytes();
                    core::ptr::copy_nonoverlapping(sz.as_ptr(), p.add(1), 4);
                    *p.add(5) = name_len as u8;
                    core::ptr::copy_nonoverlapping(b.as_ptr(), p.add(6), name_len);
                }
                pos += 6 + name_len;
                written += 1;
            }
            idx += 1;
        }
    });

    c.out[0] = next_cursor as u8;
    c.out[1] = (next_cursor >> 8) as u8;
    c.out[2] = written;
    c.pos = pos;
    Ok(())
}

#[inline(never)]
fn on_transfer_remove(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let path = c.rd_path()?;
    check_regular_file(c.fs(), path, true, false)?;
    c.fs().remove(path).map_err(|_| Status::NotFound)?;
    invalidate_apps_if_needed(path);
    Ok(())
}

/// Paginated IEEE CRC-32 digest per 256-byte page slice for O(changed pages) delta sync.
/// Request: `[start_page: u16, path_len: u8, path...]`
/// Reply:   `[total_pages: u16, count: u8, hashes: [u32; count]]` (up to 30 pages/frame).
#[inline(never)]
fn on_transfer_sector_hash(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let start_page = c.rd_u16()? as usize;
    let path = c.rd_path()?;
    let sz = check_regular_file(c.fs(), path, false, false)? as usize;
    let total_pages = (sz + 255) >> 8;
    let count = total_pages.saturating_sub(start_page).min(30);
    c.wr_u16(total_pages as u16);
    c.wr_u8(count as u8);
    let buf = &mut crate::data::arena::staging_buf()[..256];
    for page in start_page..start_page + count {
        let n = c.fs().read_file_at(path, page << 8, buf).unwrap_or(0);
        c.wr_u32(crypto::crc::CrcEngine::crc32(&buf[..n]));
    }
    Ok(())
}

#[inline(never)]
fn handle_data_service(opcode: u8, c: &mut WireCtx<'_, '_>) -> Option<Status> {
    const DATA_OPS: [fn(&mut WireCtx<'_, '_>) -> Result<(), Status>; 8] = [
        on_transfer_open,
        on_transfer_chunk,
        on_transfer_close,
        on_transfer_abort,
        on_transfer_stat,
        on_transfer_list,
        on_transfer_remove,
        on_transfer_sector_hash,
    ];
    let handler = DATA_OPS.get(opcode.wrapping_sub(1) as usize)?;
    Some(handler(c).err().unwrap_or(Status::Ok))
}
