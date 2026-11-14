//! USB CDC ACM (Communication Device Class / Abstract Control Model) Engine.
//!
//! Provides hardware-accelerated Virtual COM Port (VCP) serial streaming,
//! DTR/RTS line state management, line coding, and ring buffer I/O.

use py32_hal::pac::usbd::*;

pub const CDC_REQ_SET_LINE_CODING: u8 = 0x20;
pub const CDC_REQ_GET_LINE_CODING: u8 = 0x21;
pub const CDC_REQ_SET_CONTROL_LINE_STATE: u8 = 0x22;

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct LineCoding {
    pub baudrate: u32,
    pub stop_bits: u8,
    pub parity: u8,
    pub data_bits: u8,
}

impl LineCoding {
    pub const fn default() -> Self {
        Self {
            baudrate: 115200,
            stop_bits: 0, // 1 stop bit
            parity: 0,    // None
            data_bits: 8, // 8 data bits
        }
    }
}

pub const CDC_RX_BUF_SIZE: usize = 128;

pub struct CdcEngine {
    line_coding: LineCoding,
    dtr: bool,
    rts: bool,
    tx_ready: bool,
    rx_buf: [u8; CDC_RX_BUF_SIZE],
    rx_head: usize,
    rx_tail: usize,
}

impl CdcEngine {
    pub const fn new() -> Self {
        Self {
            line_coding: LineCoding::default(),
            dtr: false,
            rts: false,
            tx_ready: true,
            rx_buf: [0u8; CDC_RX_BUF_SIZE],
            rx_head: 0,
            rx_tail: 0,
        }
    }

    pub fn reset(&mut self) {
        self.dtr = false;
        self.rts = false;
        self.tx_ready = true;
        self.rx_head = 0;
        self.rx_tail = 0;
    }

    pub fn push_rx(&mut self, data: &[u8]) {
        for &b in data {
            let next_head = (self.rx_head + 1) & (CDC_RX_BUF_SIZE - 1);
            if next_head != self.rx_tail {
                self.rx_buf[self.rx_head] = b;
                self.rx_head = next_head;
            }
        }
    }

    pub fn pop_rx(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        while self.rx_head != self.rx_tail && count < buf.len() {
            buf[count] = self.rx_buf[self.rx_tail];
            self.rx_tail = (self.rx_tail + 1) & (CDC_RX_BUF_SIZE - 1);
            count += 1;
        }
        count
    }

    pub fn rx_available(&self) -> usize {
        if self.rx_head >= self.rx_tail {
            self.rx_head - self.rx_tail
        } else {
            CDC_RX_BUF_SIZE - (self.rx_tail - self.rx_head)
        }
    }

    #[inline(always)]
    pub fn is_dtr(&self) -> bool {
        self.dtr
    }

    #[inline(always)]
    pub fn is_rts(&self) -> bool {
        self.rts
    }

    pub fn set_control_line_state(&mut self, value: u16) {
        self.dtr = (value & 0x01) != 0;
        self.rts = (value & 0x02) != 0;
    }

    /// Send SERIAL_STATE notification (PSTN 1.20 Section 6.5.4) over interrupt endpoint
    pub fn send_serial_state(&mut self, ep: u8, dcd: bool, dsr: bool) {
        set_active_ep(ep);
        let csr = unsafe { USBD_IN_CSR1_U8.read_volatile() };
        if (csr & INCSR1_INPKTRDY) != 0 {
            return;
        }
        let state = (if dcd { 0x01 } else { 0x00 }) | (if dsr { 0x02 } else { 0x00 });
        let notif: [u8; 10] = [0xA1, 0x20, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, state, 0x00];
        write_fifo(ep, &notif);
        unsafe {
            USBD_IN_CSR1_U8.write_volatile(csr | INCSR1_INPKTRDY);
        }
    }

    pub fn set_line_coding(&mut self, bytes: &[u8; 7]) {
        self.line_coding.baudrate = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        self.line_coding.stop_bits = bytes[4];
        self.line_coding.parity = bytes[5];
        self.line_coding.data_bits = bytes[6];
    }

    pub fn get_line_coding(&self, out: &mut [u8; 7]) {
        out[0..4].copy_from_slice(&self.line_coding.baudrate.to_le_bytes());
        out[4] = self.line_coding.stop_bits;
        out[5] = self.line_coding.parity;
        out[6] = self.line_coding.data_bits;
    }

    /// Transmit serial data chunk via EP4 IN
    pub fn write(&mut self, ep: u8, data: &[u8]) -> usize {
        let chunk = data.len().min(64);
        if chunk > 0 {
            write_fifo(ep, &data[..chunk]);
            set_active_ep(ep);
            unsafe {
                let csr = USBD_IN_CSR1_U8.read_volatile();
                USBD_IN_CSR1_U8.write_volatile(csr | INCSR1_INPKTRDY);
            }
            self.tx_ready = false;
        }
        chunk
    }

    /// Read incoming serial data chunk from EP4 OUT
    pub fn read(&mut self, ep: u8, buf: &mut [u8], count: usize) -> usize {
        let to_read = buf.len().min(count);
        if to_read > 0 {
            read_fifo(ep, &mut buf[..to_read]);
        }
        to_read
    }

    pub fn handle_in_complete(&mut self) {
        self.tx_ready = true;
    }
}

impl Default for CdcEngine {
    fn default() -> Self {
        Self::new()
    }
}
