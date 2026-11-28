//! Declarative Firmware RigWire v2 Service & Component Dispatcher.
//!
//! Bridges RigWire v2 serial transport with microkernel services, radio control,
//! display mirroring, structured table sector-hash delta sync, and streaming KernFS
//! transfers using a single-context zero-copy cursor (`WireCtx`) and `abi::Status`.

use crate::hardware::SystemHardware;
use crate::services::calibration::CALIB_FILE_PATH;
use abi::Status;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::service::SerialPort;
use kernel::wire::*;

/// Single-indirect KernFS v2.1 page limit: 12 L1 index pages × 128 pages × 256 B = 384 KiB.
const MAX_WIRE_FILE_SIZE: usize = 12 * 128 * 256;

/// Active transfer session state for streaming KernFS reads/writes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ActiveTransfer {
    pub active: bool,
    pub mode: u8,
    pub transfer_id: u16,
    pub chunk_size: u16,
    pub ino: u16,
    pub total_size: u32,
    pub current_len: u32,
    pub last_activity_ms: u32,
    pub path: utils::format::InlineStr<31>,
}

impl ActiveTransfer {
    pub const fn empty() -> Self {
        Self {
            active: false,
            mode: TRANSFER_MODE_READ,
            transfer_id: 0,
            chunk_size: 0,
            ino: 0,
            total_size: 0,
            current_len: 0,
            last_activity_ms: 0,
            path: utils::format::InlineStr::new(),
        }
    }

    #[inline(always)]
    pub fn path_str(&self) -> &str {
        self.path.as_str()
    }
}

pub static mut CURRENT_TRANSFER: ActiveTransfer = ActiveTransfer::empty();
static mut NEXT_TRANSFER_ID: u16 = 1;

#[inline(always)]
fn transfer_mut() -> &'static mut ActiveTransfer {
    unsafe { &mut *core::ptr::addr_of_mut!(CURRENT_TRANSFER) }
}

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
    out: &'a mut [u8; MAX_PAYLOAD_LEN],
    pos: usize,
    max_out: usize,
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
        let head = unsafe { core::slice::from_raw_parts(self.inp.as_ptr(), n) };
        self.inp =
            unsafe { core::slice::from_raw_parts(self.inp.as_ptr().add(n), self.inp.len() - n) };
        Ok(head)
    }

    #[inline(never)]
    fn rd_le(&mut self, n: usize) -> Result<u32, Status> {
        let s = self.take(n)?;
        let mut v = 0u32;
        let mut i = n;
        while i > 0 {
            i -= 1;
            v = (v << 8) | unsafe { *s.get_unchecked(i) } as u32;
        }
        Ok(v)
    }

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
        let n = data.len().min(self.max_out.saturating_sub(self.pos));
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), self.out.as_mut_ptr().add(self.pos), n);
        }
        self.pos += n;
    }

    #[inline(never)]
    fn zeroes(&mut self, len: usize) {
        let n = len.min(self.max_out.saturating_sub(self.pos));
        unsafe {
            core::ptr::write_bytes(self.out.as_mut_ptr().add(self.pos), 0, n);
        }
        self.pos += n;
    }

    #[inline(never)]
    fn wr_le(&mut self, mut v: u32, n: usize) {
        let count = n.min(self.max_out.saturating_sub(self.pos));
        let p = unsafe { self.out.as_mut_ptr().add(self.pos) };
        let mut i = 0usize;
        while i < count {
            unsafe { *p.add(i) = v as u8 };
            v >>= 8;
            i += 1;
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
    fn require_write_auth(&mut self) -> Result<(), Status> {
        let s = &self.hw().wire.session;
        if s.require_auth_writes && !s.is_authenticated {
            Err(Status::PermissionDenied)
        } else {
            Ok(())
        }
    }
}

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

#[inline(never)]
fn handle_core_service(
    op: u8,
    c: &mut WireCtx<'_, '_>,
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
            let tp = c.hw().wire.transport_profile;
            c.hw().wire.session.begin_session(epoch, mtu);
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
            c.pos =
                c.hw()
                    .wire
                    .session
                    .handle_auth_step(inp, py32_hal::delay::micros(), auth_out)?;
        }
        OP_CORE_PING => {
            let tok = c.rd_u32()?;
            c.wr_u32(tok);
            c.wr_u32(c.now);
        }
        OP_CORE_TIME_SYNC => {
            let tok = c.rd_u32()?;
            let us = py32_hal::delay::micros();
            c.wr_u32(tok);
            c.wr_u32(us);
            c.wr_u32(us);
            c.wr_u32(c.now);
        }
        OP_CORE_GET_TIME => {
            c.wr_u32(py32_hal::delay::micros());
            c.wr_u32(c.now);
        }
        OP_CORE_RUNLEVEL => {
            if !c.hw().wire.session.is_active() {
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
    // Handlers only read `hw.wire.session` and never mutate `rx_buf`; response payload is written
    // directly at `wire_buf[WIRE_PAYLOAD_OFFSET..]` for zero-copy in-place sealing and COBS framing.
    let inp = unsafe { core::slice::from_raw_parts(raw.as_ptr(), plen) };
    let mut wire_uninit = core::mem::MaybeUninit::<[u8; MAX_WIRE_BUF_LEN]>::uninit();
    let wire_buf = unsafe { &mut *wire_uninit.as_mut_ptr() };
    let out = unsafe {
        &mut *(wire_buf.as_mut_ptr().add(WIRE_PAYLOAD_OFFSET) as *mut [u8; MAX_PAYLOAD_LEN])
    };
    let mut reboot_mode = 0xFFu8;

    let res = {
        let mut c = WireCtx {
            inp,
            out,
            pos: 0,
            max_out,
            now,
            #[cfg(test)]
            hw,
            #[cfg(not(test))]
            _b: core::marker::PhantomData,
        };

        let status_opt = match header.channel {
            CH_CORE => Some(
                handle_core_service(header.op, &mut c, &mut reboot_mode)
                    .err()
                    .unwrap_or(Status::Ok),
            ),
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

#[inline(always)]
fn config_domain_idx(instance: u8) -> Result<usize, Status> {
    if instance > 5 {
        Err(Status::InvalidArg)
    } else {
        Ok(instance as usize)
    }
}

fn read_sys_facet(facet: u16, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    match facet {
        FACET_SYS_IDENTITY => {
            let mut mac = [0u8; 6];
            let mut cf = [0u8; 15];
            let serial = bsp::uid::fill_device_identity(&mut mac, &mut cf);
            c.wr_u32(serial as u32);
            c.wr_u32((serial >> 32) as u32);
            c.put(&mac);
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
            use crate::runtime::syscall::service::live_prop_get;
            use ::abi::prop;
            let hw = c.hw();
            let app_id = hw.orchestrator.active_app_id() as u8;
            let locked = hw.orchestrator.is_keypad_locked() as u8;
            let bk_gpio = hw.radio.read_gpio_reg();
            let status_led = if bk_gpio == 0xFFFF {
                0
            } else {
                (bk_gpio & 3) as u8
            };
            let disp = &crate::master_config_ref().display;
            c.wr_u32(c.now);
            c.wr_u32(live_prop_get(prop::CPU_HZ) as u32);
            c.wr_u16(live_prop_get(prop::BATT_MV) as u16);
            c.wr_u8(live_prop_get(prop::BATT_PCT) as u8);
            c.wr_u8(app_id);
            c.wr_u8(locked);
            c.wr_u8(live_prop_get(prop::FLASHLIGHT) as u8);
            c.wr_u8(status_led);
            c.wr_u16(bk_gpio);
            c.wr_u8(crate::system::is_headless() as u8);
            c.wr_u8(drivers::backlight::BACKLIGHT_DUTY.load(core::sync::atomic::Ordering::Relaxed));
            c.wr_u8(disp.contrast);
            c.wr_u8(disp.inverted);
        }
        FACET_SYS_CALIBRATION => {
            let calib = crate::calibration_mut();
            c.wr_u8((calib.is_valid() && calib.is_device_bound()) as u8);
            c.wr_u16(calib.xtal_freq_low() as u16);
            c.wr_u16(calib.battery_calib_const());
        }
        FACET_SYS_AUDIO => {
            let rem = c.max_out.saturating_sub(c.pos).min(64);
            let n = crate::system::drain_headless_notes(&mut c.out[c.pos..c.pos + rem]);
            c.pos += n;
        }
        _ => return Err(Status::Unsupported),
    }
    Ok(())
}

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
            let modal = crate::orchestrator::compositor::active_modal_mut();
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
fn handle_resource_set(handle: ResourceHandle, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
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
                        coord.select_vfo(&mut c.hw().radio, slot);
                    } else {
                        let freq_hz = c.rd_u32()?;
                        if !(18_000_000..=1_300_000_000).contains(&freq_hz) {
                            return Err(Status::InvalidArg);
                        }
                        coord.tune_slot(&mut c.hw().radio, slot, freq_hz, None, None);
                    }
                }
                FACET_RADIO_MODULATION | FACET_RADIO_SQUELCH => {
                    let v = c.rd_u8()?;
                    let param = (handle.facet - FACET_RADIO_MODULATION) as u8;
                    coord.set_slot_param(&mut c.hw().radio, slot, param, v);
                }
                FACET_RADIO_TRANSMIT => {
                    let tx = c.rd_u8()? != 0;
                    let radio = &mut c.hw().radio;
                    if tx {
                        let token = kernel::service::radio::ClientToken::new(
                            kernel::service::radio::ClientId::SYSTEM,
                            kernel::service::radio::LockTier::FOREGROUND,
                        );
                        if !coord.request_ptt_start(radio, token).is_granted() {
                            return Err(Status::SafetyBlocked);
                        }
                    } else {
                        coord.request_ptt_stop(radio);
                    }
                    crate::system::set_wire_tx(coord.is_transmitting);
                }
                FACET_RADIO_DIGITAL => {
                    let offset_dhz = c.rd_u16()? as i16;
                    coord.set_vfo_sub_hz_offset(&mut c.hw().radio, slot, offset_dhz);
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
            c.hw().wire.port.flush();
            kernel::power::PowerControl::reboot_with(kernel::power::RebootMode::from_u8(arg));
        }
        (DOMAIN_SYSTEM, ACTION_SYS_FLASHLIGHT | ACTION_SYS_HEADLESS) => {
            let mode = c.inp.first().copied().unwrap_or(2);
            if action_id == ACTION_SYS_FLASHLIGHT {
                let _ = crate::runtime::syscall::service::live_prop_set(
                    ::abi::prop::FLASHLIGHT,
                    mode as usize,
                );
            } else {
                let enable = match mode {
                    0 => false,
                    1 => true,
                    _ => !crate::system::is_headless(),
                };
                crate::system::set_headless(c.hw(), enable);
                c.wr_u8(enable as u8);
            }
        }
        (DOMAIN_RADIO, ACTION_RADIO_VFO_TOGGLE) => {
            crate::radio_state().coord.toggle_vfo(&mut c.hw().radio);
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
        (DOMAIN_INPUT, ACTION_INPUT_LOCK_TOGGLE) => {
            c.hw().orchestrator.toggle_keypad_lock();
        }
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
fn handle_ui_stream(handle: ResourceHandle, c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
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
    let max_linear = c.max_out.saturating_sub(10).min(248);
    let len = (length as usize).min(max_linear).min(flat.len() - off);
    c.wr_u32(off as u32);
    c.wr_u16(len as u16);
    c.put(&flat[off..off + len]);
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
        OP_HANDLE_SET => c
            .require_write_auth()
            .and_then(|()| handle_resource_set(handle, c)),
        OP_HANDLE_INVOKE => c
            .require_write_auth()
            .and_then(|()| handle_resource_invoke(handle, c)),
        OP_HANDLE_STREAM => handle_ui_stream(handle, c),
        _ => return None,
    };
    Some(res.err().unwrap_or(Status::Ok))
}

// ── CH_DATA (0x03): Hardened KernFS Streaming & Sector Hash Sync ────────────

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

fn stream_file_crc32(fs: &mut crate::services::SystemStorage, path: &str, size: usize) -> u32 {
    let Ok(ino) = fs.lookup_ino(path) else {
        return 0;
    };
    let mut crc_val = 0xFFFF_FFFFu32;
    let mut cur_off = 0usize;
    let buf = &mut crate::data::arena::staging_buf()[..256];
    while cur_off < size {
        let to_read = (size - cur_off).min(buf.len());
        match fs.read_ino_at(ino, cur_off, &mut buf[..to_read]) {
            Ok(n) if n > 0 => {
                crc_val = crypto::crc::CrcEngine::crc32_update(crc_val, &buf[..n]);
                cur_off += n;
            }
            _ => break,
        }
    }
    !crc_val
}

#[inline(always)]
fn is_calib_path(path: &str) -> bool {
    path == CALIB_FILE_PATH
}

fn on_transfer_open(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let mode = c.rd_u8()?;
    let total_size = c.rd_u32()?;
    let req_chunk = c.rd_u16()?;
    let path = c.rd_path()?;
    if mode > TRANSFER_MODE_WRITE {
        return Err(Status::InvalidArg);
    }
    let is_write = mode == TRANSFER_MODE_WRITE;
    if is_write {
        c.require_write_auth()?;
    }
    let existing_size = check_regular_file(c.fs(), path, is_write, is_write)?;
    let open_ino = if is_write {
        let req_sz = total_size as usize;
        if req_sz > MAX_WIRE_FILE_SIZE {
            return Err(Status::BadLength);
        }
        let is_calib = is_calib_path(path);
        if is_calib {
            if !c.hw().wire.session.is_authenticated {
                return Err(Status::PermissionDenied);
            }
            if req_sz != 512 {
                return Err(Status::SafetyBlocked);
            }
        }
        if req_sz > c.fs().stats().avail_bytes() + existing_size as usize {
            return Err(Status::NoSpace);
        }
        if !is_calib && c.fs().write_file(path, &[]).is_err() {
            return Err(Status::IoError);
        }
        0
    } else {
        c.fs().lookup_ino(path).map_err(|_| Status::NotFound)?
    };

    let tid = unsafe {
        let p = core::ptr::addr_of_mut!(NEXT_TRANSFER_ID);
        let id = *p;
        *p = id.wrapping_add(1).max(1);
        id
    };
    let max_chunk = c.max_out.saturating_sub(8).min(256) as u16;
    let chunk_size = req_chunk.clamp(16, max_chunk);
    let t = transfer_mut();
    unsafe { core::ptr::write_bytes(t, 0, 1) };
    t.active = true;
    t.mode = mode;
    t.transfer_id = tid;
    t.chunk_size = chunk_size;
    t.ino = open_ino;
    t.total_size = total_size;
    t.last_activity_ms = c.now;
    t.path.set(path);

    c.wr_u16(tid);
    c.wr_u16(chunk_size);
    c.wr_u32(existing_size);
    Ok(())
}

fn on_transfer_chunk(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let tid = c.rd_u16()?;
    let off = c.rd_u32()? as usize;
    let length = c.rd_u16()?;
    let data = c.inp;
    let transfer = active_transfer_mut(tid)?;
    transfer.last_activity_ms = c.now;

    if transfer.mode == TRANSFER_MODE_WRITE {
        let chunk_len = data.len();
        let end = off.checked_add(chunk_len).ok_or(Status::InvalidArg)?;
        if end > MAX_WIRE_FILE_SIZE {
            return Err(Status::BadLength);
        }
        if is_calib_path(transfer.path_str()) {
            let staging = crate::data::arena::staging_buf();
            if end > staging.len() {
                return Err(Status::BadLength);
            }
            staging[off..end].copy_from_slice(data);
        } else {
            c.fs()
                .write_at(transfer.path_str(), off, data)
                .map_err(|_| Status::IoError)?;
        }
        let new_len = end as u32;
        transfer.current_len = transfer.current_len.max(new_len);
        c.wr_u16(tid);
        c.wr_u16(chunk_len as u16);
        c.wr_u32(new_len);
    } else {
        let max_read = c.max_out.saturating_sub(8).min(256);
        let len = (length as usize).min(max_read);
        let read_sz = {
            let out_ptr = c.out.as_mut_ptr();
            let dst = unsafe { core::slice::from_raw_parts_mut(out_ptr.add(8), len) };
            c.fs().read_ino_at(transfer.ino, off, dst).unwrap_or(0)
        };
        c.wr_u16(tid);
        c.wr_u32(off as u32);
        c.wr_u16(read_sz as u16);
        c.pos = 8 + read_sz;
    }
    Ok(())
}

fn on_transfer_close(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let tid = c.rd_u16()?;
    let expected_crc = c.rd_u32()?;
    let transfer = active_transfer_mut(tid)?;
    transfer.last_activity_ms = c.now;

    transfer.active = false;
    let mut actual_crc = 0u32;
    if transfer.mode == TRANSFER_MODE_WRITE {
        let total_len = transfer.current_len as usize;
        if is_calib_path(transfer.path_str()) {
            let staging = crate::data::arena::staging_buf();
            if total_len != 512 {
                return Err(Status::SafetyBlocked);
            }
            let calib = crate::calibration_mut();
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

fn on_transfer_abort(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let tid = c.rd_u16()?;
    let _reason = c.rd_u8()?;
    if let Ok(transfer) = active_transfer_mut(tid) {
        transfer.active = false;
        let path = transfer.path_str();
        if transfer.mode == TRANSFER_MODE_WRITE && !is_calib_path(path) {
            let _ = c.fs().remove(path);
            invalidate_apps_if_needed(path);
        }
    }
    c.wr_u16(tid);
    Ok(())
}

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

    let max_pos = c.max_out;
    let _ = c.fs().for_each_entry(path, |name, size, _perm, is_dir| {
        if next_cursor == 0xFFFF {
            if idx >= start {
                let b = name.as_bytes();
                let name_len = b.len().min(24);
                if pos + 6 + name_len > max_pos {
                    next_cursor = idx;
                    return;
                }
                unsafe {
                    let p = out_ptr.add(pos);
                    *p = is_dir as u8;
                    core::ptr::copy_nonoverlapping(
                        (size as u32).to_le_bytes().as_ptr(),
                        p.add(1),
                        4,
                    );
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

fn on_transfer_remove(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    c.require_write_auth()?;
    let path = c.rd_path()?;
    check_regular_file(c.fs(), path, true, false)?;
    c.fs().remove(path).map_err(|_| Status::NotFound)?;
    invalidate_apps_if_needed(path);
    Ok(())
}

/// Paginated IEEE CRC-32 digest per 256-byte page slice for O(changed pages) delta sync.
/// Request: `[start_page: u16, path_len: u8, path...]`
/// Reply:   `[total_pages: u16, count: u8, hashes: [u32; count]]` (up to 64 pages/frame).
fn on_transfer_sector_hash(c: &mut WireCtx<'_, '_>) -> Result<(), Status> {
    let start_page = c.rd_u16()? as usize;
    let path = c.rd_path()?;
    let sz = check_regular_file(c.fs(), path, false, false)? as usize;
    let ino = c.fs().lookup_ino(path).map_err(|_| Status::NotFound)?;
    let total_pages = (sz + 255) >> 8;
    let max_hashes = (c.max_out.saturating_sub(3) >> 2).min(64);
    let count = total_pages.saturating_sub(start_page).min(max_hashes);
    c.wr_u16(total_pages as u16);
    c.wr_u8(count as u8);
    let buf = &mut crate::data::arena::staging_buf()[..256];
    for page in start_page..start_page + count {
        let n = c.fs().read_ino_at(ino, page << 8, buf).unwrap_or(0);
        c.wr_u32(crypto::crc::CrcEngine::crc32(&buf[..n]));
    }
    Ok(())
}

#[inline(never)]
fn handle_data_service(opcode: u8, c: &mut WireCtx<'_, '_>) -> Option<Status> {
    let res = match opcode {
        1 => on_transfer_open(c),
        2 => on_transfer_chunk(c),
        3 => on_transfer_close(c),
        4 => on_transfer_abort(c),
        5 => on_transfer_stat(c),
        6 => on_transfer_list(c),
        7 => on_transfer_remove(c),
        8 => on_transfer_sector_hash(c),
        _ => return None,
    };
    Some(res.err().unwrap_or(Status::Ok))
}
