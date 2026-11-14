//! Serial Wire Protocol Driver (RigWire Core framing over UART).
//!
//! Provides zero-allocation streaming reception, delimiter tracking,
//! in-place COBS decoding, and dispatching of verified wire records.

pub mod core;

use crypto::cobs::Cobs;
use kernel::service::SerialPort;
use kernel::wire::*;

pub const MAX_WIRE_BUF_LEN: usize = 148;
pub const MAX_RAW_BUF_LEN: usize = 145;

pub struct WireDriver<S: SerialPort> {
    pub port: S,
    pub session: WireSession,
    pub transport_profile: u16,
    rx_buf: [u8; MAX_WIRE_BUF_LEN],
    rx_len: usize,
    in_frame: bool,
}

impl<S: SerialPort> WireDriver<S> {
    pub const fn new(port: S) -> Self {
        Self::new_with_transport(port, 0x0101) // Default: UART 115200 (§5.1 profile)
    }

    pub const fn new_with_transport(port: S, transport_profile: u16) -> Self {
        Self {
            port,
            session: WireSession::new(),
            transport_profile,
            rx_buf: [0; MAX_WIRE_BUF_LEN],
            rx_len: 0,
            in_frame: false,
        }
    }

    /// Reset the streaming frame collector, clearing partial frames and noise.
    pub fn reset_collector(&mut self) {
        self.rx_len = 0;
        self.in_frame = false;
    }

    /// Poll the serial byte stream, feeding the bounded COBS frame collector.
    #[inline(never)]
    pub fn poll(&mut self, uptime_ms: u32) -> Option<(WireHeader, usize)> {
        let mut byte_buf = [0u8; 64];
        let read_count = self.port.read(&mut byte_buf);
        if read_count == 0 {
            return None;
        }

        let mut completed_record = None;

        for &b in &byte_buf[..read_count] {
            if b == crypto::cobs::COBS_DELIMITER {
                if self.in_frame && self.rx_len > 0 {
                    if let Ok(raw_len) = Cobs::decode_inplace(&mut self.rx_buf, self.rx_len)
                        && let Ok((header, _)) =
                            self.session.validate_record(&self.rx_buf[..raw_len])
                    {
                        completed_record = Some((header, raw_len));
                    }
                    self.rx_len = 0;
                    self.in_frame = false;
                } else {
                    self.rx_len = 0;
                    self.in_frame = true;
                }
            } else if !self.in_frame && (b == b'\r' || b == b'\n') {
                if self.rx_len > 0 {
                    self.handle_ascii_line();
                    self.rx_len = 0;
                }
            } else if self.in_frame {
                if self.rx_len < MAX_WIRE_BUF_LEN {
                    self.rx_buf[self.rx_len] = b;
                    self.rx_len += 1;
                } else {
                    self.in_frame = false;
                    self.rx_len = 0;
                }
            } else {
                // Buffer printable ASCII bytes when outside COBS frame
                if b >= 0x20 && b <= 0x7E && self.rx_len < 16 {
                    self.rx_buf[self.rx_len] = b;
                    self.rx_len += 1;
                }
            }
        }

        if let Some((header, raw_len)) = completed_record {
            if header.service == SERVICE_CORE {
                self.handle_core_operation(&header, raw_len, uptime_ms);
                return None;
            }
            return Some((header, raw_len));
        }

        None
    }

    #[inline(never)]
    fn handle_ascii_line(&mut self) {
        let cmd = &self.rx_buf[..self.rx_len];
        if cmd.eq_ignore_ascii_case(b"reboot") {
            cortex_m::peripheral::SCB::sys_reset();
        }
        self.port.write(b"RigWire\r\n");
    }

    pub fn current_payload<'a>(&'a self, header: &WireHeader) -> &'a [u8] {
        let covered = HEADER_LEN + header.payload_len as usize;
        &self.rx_buf[HEADER_LEN..covered]
    }

    #[inline(never)]
    fn send_frame(
        &mut self,
        service: u8,
        opcode: u8,
        sequence: u32,
        status: Option<WireStatus>,
        payload_data: &[u8],
    ) -> Result<usize, WireStatus> {
        let mut raw_uninit = ::core::mem::MaybeUninit::<[u32; (MAX_RAW_BUF_LEN + 3) / 4]>::uninit();
        let raw_buf = unsafe {
            ::core::slice::from_raw_parts_mut(raw_uninit.as_mut_ptr() as *mut u8, MAX_RAW_BUF_LEN)
        };
        let mut wire_uninit =
            ::core::mem::MaybeUninit::<[u32; (MAX_WIRE_BUF_LEN + 3) / 4]>::uninit();
        let wire_out = unsafe {
            ::core::slice::from_raw_parts_mut(wire_uninit.as_mut_ptr() as *mut u8, MAX_WIRE_BUF_LEN)
        };

        let frame_len = match status {
            Some(st) => self.session.frame_response(
                service,
                opcode,
                sequence,
                st,
                payload_data,
                raw_buf,
                wire_out,
            )?,
            None => self.session.frame_event(
                service,
                opcode,
                sequence,
                payload_data,
                raw_buf,
                wire_out,
            )?,
        };

        self.port.write(&wire_out[..frame_len]);
        Ok(frame_len)
    }

    pub fn send_response(
        &mut self,
        header: &WireHeader,
        status: WireStatus,
        payload_data: &[u8],
    ) -> Result<usize, WireStatus> {
        self.send_frame(
            header.service,
            header.opcode,
            header.sequence,
            Some(status),
            payload_data,
        )
    }

    pub fn send_event(
        &mut self,
        service: u8,
        opcode: u8,
        sequence: u32,
        payload_data: &[u8],
    ) -> Result<usize, WireStatus> {
        self.send_frame(service, opcode, sequence, None, payload_data)
    }
}
