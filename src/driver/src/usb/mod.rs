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
    ptr: *const u8,
    total_len: u16,
    sent_len: u16,
}

/// Hardware USB Device Controller driver for PY32F071
pub struct UsbDriver {
    is_connected: bool,
    was_connected: bool,
    consecutive_sof: u8,
    last_sof_ms: u32,
    last_transition_ms: u32,
    is_initialized: bool,
    dev_addr: u8,
    pending_addr: u8,
    ep0_transfer: Option<Ep0MultiPacket>,
    configured: bool,

    product_name: [u8; 32],
    product_len: u8,

    battery_voltage_mv: u16,
    battery_pct: u8,
    is_charging: bool,

    #[cfg(feature = "usb_cdc")]
    cdc: CdcEngine,

    #[cfg(feature = "usb_msc")]
    msc: MscEngine,
}

impl UsbDriver {
    /// Create a new, uninitialized USB driver instance (const constructible).
    pub const fn new() -> Self {
        Self {
            is_connected: false,
            was_connected: false,
            consecutive_sof: 0,
            last_sof_ms: 0,
            last_transition_ms: 0,
            is_initialized: false,
            dev_addr: 0,
            pending_addr: 0,
            ep0_transfer: None,
            configured: false,

            product_name: *b"Quansheng UV-K1                 ",
            product_len: 15,

            battery_voltage_mv: 0,
            battery_pct: 0,
            is_charging: false,

            #[cfg(feature = "usb_cdc")]
            cdc: CdcEngine::new(),

            #[cfg(feature = "usb_msc")]
            msc: MscEngine::new(),
        }
    }

    /// Update configured device identity (e.g. "Quansheng UV-K1 1D9E") for USB product descriptor.
    pub fn update_device_identity(&mut self, model: &str, tag: &str) {
        let m_bytes = model.trim_ascii().as_bytes();
        let t_bytes = tag.trim_ascii().as_bytes();
        let m_len = m_bytes.len().min(12);
        let t_len = t_bytes.len().min(8);

        let mut buf = [b' '; 32];
        const PREFIX: &[u8] = b"Quansheng ";
        buf[..PREFIX.len()].copy_from_slice(PREFIX);
        let mut pos = PREFIX.len();

        buf[pos..pos + m_len].copy_from_slice(&m_bytes[..m_len]);
        pos += m_len;

        if t_len > 0 {
            buf[pos] = b' ';
            pos += 1;
            buf[pos..pos + t_len].copy_from_slice(&t_bytes[..t_len]);
            pos += t_len;
        }

        self.product_name = buf;
        self.product_len = pos as u8;
    }

    /// Update live power and battery telemetry for standard USB descriptors and CAT queries.
    pub fn update_power_telemetry(&mut self, voltage_mv: u16, pct: u8, charging: bool) {
        self.battery_voltage_mv = voltage_mv;
        self.battery_pct = pct;
        self.is_charging = charging;
    }

    /// Initialize the USBD peripheral clock and arm bus state detection interrupts.
    pub fn init(&mut self) {
        // 1. Enable peripheral clock on APB1, SYSCFG on APB2, GPIOA, and cycle reset
        py32_hal::rcc::enable_usbd();

        unsafe {
            // 2. Select EP0 and clear device address
            set_active_ep(0);
            USBD_ADDR_U8.write_volatile(0);

            // 3. Enable Suspend mode detection in POWER register
            let pwr = USBD_POWER_U8.read_volatile();
            USBD_POWER_U8.write_volatile(pwr | USB_POWER_SUSPENDENB);

            // 4. Arm bus lifecycle interrupts: Reset, Resume, Suspend, SOF
            USBD_INT_USBE_U8
                .write_volatile(USB_INTR_RESET | USB_INTR_RESUME | USB_INTR_SUSPEND | USB_INTR_SOF);

            // 5. Arm EP0 interrupts
            USBD_INT_IN1E_U8.write_volatile(1); // EP0
            USBD_INT_OUT1E_U8.write_volatile(0);

            // 6. Clear initial interrupt flags by reading INT_USB
            let _ = USBD_INT_USB_U8.read_volatile();
        }

        self.dev_addr = 0;
        self.pending_addr = 0;
        self.ep0_transfer = None;
        self.configured = false;
        self.last_sof_ms = 0;
        self.is_initialized = true;
    }

    /// Poll hardware registers non-blockingly to handle bus events, control transfers, and endpoints.
    /// Returns `Some(UsbEvent)` on connection transitions.
    pub fn poll(&mut self) -> Option<UsbEvent> {
        if !self.is_initialized {
            return None;
        }

        let int_usb = unsafe { USBD_INT_USB_U8.read_volatile() };
        let now = py32_hal::delay::millis();

        let mut event = None;

        // ── 1. Bus Lifecycle Events ──────────────────────────────────────────
        if (int_usb & USB_INTR_RESET) != 0 {
            self.consecutive_sof = 0;
            self.dev_addr = 0;
            self.pending_addr = 0;
            self.ep0_transfer = None;
            self.configured = false;
            // A bus reset is driven by an active USB host; refresh bus activity timestamp
            // so host enumeration resets do not trigger false disconnects.
            self.last_sof_ms = now;
            #[cfg(feature = "usb_cdc")]
            {
                self.cdc.reset();
                let in_ep = descriptors::CDC_EP_IN & 0x7F;
                set_active_ep(in_ep);
                unsafe {
                    USBD_IN_CSR1_U8.write_volatile(INCSR1_FLUSHFIFO);
                }
            }
            unsafe {
                USBD_ADDR_U8.write_volatile(0);
                let pwr = USBD_POWER_U8.read_volatile();
                USBD_POWER_U8.write_volatile(pwr | USB_POWER_SUSPENDENB);
            }
        }

        if (int_usb & USB_INTR_RESUME) != 0 {
            // Bus resume event: do not force connection until consecutive SOF frames arrive.
        }

        if (int_usb & USB_INTR_SOF) != 0 {
            self.last_sof_ms = now;
            self.consecutive_sof = self.consecutive_sof.saturating_add(1);

            let latch_ok = now.wrapping_sub(self.last_transition_ms) >= 50;
            if self.consecutive_sof >= 25 && !self.is_connected && latch_ok {
                self.was_connected = false;
                self.is_connected = true;
                self.last_transition_ms = now;
                if event.is_none() {
                    event = Some(UsbEvent::Connected);
                }
            }
        }

        if self.is_connected {
            let elapsed_since_sof = now.wrapping_sub(self.last_sof_ms);
            let is_suspended = (int_usb & USB_INTR_SUSPEND) != 0 && elapsed_since_sof >= 40;
            let is_disconnected = elapsed_since_sof >= 80 || is_suspended;

            if is_disconnected {
                self.was_connected = true;
                self.is_connected = false;
                self.consecutive_sof = 0;
                self.configured = false;
                self.ep0_transfer = None;
                #[cfg(feature = "usb_cdc")]
                {
                    self.cdc.reset();
                    let in_ep = descriptors::CDC_EP_IN & 0x7F;
                    set_active_ep(in_ep);
                    unsafe {
                        USBD_IN_CSR1_U8.write_volatile(INCSR1_FLUSHFIFO);
                    }
                }
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
        if let Some(ref mut xfer) = self.ep0_transfer {
            if (csr & EP0_TXPKTRDY) == 0 {
                let remaining = (xfer.total_len - xfer.sent_len) as usize;
                let chunk = remaining.min(USB_EP0_MAX_PACKET_SIZE);
                let slice = unsafe {
                    core::slice::from_raw_parts(xfer.ptr.add(xfer.sent_len as usize), chunk)
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
        }

        // Apply pending device address once status stage of SET_ADDRESS finishes
        if self.pending_addr > 0 && (csr & (EP0_TXPKTRDY | EP0_RXPKTRDY)) == 0 {
            unsafe {
                USBD_ADDR_U8.write_volatile(self.pending_addr);
            }
            self.dev_addr = self.pending_addr;
            self.pending_addr = 0;
        }

        // Check if a new SETUP packet has arrived
        if (csr & EP0_RXPKTRDY) != 0 {
            self.last_sof_ms = py32_hal::delay::millis();
            let count = unsafe { USBD_EP0_COUNT_U8.read_volatile() } as usize;
            if count >= 8 {
                let mut setup = [0u8; 8];
                read_fifo(0, &mut setup);
                self.handle_setup_packet(&setup);
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
                    #[cfg(feature = "usb_cdc")]
                    self.cdc
                        .send_serial_state(descriptors::CDC_EP_NOTIF & 0x7F, true, true);
                    self.ep0_send_zlp();
                }
                0x00 => {
                    // GET_STATUS
                    let status: [u8; 2] = [0, 0];
                    self.ep0_start_transfer(&status, w_length);
                }
                0x01 | 0x03 => {
                    // CLEAR_FEATURE / SET_FEATURE
                    self.ep0_send_zlp();
                }
                0x0B => {
                    // SET_INTERFACE
                    self.ep0_send_zlp();
                }
                0x0A => {
                    // GET_INTERFACE
                    let alt: [u8; 1] = [0];
                    self.ep0_start_transfer(&alt, w_length);
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

    fn ep0_start_string_transfer(&mut self, ascii: &[u8], max_len: u16) {
        let len = ascii.len().min(31);
        let mut desc = [0u8; 64];
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
        let name = self.product_name;
        self.ep0_start_string_transfer(&name[..len], max_len);
    }

    fn ep0_start_telemetry_transfer(&mut self, max_len: u16) {
        let mut buf = [0u8; 31];
        buf[..14].copy_from_slice(b"Supply: 0.00v ");

        let v_int = ((self.battery_voltage_mv as u32 * 4195) >> 22) as u16;
        let rem = self.battery_voltage_mv.saturating_sub(v_int * 1000);
        let d1 = ((rem as u32 * 5243) >> 19) as u16;
        let rem2 = rem.saturating_sub(d1 * 100);
        let d2 = ((rem2 as u32 * 52429) >> 19) as u16;

        buf[8] = b'0' + (v_int as u8);
        buf[10] = b'0' + (d1 as u8);
        buf[11] = b'0' + (d2 as u8);

        let pos = if self.is_charging {
            buf[14..19].copy_from_slice(b"[CHG]");
            19
        } else {
            buf[14] = b'(';
            let pct = self.battery_pct.min(100);
            if pct >= 100 {
                buf[15..20].copy_from_slice(b"100%)");
                20
            } else if pct >= 10 {
                let tens = ((pct as u32 * 52429) >> 19) as u8;
                let ones = pct - tens * 10;
                buf[15] = b'0' + tens;
                buf[16] = b'0' + ones;
                buf[17] = b'%';
                buf[18] = b')';
                19
            } else {
                buf[15] = b'0' + pct;
                buf[16] = b'%';
                buf[17] = b')';
                18
            }
        };
        self.ep0_start_string_transfer(&buf[..pos], max_len);
    }

    fn ep0_start_mac_transfer(&mut self, max_len: u16) {
        let mac = bsp::uid::read_mac_address();
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut buf = *b"MAC: 00:00:00:00:00:00";
        for (i, &b) in mac.iter().enumerate() {
            buf[5 + i * 3] = HEX[(b >> 4) as usize];
            buf[5 + i * 3 + 1] = HEX[(b & 0x0F) as usize];
        }
        self.ep0_start_string_transfer(&buf, max_len);
    }

    fn handle_get_descriptor(&mut self, w_value: u16, w_length: u16) {
        let desc_type = (w_value >> 8) as u8;
        let desc_index = (w_value & 0xFF) as u8;

        match desc_type {
            DESC_TYPE_DEVICE => {
                self.ep0_start_transfer(&DEVICE_DESCRIPTOR, w_length);
            }
            DESC_TYPE_CONFIGURATION => {
                self.ep0_start_transfer(&CONFIG_DESCRIPTOR, w_length);
            }
            DESC_TYPE_STRING => match desc_index {
                0 => self.ep0_start_transfer(&STRING_LANGID, w_length),
                1 => self.ep0_start_transfer(&STRING_MANUFACTURER, w_length),
                2 => self.ep0_start_product_transfer(w_length),
                3 => {
                    let crock = bsp::uid::get_crockford_serial();
                    self.ep0_start_string_transfer(&crock[..14], w_length);
                }
                #[cfg(feature = "usb_cdc")]
                4 => self.ep0_start_transfer(&STRING_INTERFACE_SERIAL, w_length),
                5 => self.ep0_start_telemetry_transfer(w_length),
                6 => self.ep0_start_mac_transfer(w_length),
                #[cfg(feature = "usb_msc")]
                7 => self.ep0_start_transfer(&STRING_INTERFACE_STORAGE, w_length),
                _ => self.ep0_stall(),
            },
            DESC_TYPE_DEVICE_QUALIFIER => {
                self.ep0_stall();
            }
            _ => self.ep0_stall(),
        }
    }

    #[inline(never)]
    fn handle_class_request(
        &mut self,
        _req_type: u8,
        request: u8,
        _value: u16,
        _index: u16,
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
            self.cdc.set_control_line_state(_value);
            self.cdc
                .send_serial_state(descriptors::CDC_EP_NOTIF & 0x7F, true, true);
            self.ep0_send_zlp();
            return;
        } else if request == cdc::CDC_REQ_GET_LINE_CODING {
            let mut coding = [0u8; 7];
            self.cdc.get_line_coding(&mut coding);
            self.ep0_start_transfer(&coding, length);
            return;
        } else if request == cdc::CDC_REQ_SET_LINE_CODING {
            // Acknowledge SET_LINE_CODING request handshake
            self.ep0_send_zlp();
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
    }

    fn ep0_start_transfer(&mut self, data: &[u8], max_len: u16) {
        let total = data.len().min(max_len as usize);
        set_active_ep(0);

        // Acknowledge SETUP packet reception
        unsafe {
            USBD_EP0_CSR_U8.write_volatile(EP0_SVRXPKTRDY);
        }

        if total == 0 {
            unsafe {
                USBD_EP0_CSR_U8.write_volatile(EP0_SVRXPKTRDY | EP0_DATAEND);
            }
            self.ep0_transfer = None;
            return;
        }

        let chunk = total.min(USB_EP0_MAX_PACKET_SIZE);
        write_fifo(0, &data[..chunk]);

        if chunk >= total {
            unsafe {
                USBD_EP0_CSR_U8.write_volatile(EP0_TXPKTRDY | EP0_DATAEND);
            }
            self.ep0_transfer = None;
        } else {
            self.ep0_transfer = Some(Ep0MultiPacket {
                ptr: data.as_ptr(),
                total_len: total as u16,
                sent_len: chunk as u16,
            });
            unsafe {
                USBD_EP0_CSR_U8.write_volatile(EP0_TXPKTRDY);
            }
        }
    }

    fn ep0_send_zlp(&mut self) {
        set_active_ep(0);
        // On PY32F071 MUSB EP0, zero-data control transfers require
        // acknowledging the SETUP packet with SvRxPktRdy and setting DataEnd.
        unsafe {
            USBD_EP0_CSR_U8.write_volatile(EP0_SVRXPKTRDY | EP0_DATAEND);
        }
    }

    fn ep0_stall(&mut self) {
        set_active_ep(0);
        unsafe {
            USBD_EP0_CSR_U8.write_volatile(EP0_SENDSTALL);
        }
    }

    #[inline(always)]
    pub fn is_connected(&self) -> bool {
        self.is_connected
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
            if !self.is_connected || !self.configured {
                return 0;
            }
            if !_data.is_empty() {
                py32_hal::uart::mark_activity();
            }
            let in_ep = descriptors::CDC_EP_IN & 0x7F;
            self.cdc.write(in_ep, _data)
        }
        #[cfg(not(feature = "usb_cdc"))]
        {
            0
        }
    }

    /// Read incoming serial data chunk from USB CDC ACM
    pub fn cdc_read(&mut self, _buf: &mut [u8]) -> usize {
        #[cfg(feature = "usb_cdc")]
        {
            if !self.is_connected || !self.configured {
                return 0;
            }
            let out_ep = descriptors::CDC_EP_OUT & 0x7F;
            set_active_ep(out_ep);
            let out_csr = unsafe { USBD_OUT_CSR1_U8.read_volatile() };
            if (out_csr & OUTCSR1_OUTPKTRDY) == 0 {
                return 0;
            }
            let count = unsafe { USBD_OUT_COUNT_U16.read_volatile() } as usize;
            let read = self.cdc.read(out_ep, _buf, count);
            if read > 0 {
                py32_hal::uart::mark_activity();
                unsafe {
                    USBD_OUT_CSR1_U8.write_volatile(0);
                }
            }
            read
        }
        #[cfg(not(feature = "usb_cdc"))]
        {
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
