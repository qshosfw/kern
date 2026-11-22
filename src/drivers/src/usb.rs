//! PY32F071 Native USB 2.0 Full-Speed Device Driver.
//!
//! Provides hardware-accelerated connection detection, bus lifecycle monitoring
//! (Reset, SOF, Resume, Suspend), EP0 Control request handling, and granularly
//! feature-gated CDC ACM Virtual COM Port and Mass Storage Class (MSC) support.

pub mod descriptors;

#[cfg(feature = "usb_msc_fat")]
pub mod fat;

#[cfg(feature = "usb_msc")]
pub mod msc;

#[cfg(feature = "usb_cdc")]
pub mod cdc;

use self::descriptors::*;
pub use kernel::event::UsbEvent;
use py32_hal::pac::usbd::*;

#[cfg(feature = "usb_msc")]
use self::msc::MscEngine;

#[cfg(feature = "usb_cdc")]
use self::cdc::CdcEngine;

/// Maximum packet size for Full-Speed Control Endpoint (EP0)
pub const USB_EP0_MAX_PACKET_SIZE: usize = 64;

const IRQ_FLAG_ACTIVITY: u8 = 1 << 0;
const IRQ_FLAG_SUSPEND: u8 = 1 << 1;

/// Runaway bus-reset storm threshold: 6 resets within 500 ms indicates severe RF port flapping
/// that can trigger Linux `xhci_hcd` / `cdc_acm` teardown races. Normal TX never hits this.
const RESET_STORM_WINDOW_MS: u32 = 500;
const RESET_STORM_THRESHOLD: u8 = 6;
const RESET_STORM_BACKOFF_MS: u32 = 180;

/// SOF silence disconnect timeouts (ms). During active RF TX we hold the link open longer
/// (1500 ms) and ignore transient suspend glitches so attached docks never drop on brief RF bursts.
const SOF_DISCONNECT_SUSPEND_MS: u32 = 150;
const SOF_DISCONNECT_IDLE_MS: u32 = 300;
const SOF_DISCONNECT_TX_GRACE_MS: u32 = 1_500;

/// Bounded wall-clock wait (ms) for an IN FIFO slot in `cdc_write()` before aborting cleanly.
const CDC_WRITE_TIMEOUT_MS: u32 = 12;

#[inline(always)]
fn usb_critical<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(all(not(test), target_arch = "arm"))]
    {
        cortex_m::interrupt::free(|_| f())
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    {
        f()
    }
}

/// Pure reset-storm detector: tracks bus resets inside a rolling window and returns `true`
/// only when rapid RF-induced port flapping crosses `RESET_STORM_THRESHOLD`.
#[inline(always)]
pub(crate) fn record_bus_reset(window_start_ms: &mut u32, count: &mut u8, now_ms: u32) -> bool {
    if *count == 0 || now_ms.wrapping_sub(*window_start_ms) >= RESET_STORM_WINDOW_MS {
        *window_start_ms = now_ms;
        *count = 1;
        false
    } else {
        *count = count.saturating_add(1);
        *count >= RESET_STORM_THRESHOLD
    }
}

/// Pure SOF-timeout disconnect policy with extended grace during active RF transmission.
#[inline(always)]
pub(crate) const fn should_disconnect(
    elapsed_since_sof_ms: u32,
    has_suspend: bool,
    rf_tx_active: bool,
) -> bool {
    if rf_tx_active {
        elapsed_since_sof_ms >= SOF_DISCONNECT_TX_GRACE_MS
    } else {
        (has_suspend && elapsed_since_sof_ms >= SOF_DISCONNECT_SUSPEND_MS)
            || elapsed_since_sof_ms >= SOF_DISCONNECT_IDLE_MS
    }
}

/// Returns `true` when an EP0 IN response needs a trailing Zero-Length Packet per USB 2.0 §8.5.3.2
/// (transfer length is a non-zero multiple of 64 bytes and strictly shorter than host `wLength`).
#[inline(always)]
pub(crate) const fn needs_ep0_zlp(total_len: usize, host_max_len: u16) -> bool {
    total_len > 0
        && (total_len & (USB_EP0_MAX_PACKET_SIZE - 1)) == 0
        && total_len < host_max_len as usize
}

/// Hardware USB Device Controller driver for PY32F071
#[repr(C)]
pub struct UsbDriver {
    // Pack ALL 1-byte state flags at offset 0..16 so every Thumb-1 ldrb/strb uses 2-byte imm5.
    is_connected: bool,
    was_connected: bool,
    consecutive_sof: u8,
    is_initialized: bool,
    dev_addr: u8,
    pending_addr: u8,
    configured: bool,
    is_charging: bool,

    #[cfg(feature = "usb_cdc")]
    pending_set_line_coding: bool,
    ep0_need_zlp: bool,
    irq_flags: u8,
    reset_burst_count: u8,
    rf_tx_active: bool,
    product_len: u8,
    battery_pct: u8,

    // 2-byte fields within ldrh/strh imm5*2 (<= 62 B)
    last_frame: u16,
    battery_voltage_mv: u16,
    ep0_rem: u16,

    // 4-byte fields within ldr/str imm5*4 (<= 124 B)
    last_sof_ms: u32,
    last_transition_ms: u32,
    reset_window_start_ms: u32,
    backoff_until_ms: u32,
    ep0_ptr: *const u8,

    #[cfg(feature = "usb_cdc")]
    cdc: CdcEngine,

    product_name: [u8; 32],

    #[cfg(feature = "usb_msc")]
    msc: MscEngine,
}

#[inline(always)]
fn usb_reg(off: usize) -> *mut u8 {
    unsafe { (USBD_BASE as *mut u8).add(off) }
}

#[inline(always)]
fn usb_rd(off: usize) -> u8 {
    unsafe { usb_reg(off).read_volatile() }
}

#[inline(always)]
fn usb_wr(off: usize, val: u8) {
    unsafe { usb_reg(off).write_volatile(val) }
}

#[inline(always)]
fn usb_rd16(off: usize) -> u16 {
    unsafe { (usb_reg(off) as *const u16).read_volatile() }
}

impl UsbDriver {
    /// Create a new, uninitialized USB driver instance (const constructible).
    pub const fn new() -> Self {
        Self {
            last_sof_ms: 0,
            last_transition_ms: 0,
            reset_window_start_ms: 0,
            backoff_until_ms: 0,
            ep0_ptr: core::ptr::null(),
            ep0_rem: 0,
            battery_voltage_mv: 0,
            last_frame: 0,
            product_len: bsp::pins::BOARD_SPEC.usb_product_default.len() as u8,
            battery_pct: 0,
            product_name: if cfg!(feature = "target_k10pro") {
                *b"Quansheng UV-K10 Pro            "
            } else {
                *b"Quansheng UV-K1                 "
            },
            is_connected: false,
            was_connected: false,
            consecutive_sof: 0,
            is_initialized: false,
            dev_addr: 0,
            pending_addr: 0,
            configured: false,
            is_charging: false,

            #[cfg(feature = "usb_cdc")]
            pending_set_line_coding: false,
            ep0_need_zlp: false,
            irq_flags: 0,
            reset_burst_count: 0,
            rf_tx_active: false,

            #[cfg(feature = "usb_cdc")]
            cdc: CdcEngine::new(),

            #[cfg(feature = "usb_msc")]
            msc: MscEngine::new(),
        }
    }

    /// Update configured device identity (e.g. "Quansheng UV-K1 1D9E") for USB product descriptor.
    pub fn update_device_identity(&mut self, model: &str, tag: &str) {
        let m_bytes = utils::format::trim_ascii_bytes(model.as_bytes());
        let t_bytes = utils::format::trim_ascii_bytes(tag.as_bytes());
        let m_len = m_bytes.len().min(12);
        let t_len = t_bytes.len().min(8);

        const PREFIX: &[u8] = b"Quansheng ";
        let dst = self.product_name.as_mut_ptr();
        unsafe {
            core::ptr::copy_nonoverlapping(PREFIX.as_ptr(), dst, PREFIX.len());
            let mut pos = PREFIX.len();
            core::ptr::copy_nonoverlapping(m_bytes.as_ptr(), dst.add(pos), m_len);
            pos += m_len;
            if t_len > 0 {
                *dst.add(pos) = b' ';
                pos += 1;
                core::ptr::copy_nonoverlapping(t_bytes.as_ptr(), dst.add(pos), t_len);
                pos += t_len;
            }
            self.product_len = pos as u8;
        }
    }

    /// Inform the USB driver whether the RF PA is actively transmitting so transient RF-induced
    /// SOF dropouts extend disconnect grace instead of prematurely dropping attached docks.
    #[inline(always)]
    pub fn set_rf_tx_active(&mut self, active: bool) {
        self.rf_tx_active = active;
    }

    #[inline(never)]
    fn set_usb_nvic(enable: bool) {
        #[cfg(all(not(test), target_arch = "arm"))]
        unsafe {
            let irq = py32_pac::Interrupt::USB as u8;
            py32_pac::nvic_clear_pending(irq);
            if enable {
                py32_pac::nvic_enable(irq);
            } else {
                py32_pac::nvic_disable(irq);
            }
        }
        #[cfg(not(all(not(test), target_arch = "arm")))]
        let _ = enable;
    }

    /// Truly detach the PY32F071 USB PHY and reset link state before deep STOP sleep.
    #[inline(never)]
    pub fn disconnect_for_sleep(&mut self) {
        Self::set_usb_nvic(false);
        py32_hal::rcc::disable_usbd();
        self.is_connected = false;
        self.configured = false;
        self.consecutive_sof = 0;
        self.ep0_rem = 0;
        self.ep0_need_zlp = false;
        self.backoff_until_ms = 0;
        self.reset_burst_count = 0;
        self.irq_flags = 0;
        self.last_frame = 0;
    }

    /// Update live power and battery telemetry for standard USB descriptors and CAT queries.
    pub fn update_power_telemetry(&mut self, voltage_mv: u16, pct: u8, charging: bool) {
        self.battery_voltage_mv = voltage_mv;
        self.battery_pct = pct;
        self.is_charging = charging;
    }

    /// Initialize the USBD peripheral clock, CTC SOF auto-trim, and arm interrupt-driven USB.
    #[inline(never)]
    pub fn init(&mut self) {
        Self::set_usb_nvic(false);

        if py32_hal::rcc::sysclk_hz() != 48_000_000 {
            py32_hal::rcc::set_sysclk_48mhz();
        }
        py32_hal::rcc::enable_usbd();

        usb_wr(0x0E, 0);
        self.reset_bus_state();
        usb_wr(0x08, USB_INTR_RESET | USB_INTR_RESUME | USB_INTR_SUSPEND);
        let _ = (usb_rd(0x04), usb_rd(0x05), usb_rd(0x06));

        self.last_sof_ms = 0;
        self.last_frame = usb_rd16(0x0C) & 0x07FF;
        self.irq_flags = 0;
        self.backoff_until_ms = 0;
        self.reset_burst_count = 0;
        self.is_initialized = true;
        Self::set_usb_nvic(true);
    }

    #[inline(never)]
    fn reset_bus_state(&mut self) {
        self.dev_addr = 0;
        self.pending_addr = 0;
        self.ep0_rem = 0;
        self.ep0_need_zlp = false;
        self.configured = false;
        usb_wr(0x00, 0);
        usb_wr(0x01, USB_POWER_SUSPENDENB);
        usb_wr(0x0A, 1); // INT_IN1E: EP0 only
        usb_wr(0x09, 0); // INT_OUT1E: none
    }

    /// Interrupt handler (`IRQ 31`): services USB bus resets, EP0 control stages (meeting the
    /// 2 ms `SET_ADDRESS` window even during LCD blits or PA ramps), and CDC Bulk endpoints
    /// with RF packet-error flushing and hardware NAK flow control.
    #[inline(never)]
    pub fn handle_irq(&mut self) {
        if !self.is_initialized {
            return;
        }

        let int_usb = usb_rd(0x04);
        let int_out = usb_rd(0x05);
        let int_in = usb_rd(0x06);
        let saved_ep = usb_rd(0x0E) & 0x07;

        if (int_usb & USB_INTR_RESET) != 0 {
            self.consecutive_sof = 0;
            self.reset_bus_state();
            let now = py32_hal::delay::millis();
            let _ = record_bus_reset(
                &mut self.reset_window_start_ms,
                &mut self.reset_burst_count,
                now,
            );
            self.irq_flags |= IRQ_FLAG_ACTIVITY;

            #[cfg(feature = "usb_cdc")]
            {
                self.pending_set_line_coding = false;
                self.cdc.reset();
                usb_wr(0x0E, descriptors::CDC_EP_IN & 0x07);
                usb_wr(0x15, INCSR1_FLUSHFIFO | INCSR1_CLRDT);
                usb_wr(0x0E, descriptors::CDC_EP_OUT & 0x07);
                usb_wr(0x19, OUTCSR1_FLUSHFIFO | OUTCSR1_CLRDT);
            }
        }

        if (int_usb & USB_INTR_RESUME) != 0 {
            self.irq_flags |= IRQ_FLAG_ACTIVITY;
        }
        if (int_usb & USB_INTR_SUSPEND) != 0 {
            self.irq_flags |= IRQ_FLAG_SUSPEND;
        }

        if (int_in & 0x01) != 0 {
            self.poll_ep0();
        }

        #[cfg(feature = "usb_cdc")]
        if self.configured {
            let in_ep = descriptors::CDC_EP_IN & 0x07;
            if (int_in & (1 << in_ep)) != 0 {
                usb_wr(0x0E, in_ep);
                let csr = usb_rd(0x15);
                if (csr & (INCSR1_UNDERRUN | INCSR1_SENTSTALL)) != 0 {
                    usb_wr(0x15, csr & !(INCSR1_UNDERRUN | INCSR1_SENTSTALL));
                }
                if (csr & INCSR1_INPKTRDY) == 0 {
                    self.cdc.handle_in_complete();
                }
            }

            let out_ep = descriptors::CDC_EP_OUT & 0x07;
            if (int_out & (1 << out_ep)) != 0 {
                self.drain_cdc_out_ep(out_ep);
            }
        }

        #[cfg(not(feature = "usb_cdc"))]
        let _ = int_out;

        usb_wr(0x0E, saved_ep);
    }

    #[cfg(feature = "usb_cdc")]
    #[inline(never)]
    fn drain_cdc_out_ep(&mut self, out_ep: u8) {
        usb_wr(0x0E, out_ep);
        let out_csr = usb_rd(0x19);
        if (out_csr & (OUTCSR1_DATAPACKETERROR | OUTCSR1_OVERRUN)) != 0 {
            usb_wr(0x19, OUTCSR1_FLUSHFIFO);
            return;
        }
        if (out_csr & OUTCSR1_OUTPKTRDY) != 0 {
            let count = (usb_rd16(0x1C) as usize).min(64);
            if count > self.cdc.rx_free() {
                usb_wr(0x09, usb_rd(0x09) & !(1 << out_ep));
                return;
            }
            if count > 0 {
                self.cdc.push_rx_from_fifo(out_ep, count);
                py32_hal::uart::mark_activity();
            }
            usb_wr(0x19, 0);
        }
    }

    /// Poll bus lifecycle state (SOF frame counter, connect/disconnect transitions, runaway
    /// reset-storm backoff, and optional MSC endpoints). Returns `Some(UsbEvent)` on transitions.
    #[inline(never)]
    pub fn poll(&mut self) -> Option<UsbEvent> {
        if !self.is_initialized {
            return None;
        }

        let now = py32_hal::delay::millis();

        if self.backoff_until_ms != 0 {
            if now.wrapping_sub(self.backoff_until_ms) < 0x8000_0000 {
                self.init();
            }
            return None;
        }

        let (flags, burst, frame) = usb_critical(|| {
            let f = self.irq_flags;
            self.irq_flags = 0;
            (f, self.reset_burst_count, usb_rd16(0x0C) & 0x07FF)
        });

        if burst >= RESET_STORM_THRESHOLD {
            let was_conn = self.is_connected;
            self.disconnect_for_sleep();
            self.was_connected = was_conn;
            self.backoff_until_ms = now.wrapping_add(RESET_STORM_BACKOFF_MS).max(1);
            self.last_transition_ms = now;
            return was_conn.then_some(UsbEvent::Disconnected);
        }

        let sof_advanced = frame != self.last_frame;
        if ((flags & IRQ_FLAG_ACTIVITY) != 0 || sof_advanced)
            && py32_hal::rcc::sysclk_hz() != 48_000_000
        {
            py32_hal::rcc::set_sysclk_48mhz();
        }

        let mut event = None;

        if sof_advanced {
            self.last_frame = frame;
            self.last_sof_ms = now;
            self.consecutive_sof = self.consecutive_sof.saturating_add(1);

            if self.consecutive_sof >= 5
                && !self.is_connected
                && now.wrapping_sub(self.last_transition_ms) >= 50
            {
                self.was_connected = false;
                self.is_connected = true;
                self.last_transition_ms = now;
                event = Some(if self.configured {
                    UsbEvent::Resumed
                } else {
                    UsbEvent::Connected
                });
            }
        }

        if self.is_connected
            && should_disconnect(
                now.wrapping_sub(self.last_sof_ms),
                (flags & IRQ_FLAG_SUSPEND) != 0,
                self.rf_tx_active,
            )
        {
            self.was_connected = true;
            self.is_connected = false;
            self.configured = false;
            self.consecutive_sof = 0;
            self.last_transition_ms = now;
            if event.is_none() {
                event = Some(UsbEvent::Disconnected);
            }
        }

        #[cfg(feature = "usb_msc")]
        usb_critical(|| self.poll_msc_endpoints());

        event
    }

    #[inline(never)]
    fn ep0_tx_next(&mut self) -> bool {
        if self.ep0_rem > 0 {
            let chunk = (self.ep0_rem as usize).min(USB_EP0_MAX_PACKET_SIZE);
            let slice = unsafe { core::slice::from_raw_parts(self.ep0_ptr, chunk) };
            write_fifo(0, slice);
            self.ep0_ptr = unsafe { self.ep0_ptr.add(chunk) };
            self.ep0_rem -= chunk as u16;
            usb_wr(
                0x10,
                if self.ep0_rem == 0 && !self.ep0_need_zlp {
                    EP0_TXPKTRDY | EP0_DATAEND
                } else {
                    EP0_TXPKTRDY
                },
            );
            true
        } else if self.ep0_need_zlp {
            self.ep0_need_zlp = false;
            usb_wr(0x10, EP0_TXPKTRDY | EP0_DATAEND);
            true
        } else {
            false
        }
    }

    #[inline(never)]
    fn poll_ep0(&mut self) {
        usb_wr(0x0E, 0);
        let csr = usb_rd(0x10);

        if (csr & EP0_SENTSTALL) != 0 {
            usb_wr(0x10, 0);
            self.ep0_rem = 0;
            self.ep0_need_zlp = false;
            return;
        }

        if (csr & EP0_SETUPEND) != 0 {
            usb_wr(0x10, EP0_SVSETUPEND);
            self.ep0_rem = 0;
            self.ep0_need_zlp = false;
            #[cfg(feature = "usb_cdc")]
            {
                self.pending_set_line_coding = false;
            }
        }

        // Apply pending device address immediately on Status Stage completion (< 2 ms USB spec)
        if self.pending_addr > 0 && (csr & (EP0_TXPKTRDY | EP0_RXPKTRDY)) == 0 {
            usb_wr(0x00, self.pending_addr);
            self.dev_addr = self.pending_addr;
            self.pending_addr = 0;
        }

        if (csr & EP0_TXPKTRDY) == 0 && self.ep0_tx_next() {
            return;
        }

        #[cfg(feature = "usb_cdc")]
        if self.pending_set_line_coding {
            if (csr & EP0_RXPKTRDY) != 0 {
                let count = (usb_rd(0x11) as usize).min(7);
                let mut coding = [0u8; 7];
                if count > 0 {
                    read_fifo(0, &mut coding[..count]);
                }
                if count == 7 {
                    self.cdc.set_line_coding(&coding);
                }
                usb_wr(0x10, EP0_SVRXPKTRDY | EP0_DATAEND);
                self.pending_set_line_coding = false;
            }
            return;
        }

        if (csr & EP0_RXPKTRDY) != 0 {
            self.last_sof_ms = py32_hal::delay::millis();
            self.irq_flags |= IRQ_FLAG_ACTIVITY;
            let count = usb_rd(0x11) as usize;
            if count == 8 {
                let mut setup = [0u8; 8];
                read_fifo(0, &mut setup);
                self.handle_setup_packet(&setup);
            } else {
                let fifo = fifo_ep_u8(0);
                for _ in 0..count.min(64) {
                    let _ = unsafe { fifo.read_volatile() };
                }
                usb_wr(0x10, EP0_SVRXPKTRDY);
            }
        }
    }

    #[inline(never)]
    fn handle_setup_packet(&mut self, setup: &[u8; 8]) {
        let bm_req_type = setup[0];
        let b_request = setup[1];
        let w_value = u16::from_le_bytes([setup[2], setup[3]]);
        let w_index = u16::from_le_bytes([setup[4], setup[5]]);
        let w_length = u16::from_le_bytes([setup[6], setup[7]]);

        let req_type = (bm_req_type >> 5) & 0x03;

        if req_type == 0 {
            // Standard USB Request
            match b_request {
                0x06 => self.handle_get_descriptor(w_value, w_length),
                0x05 => {
                    // SET_ADDRESS: do NOT write USBD_ADDR immediately;
                    // wait until status handshake is transmitted.
                    self.pending_addr = (w_value & 0x7F) as u8;
                    self.ep0_send_zlp();
                }
                0x09 => {
                    // SET_CONFIGURATION
                    self.configured = true;
                    self.configure_endpoints();
                    self.ep0_send_zlp();
                }
                0x00 | 0x0A => {
                    // GET_STATUS (2 zero bytes) / GET_INTERFACE (1 zero byte)
                    let len = if b_request == 0x00 { 2 } else { 1 };
                    self.ep0_start_transfer(&[0, 0][..len], w_length);
                }
                0x01 | 0x03 | 0x0B => {
                    // CLEAR_FEATURE / SET_FEATURE / SET_INTERFACE
                    self.ep0_send_zlp();
                }
                _ => self.ep0_stall(),
            }
        } else if req_type == 1 {
            // Class Request
            self.handle_class_request(bm_req_type, b_request, w_value, w_index, w_length);
        } else {
            self.ep0_stall();
        }
    }

    #[inline(never)]
    fn ep0_start_string_transfer(&mut self, ascii: &[u8], max_len: u16) {
        let len = ascii.len().min(31);
        let mut desc = core::mem::MaybeUninit::<[u8; 64]>::uninit();
        let desc = unsafe { &mut *desc.as_mut_ptr() };
        let total_bytes = 2 + len * 2;
        desc[0] = total_bytes as u8;
        desc[1] = DESC_TYPE_STRING;
        for i in 0..len {
            desc[2 + i * 2] = ascii[i];
            desc[2 + i * 2 + 1] = 0;
        }
        self.ep0_start_transfer(&desc[..total_bytes], max_len);
    }

    fn ep0_start_product_transfer(&mut self, max_len: u16) {
        let len = (self.product_len as usize).min(31);
        // SAFETY: `ep0_start_string_transfer` copies the bytes into its stack
        // descriptor buffer before mutating `self.ep0_transfer`; `product_name`
        // is untouched during the call.
        let s = unsafe { core::slice::from_raw_parts(self.product_name.as_ptr(), len) };
        self.ep0_start_string_transfer(s, max_len);
    }

    fn ep0_start_telemetry_transfer(&mut self, max_len: u16) {
        let mut uninit = core::mem::MaybeUninit::<[u8; 31]>::uninit();
        let buf = unsafe { &mut *uninit.as_mut_ptr() };
        let c_volts = ((self.battery_voltage_mv as u32 * 52429) >> 19) as i32;
        let suffix = if self.is_charging { "v [CHG]" } else { "v (" };
        let s = utils::format::format_int_fixed(c_volts, 2, "Supply: ", suffix, buf);
        let mut pos = s.len();
        if !self.is_charging {
            let pct = self.battery_pct.min(100) as i32;
            let p_str = utils::format::format_int_fixed(pct, 0, "", "%)", &mut buf[pos..]);
            pos += p_str.len();
        }
        self.ep0_start_string_transfer(&buf[..pos], max_len);
    }

    fn ep0_start_mac_transfer(&mut self, max_len: u16) {
        let mac = bsp::uid::read_mac_address();
        let mut uninit = core::mem::MaybeUninit::<[u8; 22]>::uninit();
        let buf = unsafe { &mut *uninit.as_mut_ptr() };
        buf[..5].copy_from_slice(b"MAC: ");
        let mac_dst = unsafe { &mut *(buf.as_mut_ptr().add(5) as *mut [u8; 17]) };
        let _ = utils::format::format_mac_address(&mac, mac_dst);
        self.ep0_start_string_transfer(buf, max_len);
    }

    fn handle_get_descriptor(&mut self, w_value: u16, w_length: u16) {
        let desc_type = (w_value >> 8) as u8;
        let desc_index = (w_value & 0xFF) as u8;

        let static_desc: Option<&[u8]> = match desc_type {
            DESC_TYPE_DEVICE => Some(&DEVICE_DESCRIPTOR),
            DESC_TYPE_CONFIGURATION => Some(&CONFIG_DESCRIPTOR),
            DESC_TYPE_STRING => match desc_index {
                0 => Some(&STRING_LANGID),
                1 => {
                    self.ep0_start_string_transfer(b"Quansheng", w_length);
                    return;
                }
                2 => {
                    self.ep0_start_product_transfer(w_length);
                    return;
                }
                3 => {
                    let crock = bsp::uid::get_crockford_serial();
                    self.ep0_start_string_transfer(&crock[..14], w_length);
                    return;
                }
                #[cfg(feature = "usb_cdc")]
                4 => {
                    self.ep0_start_string_transfer(b"Serial", w_length);
                    return;
                }
                5 => {
                    self.ep0_start_telemetry_transfer(w_length);
                    return;
                }
                6 => {
                    self.ep0_start_mac_transfer(w_length);
                    return;
                }
                #[cfg(feature = "usb_msc")]
                7 => Some(&STRING_INTERFACE_STORAGE),
                _ => None,
            },
            _ => None,
        };
        if let Some(d) = static_desc {
            self.ep0_start_transfer(d, w_length);
        } else {
            self.ep0_stall();
        }
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn handle_class_request(
        &mut self,
        req_type: u8,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
    ) {
        #[cfg(feature = "usb_msc")]
        if request == 0xFE {
            // GET_MAX_LUN
            let max_lun: [u8; 1] = [0];
            self.ep0_start_transfer(&max_lun, length);
            return;
        } else if request == 0xFF {
            // MSC_RESET
            self.ep0_send_zlp();
            return;
        }

        #[cfg(feature = "usb_cdc")]
        if request == cdc::CDC_REQ_SET_CONTROL_LINE_STATE {
            self.cdc.set_control_line_state(value);
            self.ep0_send_zlp();
            return;
        } else if request == cdc::CDC_REQ_GET_LINE_CODING {
            let mut coding = [0u8; 7];
            self.cdc.get_line_coding(&mut coding);
            self.ep0_start_transfer(&coding, length);
            return;
        } else if request == cdc::CDC_REQ_SET_LINE_CODING {
            // Acknowledge SETUP packet reception only (do NOT assert DATAEND yet);
            // wait for the 7-byte line coding data packet in the OUT data stage.
            Self::ep0_write_csr(EP0_SVRXPKTRDY);
            self.pending_set_line_coding = true;
            return;
        }

        self.ep0_stall();
    }

    #[inline(never)]
    fn configure_endpoints(&mut self) {
        #[cfg(feature = "usb_msc")]
        {
            let in_ep = descriptors::MSC_EP_IN & 0x07;
            let out_ep = descriptors::MSC_EP_OUT & 0x07;
            usb_wr(0x0E, in_ep);
            usb_wr(0x14, INCSR2_MODE);
            usb_wr(0x16, 8);
            usb_wr(0x0A, usb_rd(0x0A) | (1 << in_ep));
            usb_wr(0x0E, out_ep);
            usb_wr(0x1A, 8);
            usb_wr(0x09, usb_rd(0x09) | (1 << out_ep));
        }

        #[cfg(feature = "usb_cdc")]
        {
            let notif_ep = descriptors::CDC_EP_NOTIF & 0x07;
            let in_ep = descriptors::CDC_EP_IN & 0x07;
            let out_ep = descriptors::CDC_EP_OUT & 0x07;

            // CDC Notification Interrupt EP
            usb_wr(0x0E, notif_ep);
            usb_wr(0x14, INCSR2_MODE);
            usb_wr(0x16, 2); // 16 bytes

            // CDC Data IN Bulk
            usb_wr(0x0E, in_ep);
            usb_wr(0x15, INCSR1_CLRDT | INCSR1_FLUSHFIFO);
            usb_wr(0x14, INCSR2_MODE);
            usb_wr(0x16, 8); // 64 bytes
            usb_wr(0x0A, usb_rd(0x0A) | (1 << in_ep));

            // CDC Data OUT Bulk
            usb_wr(0x0E, out_ep);
            if in_ep != out_ep {
                usb_wr(0x14, 0);
            }
            usb_wr(0x19, OUTCSR1_CLRDT | OUTCSR1_FLUSHFIFO);
            usb_wr(0x1A, 8); // 64 bytes
            usb_wr(0x09, usb_rd(0x09) | (1 << out_ep));
        }
    }

    #[cfg(feature = "usb_msc")]
    fn poll_msc_endpoints(&mut self) {
        if !self.configured {
            return;
        }

        let out_ep = descriptors::MSC_EP_OUT & 0x7F;
        let in_ep = descriptors::MSC_EP_IN & 0x7F;

        // Check OUT (Host -> Radio commands / data)
        set_active_ep(out_ep);
        let out_csr = unsafe { USBD_OUT_CSR1_U8.read_volatile() };
        if (out_csr & OUTCSR1_OUTPKTRDY) != 0 {
            let count = unsafe { USBD_OUT_COUNT_U16.read_volatile() } as usize;
            self.msc.handle_out_packet(out_ep, count);
            // Clear OutPktRdy
            unsafe {
                USBD_OUT_CSR1_U8.write_volatile(0);
            }
        }

        // Check IN (Radio -> Host data completed)
        set_active_ep(in_ep);
        let in_csr = unsafe { USBD_IN_CSR1_U8.read_volatile() };
        if (in_csr & INCSR1_INPKTRDY) == 0 && self.msc.state() == msc::MscState::DataIn {
            self.msc.handle_in_packet();
        }
    }

    #[inline(never)]
    fn ep0_start_transfer(&mut self, data: &[u8], max_len: u16) {
        let total = data.len().min(max_len as usize);
        if total == 0 {
            self.ep0_rem = 0;
            self.ep0_need_zlp = false;
            self.ep0_send_zlp();
            return;
        }
        Self::ep0_write_csr(EP0_SVRXPKTRDY);
        self.ep0_ptr = data.as_ptr();
        self.ep0_rem = total as u16;
        self.ep0_need_zlp = needs_ep0_zlp(total, max_len);
        self.ep0_tx_next();
    }

    #[inline(never)]
    fn ep0_write_csr(csr: u8) {
        usb_wr(0x0E, 0);
        usb_wr(0x10, csr);
    }

    #[inline(always)]
    fn ep0_send_zlp(&mut self) {
        Self::ep0_write_csr(EP0_SVRXPKTRDY | EP0_DATAEND);
    }

    #[inline(always)]
    fn ep0_stall(&mut self) {
        self.ep0_rem = 0;
        self.ep0_need_zlp = false;
        Self::ep0_write_csr(EP0_SENDSTALL | EP0_SVRXPKTRDY);
    }

    #[inline(always)]
    pub fn is_connected(&self) -> bool {
        self.is_connected
    }

    #[inline(always)]
    pub fn consecutive_sof(&self) -> u8 {
        self.consecutive_sof
    }

    #[inline(always)]
    pub fn was_connected(&self) -> bool {
        self.was_connected
    }

    #[inline(always)]
    pub fn has_pending_transfer(&self) -> bool {
        #[cfg(feature = "usb_cdc")]
        let cdc_rx = self.cdc.rx_available() > 0;
        #[cfg(not(feature = "usb_cdc"))]
        let cdc_rx = false;
        self.ep0_rem > 0 || self.ep0_need_zlp || self.pending_addr > 0 || cdc_rx
    }

    #[inline(always)]
    pub fn is_dtr(&self) -> bool {
        #[cfg(feature = "usb_cdc")]
        {
            self.cdc.is_dtr()
        }
        #[cfg(not(feature = "usb_cdc"))]
        {
            false
        }
    }

    #[inline(always)]
    pub fn set_dtr(&mut self, _dtr: bool) {
        #[cfg(feature = "usb_cdc")]
        {
            self.cdc
                .set_control_line_state(if _dtr { 0x01 } else { 0x00 });
        }
    }

    #[inline(always)]
    pub fn frame_number(&self) -> u16 {
        unsafe { USBD_FRAME_U16.read_volatile() }
    }

    /// Whether the device is configured and active in USB Mass Storage Class mode.
    #[inline(always)]
    pub fn msc_active(&self) -> bool {
        #[cfg(feature = "usb_msc")]
        {
            self.configured
        }
        #[cfg(not(feature = "usb_msc"))]
        {
            false
        }
    }

    #[cfg(feature = "usb_cdc")]
    #[inline(never)]
    fn cdc_write_packet(&mut self, in_ep: u8, chunk: &[u8]) -> bool {
        let start = py32_hal::delay::millis();
        loop {
            let ready = usb_critical(|| {
                if !self.configured {
                    return 2u8;
                }
                let saved = usb_rd(0x0E) & 0x07;
                usb_wr(0x0E, in_ep);
                let csr = usb_rd(0x15);
                if (csr & (INCSR1_UNDERRUN | INCSR1_SENTSTALL)) != 0 {
                    usb_wr(0x15, csr & !(INCSR1_UNDERRUN | INCSR1_SENTSTALL));
                }
                let res = if (csr & INCSR1_INPKTRDY) == 0 {
                    if !chunk.is_empty() {
                        write_fifo(in_ep, chunk);
                    }
                    usb_wr(0x15, INCSR1_INPKTRDY);
                    1u8
                } else {
                    0u8
                };
                usb_wr(0x0E, saved);
                res
            });
            if ready == 1 {
                return true;
            }
            if ready == 2 || py32_hal::delay::millis().wrapping_sub(start) >= CDC_WRITE_TIMEOUT_MS {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    /// Transmit serial data chunk via USB CDC ACM (interrupt-safe, bounded timeout, RF-resilient).
    pub fn cdc_write(&mut self, _data: &[u8]) -> usize {
        #[cfg(feature = "usb_cdc")]
        {
            if !self.configured || _data.is_empty() {
                return 0;
            }
            py32_hal::uart::mark_activity();
            let in_ep = descriptors::CDC_EP_IN & 0x7F;
            let mut sent = 0;
            for chunk in _data.chunks(64) {
                if !self.cdc_write_packet(in_ep, chunk) {
                    return sent;
                }
                sent += chunk.len();
            }
            // USB Full-Speed Bulk IN requires a Zero-Length Packet when transfer length
            // is an exact multiple of 64 B so host CDC ACM flushes the frame immediately.
            if sent > 0 && (sent & 63) == 0 {
                let _ = self.cdc_write_packet(in_ep, &[]);
            }
            sent
        }
        #[cfg(not(feature = "usb_cdc"))]
        {
            let _ = _data;
            0
        }
    }

    /// Read incoming serial data chunk from USB CDC ACM and release NAK flow control if armed.
    #[inline(never)]
    pub fn cdc_read(&mut self, _buf: &mut [u8]) -> usize {
        #[cfg(feature = "usb_cdc")]
        {
            if !self.configured {
                return 0;
            }
            let read = usb_critical(|| {
                let n = self.cdc.pop_rx(_buf);
                let out_ep = descriptors::CDC_EP_OUT & 0x07;
                let mask = usb_rd(0x09);
                if (mask & (1 << out_ep)) == 0 && self.cdc.rx_free() >= 64 {
                    let saved = usb_rd(0x0E) & 0x07;
                    self.drain_cdc_out_ep(out_ep);
                    usb_wr(0x09, mask | (1 << out_ep));
                    usb_wr(0x0E, saved);
                }
                n
            });
            if read > 0 {
                py32_hal::uart::mark_activity();
            }
            read
        }
        #[cfg(not(feature = "usb_cdc"))]
        {
            let _ = _buf;
            0
        }
    }
}

impl kernel::service::UsbService for UsbDriver {
    #[inline(always)]
    fn is_connected(&self) -> bool {
        self.is_connected()
    }

    #[inline(always)]
    fn is_dtr(&self) -> bool {
        self.is_dtr()
    }

    #[inline(always)]
    fn poll_event(&mut self) -> Option<kernel::event::UsbEvent> {
        self.poll()
    }
}

/// Lightweight adapter wrapping `UsbDriver` to expose the `SerialPort` interface.
pub struct UsbCdcSerial<'a>(pub &'a mut UsbDriver);

impl<'a> kernel::service::SerialPort for UsbCdcSerial<'a> {
    fn write(&mut self, data: &[u8]) {
        let _ = self.0.cdc_write(data);
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        self.0.cdc_read(buf)
    }
}

static mut ACTIVE_USB_DRIVER: *mut UsbDriver = core::ptr::null_mut();

pub fn register_active_usb_driver(driver: *mut UsbDriver) {
    unsafe {
        ACTIVE_USB_DRIVER = driver;
    }
}

/// Dispatch hardware `IRQ 31` (`Interrupt::USB`) into the registered `UsbDriver`.
#[inline(always)]
pub fn on_usb_irq() {
    let ptr = unsafe { ACTIVE_USB_DRIVER };
    if !ptr.is_null() {
        unsafe {
            (*ptr).handle_irq();
        }
    }
}

pub fn active_usb_read(buf: &mut [u8]) -> usize {
    unsafe {
        if !ACTIVE_USB_DRIVER.is_null() {
            (*ACTIVE_USB_DRIVER).cdc_read(buf)
        } else {
            0
        }
    }
}

pub fn active_usb_write(data: &[u8]) -> usize {
    unsafe {
        if !ACTIVE_USB_DRIVER.is_null() {
            (*ACTIVE_USB_DRIVER).cdc_write(data)
        } else {
            0
        }
    }
}

fn active_usb_driver() -> Option<&'static UsbDriver> {
    let ptr = unsafe { ACTIVE_USB_DRIVER };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

pub fn is_active_usb_connected() -> bool {
    active_usb_driver().is_some_and(|d| d.is_connected())
}

pub fn is_active_usb_configured() -> bool {
    active_usb_driver().is_some_and(|d| d.configured)
}

pub fn is_active_usb_dtr() -> bool {
    active_usb_driver().is_some_and(|d| d.is_dtr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rf_tx_extends_disconnect_grace_without_dropping_docks() {
        // Idle mode: suspend at 150 ms or silence at 300 ms disconnects.
        assert!(!should_disconnect(149, true, false));
        assert!(should_disconnect(150, true, false));
        assert!(!should_disconnect(299, false, false));
        assert!(should_disconnect(300, false, false));

        // Active RF TX: spurious suspend glitches and short SOF dropouts do NOT drop the dock.
        assert!(!should_disconnect(150, true, true));
        assert!(!should_disconnect(500, true, true));
        assert!(!should_disconnect(1499, true, true));
        assert!(should_disconnect(1500, false, true));
    }

    #[test]
    fn reset_storm_detector_ignores_normal_resets_and_catches_flapping() {
        let mut win = 0u32;
        let mut count = 0u8;

        // Normal enumeration (2 resets) + occasional RF glitch (3 more) stays below threshold.
        for t in [100u32, 150, 220, 310, 400] {
            assert!(!record_bus_reset(&mut win, &mut count, t));
        }
        assert_eq!(count, 5);

        // 6th reset within 500 ms triggers clean PHY quiet backoff.
        assert!(record_bus_reset(&mut win, &mut count, 599));

        // After the 500 ms window expires, counter resets cleanly (including u32 wraparound).
        win = u32::MAX - 100;
        count = 5;
        assert!(!record_bus_reset(&mut win, &mut count, 450));
        assert_eq!(count, 1);
    }

    #[test]
    fn ep0_zlp_required_only_on_exact_64b_short_transfer() {
        assert!(!needs_ep0_zlp(0, 64));
        assert!(!needs_ep0_zlp(18, 64));
        assert!(!needs_ep0_zlp(64, 64));
        assert!(needs_ep0_zlp(64, 255));
        assert!(!needs_ep0_zlp(67, 255));
        assert!(needs_ep0_zlp(128, 255));
    }
}
