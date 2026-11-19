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

#[derive(Copy, Clone)]
struct Ep0MultiPacket {
    ptr: core::ptr::NonNull<u8>,
    total_len: u16,
    sent_len: u16,
}

/// Hardware USB Device Controller driver for PY32F071
#[repr(C)]
pub struct UsbDriver {
    last_sof_ms: u32,
    last_transition_ms: u32,

    // Keep consecutive 1-byte state flags 4-byte aligned and < 32 B for Cortex-M0+ Thumb-1 imm5.
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
    product_len: u8,
    battery_pct: u8,
    battery_voltage_mv: u16,

    ep0_transfer: Option<Ep0MultiPacket>,

    #[cfg(feature = "usb_cdc")]
    cdc: CdcEngine,

    product_name: [u8; 32],

    #[cfg(feature = "usb_msc")]
    msc: MscEngine,
}

impl UsbDriver {
    /// Create a new, uninitialized USB driver instance (const constructible).
    pub const fn new() -> Self {
        Self {
            last_sof_ms: 0,
            last_transition_ms: 0,
            ep0_transfer: None,
            battery_voltage_mv: 0,
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

    /// Release USB soft-connect pull-up and reset link state before deep STOP sleep.
    pub fn disconnect_for_sleep(&mut self) {
        unsafe {
            let pwr = USBD_POWER_U8.read_volatile();
            USBD_POWER_U8.write_volatile(pwr & !USB_POWER_SOFTCONN);
        }
        self.is_connected = false;
        self.configured = false;
        self.consecutive_sof = 0;
        self.ep0_transfer = None;
    }

    /// Update live power and battery telemetry for standard USB descriptors and CAT queries.
    pub fn update_power_telemetry(&mut self, voltage_mv: u16, pct: u8, charging: bool) {
        self.battery_voltage_mv = voltage_mv;
        self.battery_pct = pct;
        self.is_charging = charging;
    }

    /// Initialize the USBD peripheral clock and arm bus state detection interrupts.
    pub fn init(&mut self) {
        // Guarantee bus clock is at 48 MHz required by USB 2.0 Full-Speed PHY
        if py32_hal::rcc::sysclk_hz() != 48_000_000 {
            py32_hal::rcc::set_sysclk_48mhz();
        }

        // 1. Enable peripheral clock on APB1, SYSCFG on APB2, GPIOA, and cycle reset
        py32_hal::rcc::enable_usbd();

        unsafe {
            // 2. Select EP0, clear device address, and engage Soft Connect
            set_active_ep(0);
            self.reset_bus_state();

            // 3. Arm bus lifecycle interrupts: Reset, Resume, Suspend, SOF
            USBD_INT_USBE_U8
                .write_volatile(USB_INTR_RESET | USB_INTR_RESUME | USB_INTR_SUSPEND | USB_INTR_SOF);

            // 4. Arm EP0 interrupts
            USBD_INT_IN1E_U8.write_volatile(1); // EP0
            USBD_INT_OUT1E_U8.write_volatile(0);

            // 5. Clear initial interrupt flags by reading INT_USB
            let _ = USBD_INT_USB_U8.read_volatile();
        }

        self.last_sof_ms = 0;
        self.is_initialized = true;
    }

    #[inline(never)]
    fn reset_bus_state(&mut self) {
        self.dev_addr = 0;
        self.pending_addr = 0;
        self.ep0_transfer = None;
        self.configured = false;
        unsafe {
            USBD_ADDR_U8.write_volatile(0);
            let pwr = USBD_POWER_U8.read_volatile();
            USBD_POWER_U8.write_volatile(
                (pwr | USB_POWER_SUSPENDENB | USB_POWER_SOFTCONN) & !USB_POWER_HSENAB,
            );
        }
    }

    /// Poll hardware registers non-blockingly to handle bus events, control transfers, and endpoints.
    /// Returns `Some(UsbEvent)` on connection transitions.
    pub fn poll(&mut self) -> Option<UsbEvent> {
        if !self.is_initialized {
            return None;
        }

        let int_usb = unsafe { USBD_INT_USB_U8.read_volatile() };
        let now = py32_hal::delay::millis();

        // ── 0. Clock Alignment Safeguard ─────────────────────────────────────
        // If any USB host bus activity (Reset, Resume, SOF) is detected while
        // running at a non-48 MHz system clock (e.g. 16, 24, 32, 72 MHz), immediately
        // snap SYSCLK to 48 MHz so the USBD PHY decoder locks onto the 12 Mbps NRZI stream.
        if (int_usb & (USB_INTR_RESET | USB_INTR_RESUME | USB_INTR_SOF)) != 0
            && py32_hal::rcc::sysclk_hz() != 48_000_000
        {
            py32_hal::rcc::set_sysclk_48mhz();
        }

        let mut event = None;

        // ── 1. Bus Lifecycle Events ──────────────────────────────────────────
        if (int_usb & USB_INTR_RESET) != 0 {
            self.consecutive_sof = 0;
            self.reset_bus_state();
            // Bus reset is SE0 (both D+ and D- low). Note: do NOT update last_sof_ms here!
            // When a cable is unplugged, D+ drops to 0V causing continuous SE0 (RESET).
            // Updating last_sof_ms on reset would mask the unplug state indefinitely.
            #[cfg(feature = "usb_cdc")]
            {
                self.pending_set_line_coding = false;
                self.cdc.reset();
                let in_ep = descriptors::CDC_EP_IN & 0x7F;
                set_active_ep(in_ep);
                unsafe {
                    USBD_IN_CSR1_U8.write_volatile(INCSR1_FLUSHFIFO);
                }
            }
        }

        if (int_usb & USB_INTR_RESUME) != 0 {
            // Bus resume event: do not force connection until consecutive SOF frames arrive.
        }

        if (int_usb & USB_INTR_SOF) != 0 {
            self.last_sof_ms = now;
            self.consecutive_sof = self.consecutive_sof.saturating_add(1);

            let latch_ok = now.wrapping_sub(self.last_transition_ms) >= 50;
            if self.consecutive_sof >= 10 && !self.is_connected && latch_ok {
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

        if self.is_connected {
            let elapsed_since_sof = now.wrapping_sub(self.last_sof_ms);
            let has_suspend = (int_usb & USB_INTR_SUSPEND) != 0;
            // Robust disconnect detection:
            // 1. Explicit bus suspend interrupt received and no SOF for >= 150ms
            // 2. No SOF received for >= 300ms (sustained silence)
            let is_disconnected =
                (has_suspend && elapsed_since_sof >= 150) || elapsed_since_sof >= 300;

            if is_disconnected {
                self.was_connected = true;
                self.is_connected = false;
                self.configured = false;
                self.consecutive_sof = 0;
                self.last_transition_ms = now;
                if event.is_none() {
                    event = Some(UsbEvent::Disconnected);
                }
            }
        }

        // ── 2. Endpoint 0 Control Processing ─────────────────────────────────
        // NOTE: Always process EP0 transfers, even on frames where bus events occurred.
        self.poll_ep0();

        // ── 3. Class Endpoint Processing ─────────────────────────────────────
        #[cfg(feature = "usb_msc")]
        self.poll_msc_endpoints();

        #[cfg(feature = "usb_cdc")]
        self.poll_cdc_endpoints();

        event
    }

    #[inline(never)]
    fn poll_ep0(&mut self) {
        set_active_ep(0);
        let csr = unsafe { USBD_EP0_CSR_U8.read_volatile() };

        // Handle SetupEnd (premature termination by host)
        if (csr & EP0_SETUPEND) != 0 {
            unsafe {
                USBD_EP0_CSR_U8.write_volatile(EP0_SVSETUPEND);
            }
            self.ep0_transfer = None;
            return;
        }

        // Handle SentStall
        if (csr & EP0_SENTSTALL) != 0 {
            unsafe {
                USBD_EP0_CSR_U8.write_volatile(0);
            }
            self.ep0_transfer = None;
            return;
        }

        // Continue multi-packet control IN transfer if active
        if let Some(ref mut xfer) = self.ep0_transfer
            && (csr & EP0_TXPKTRDY) == 0
        {
            let remaining = (xfer.total_len - xfer.sent_len) as usize;
            let chunk = remaining.min(USB_EP0_MAX_PACKET_SIZE);
            let slice = unsafe {
                core::slice::from_raw_parts(xfer.ptr.as_ptr().add(xfer.sent_len as usize), chunk)
            };
            write_fifo(0, slice);
            xfer.sent_len += chunk as u16;

            if xfer.sent_len >= xfer.total_len {
                unsafe {
                    USBD_EP0_CSR_U8.write_volatile(EP0_TXPKTRDY | EP0_DATAEND);
                }
                self.ep0_transfer = None;
            } else {
                unsafe {
                    USBD_EP0_CSR_U8.write_volatile(EP0_TXPKTRDY);
                }
            }
            return;
        }

        // Apply pending device address once status stage of SET_ADDRESS finishes
        if self.pending_addr > 0 && (csr & (EP0_TXPKTRDY | EP0_RXPKTRDY)) == 0 {
            unsafe {
                USBD_ADDR_U8.write_volatile(self.pending_addr);
            }
            self.dev_addr = self.pending_addr;
            self.pending_addr = 0;
        }

        // Handle pending CDC SET_LINE_CODING OUT data stage
        #[cfg(feature = "usb_cdc")]
        if self.pending_set_line_coding {
            if (csr & EP0_RXPKTRDY) != 0 {
                let count = unsafe { USBD_EP0_COUNT_U8.read_volatile() } as usize;
                if count >= 7 {
                    let mut coding_uninit = core::mem::MaybeUninit::<[u8; 7]>::uninit();
                    let coding = unsafe { &mut *coding_uninit.as_mut_ptr() };
                    read_fifo(0, coding);
                    self.cdc.set_line_coding(coding);
                }
                unsafe {
                    USBD_EP0_CSR_U8.write_volatile(EP0_SVRXPKTRDY | EP0_DATAEND);
                }
                self.pending_set_line_coding = false;
            }
            return;
        }

        // Check if a new SETUP packet has arrived
        if (csr & EP0_RXPKTRDY) != 0 {
            self.last_sof_ms = py32_hal::delay::millis();
            let count = unsafe { USBD_EP0_COUNT_U8.read_volatile() } as usize;
            if count >= 8 {
                let mut setup_uninit = core::mem::MaybeUninit::<[u8; 8]>::uninit();
                let setup = unsafe { &mut *setup_uninit.as_mut_ptr() };
                read_fifo(0, setup);
                self.handle_setup_packet(setup);
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

    fn configure_endpoints(&mut self) {
        #[cfg(feature = "usb_msc")]
        {
            let in_ep = descriptors::MSC_EP_IN & 0x7F;
            let out_ep = descriptors::MSC_EP_OUT & 0x7F;

            // MSC Data IN Bulk
            set_active_ep(in_ep);
            unsafe {
                USBD_IN_CSR2_U8.write_volatile(INCSR2_MODE);
                USBD_MAX_PKT_IN_U8.write_volatile(8); // 64 bytes (8 * 8)
                let int_in = USBD_INT_IN1E_U8.read_volatile();
                USBD_INT_IN1E_U8.write_volatile(int_in | (1 << in_ep));
            }

            // MSC Data OUT Bulk
            set_active_ep(out_ep);
            unsafe {
                USBD_MAX_PKT_OUT_U8.write_volatile(8); // 64 bytes
                let int_out = USBD_INT_OUT1E_U8.read_volatile();
                USBD_INT_OUT1E_U8.write_volatile(int_out | (1 << out_ep));
            }
        }

        #[cfg(feature = "usb_cdc")]
        {
            let notif_ep = descriptors::CDC_EP_NOTIF & 0x7F;
            let in_ep = descriptors::CDC_EP_IN & 0x7F;
            let out_ep = descriptors::CDC_EP_OUT & 0x7F;

            // CDC Notification Interrupt EP
            set_active_ep(notif_ep);
            unsafe {
                USBD_IN_CSR2_U8.write_volatile(INCSR2_MODE);
                USBD_MAX_PKT_IN_U8.write_volatile(2); // 16 bytes
            }

            // CDC Data IN Bulk
            set_active_ep(in_ep);
            unsafe {
                USBD_IN_CSR1_U8.write_volatile(INCSR1_CLRDT | INCSR1_FLUSHFIFO);
                USBD_IN_CSR2_U8.write_volatile(INCSR2_MODE);
                USBD_MAX_PKT_IN_U8.write_volatile(8); // 64 bytes
                let int_in = USBD_INT_IN1E_U8.read_volatile();
                USBD_INT_IN1E_U8.write_volatile(int_in | (1 << in_ep));
            }

            // CDC Data OUT Bulk
            set_active_ep(out_ep);
            unsafe {
                if in_ep != out_ep {
                    USBD_IN_CSR2_U8.write_volatile(0);
                }
                USBD_OUT_CSR1_U8.write_volatile(OUTCSR1_CLRDT | OUTCSR1_FLUSHFIFO);
                USBD_MAX_PKT_OUT_U8.write_volatile(8); // 64 bytes
                let int_out = USBD_INT_OUT1E_U8.read_volatile();
                USBD_INT_OUT1E_U8.write_volatile(int_out | (1 << out_ep));
            }
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

    #[cfg(feature = "usb_cdc")]
    fn poll_cdc_endpoints(&mut self) {
        if !self.configured {
            return;
        }

        let in_ep = descriptors::CDC_EP_IN & 0x7F;
        set_active_ep(in_ep);
        let in_csr = unsafe { USBD_IN_CSR1_U8.read_volatile() };
        if (in_csr & INCSR1_INPKTRDY) == 0 {
            self.cdc.handle_in_complete();
        }

        // Drain incoming EP4 OUT packets into CDC ring buffer immediately
        let out_ep = descriptors::CDC_EP_OUT & 0x7F;
        set_active_ep(out_ep);
        let out_csr = unsafe { USBD_OUT_CSR1_U8.read_volatile() };
        if (out_csr & OUTCSR1_OUTPKTRDY) != 0 {
            let count = (unsafe { USBD_OUT_COUNT_U16.read_volatile() } as usize).min(64);
            if count > 0 {
                let mut temp_uninit = core::mem::MaybeUninit::<[u8; 64]>::uninit();
                let temp = unsafe { &mut *temp_uninit.as_mut_ptr() };
                read_fifo(out_ep, &mut temp[..count]);
                self.cdc.push_rx(&temp[..count]);
                py32_hal::uart::mark_activity();
            }
            unsafe {
                USBD_OUT_CSR1_U8.write_volatile(0);
            }
        }
        set_active_ep(0);
    }

    #[inline(never)]
    fn ep0_start_transfer(&mut self, data: &[u8], max_len: u16) {
        let total = data.len().min(max_len as usize);
        if total == 0 {
            self.ep0_send_zlp();
            self.ep0_transfer = None;
            return;
        }

        // Acknowledge SETUP packet reception
        Self::ep0_write_csr(EP0_SVRXPKTRDY);

        let chunk = total.min(USB_EP0_MAX_PACKET_SIZE);
        write_fifo(0, &data[..chunk]);

        if chunk >= total {
            unsafe {
                USBD_EP0_CSR_U8.write_volatile(EP0_TXPKTRDY | EP0_DATAEND);
            }
            self.ep0_transfer = None;
        } else {
            self.ep0_transfer = Some(Ep0MultiPacket {
                ptr: core::ptr::NonNull::new(data.as_ptr() as *mut u8).unwrap(),
                total_len: total as u16,
                sent_len: chunk as u16,
            });
            unsafe {
                USBD_EP0_CSR_U8.write_volatile(EP0_TXPKTRDY);
            }
        }
    }

    #[inline(never)]
    fn ep0_write_csr(csr: u8) {
        set_active_ep(0);
        unsafe {
            USBD_EP0_CSR_U8.write_volatile(csr);
        }
    }

    #[inline(always)]
    fn ep0_send_zlp(&mut self) {
        // On PY32F071 MUSB EP0, zero-data control transfers require
        // acknowledging the SETUP packet with SvRxPktRdy and setting DataEnd.
        Self::ep0_write_csr(EP0_SVRXPKTRDY | EP0_DATAEND);
    }

    #[inline(always)]
    fn ep0_stall(&mut self) {
        Self::ep0_write_csr(EP0_SENDSTALL);
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
        self.ep0_transfer.is_some() || self.pending_addr > 0
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

    /// Transmit serial data chunk via USB CDC ACM
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
                set_active_ep(in_ep);
                let mut timeout = 50_000;
                while (unsafe { USBD_IN_CSR1_U8.read_volatile() } & INCSR1_INPKTRDY) != 0 {
                    timeout -= 1;
                    if timeout == 0 {
                        return sent;
                    }
                }
                write_fifo(in_ep, chunk);
                unsafe {
                    USBD_IN_CSR1_U8.write_volatile(INCSR1_INPKTRDY);
                }
                sent += chunk.len();
            }
            set_active_ep(0);
            sent
        }
        #[cfg(not(feature = "usb_cdc"))]
        {
            let _ = _data;
            0
        }
    }

    /// Read incoming serial data chunk from USB CDC ACM
    pub fn cdc_read(&mut self, _buf: &mut [u8]) -> usize {
        #[cfg(feature = "usb_cdc")]
        {
            if !self.configured {
                return 0;
            }
            let read = self.cdc.pop_rx(_buf);
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
