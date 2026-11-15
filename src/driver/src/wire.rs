//! Serial Wire Protocol Driver (RigWire Core framing over UART).
//!
//! Provides zero-allocation streaming reception, delimiter tracking,
//! in-place COBS decoding, and dispatching of verified wire records.

pub mod core;

use crypto::cobs::Cobs;
use kernel::service::SerialPort;
use kernel::wire::*;

pub const MAX_RAW_BUF_LEN: usize = 146;
pub const MAX_WIRE_BUF_LEN: usize = 149;

#[repr(C)]
pub struct WireDriver<S: SerialPort> {
    // Keep hot RX collector state in first 32 B for Cortex-M0+ Thumb-1 imm5.
    rx_stage_pos: u8,
    rx_stage_len: u8,
    in_frame: bool,
    transport_profile: u16,
    rx_len: usize,
    pub session: WireSession,
    pub port: S,
    rx_stage: [u8; 64],
    rx_buf: [u8; MAX_WIRE_BUF_LEN],
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
            rx_stage: [0; 64],
            rx_stage_pos: 0,
            rx_stage_len: 0,
            in_frame: false,
        }
    }

    /// Reset the streaming frame collector, clearing partial frames and noise.
    pub fn reset_collector(&mut self) {
        self.rx_len = 0;
        self.rx_stage_pos = 0;
        self.rx_stage_len = 0;
        self.in_frame = false;
    }

    /// Poll the serial byte stream, feeding the bounded COBS frame collector.
    #[inline(never)]
    pub fn poll(&mut self, uptime_ms: u32) -> Option<(WireHeader, usize)> {
        if self.rx_stage_pos >= self.rx_stage_len {
            let read_count = self.port.read(&mut self.rx_stage);
            if read_count == 0 {
                return None;
            }
            self.rx_stage_pos = 0;
            self.rx_stage_len = read_count.min(self.rx_stage.len()) as u8;
        }

        while (self.rx_stage_pos as usize) < (self.rx_stage_len as usize) {
            let b = self.rx_stage[self.rx_stage_pos as usize];
            self.rx_stage_pos += 1;

            if b == crypto::cobs::COBS_DELIMITER {
                if self.in_frame && self.rx_len > 0 {
                    let rx_len = self.rx_len;
                    self.rx_len = 0;
                    self.in_frame = false;
                    let mut hdr_uninit = ::core::mem::MaybeUninit::<WireHeader>::uninit();
                    if let Ok(raw_len) = Cobs::decode_inplace(&mut self.rx_buf, rx_len)
                        && self
                            .session
                            .validate_record(&self.rx_buf[..raw_len], unsafe {
                                &mut *hdr_uninit.as_mut_ptr()
                            })
                            == WireStatus::Ok
                    {
                        let header = unsafe { hdr_uninit.assume_init() };
                        if header.service == SERVICE_CORE {
                            self.handle_core_operation(&header, raw_len, uptime_ms);
                        } else {
                            return Some((header, raw_len));
                        }
                    }
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

        None
    }

    #[inline(never)]
    fn handle_ascii_line(&mut self) {
        let cmd = &self.rx_buf[..self.rx_len];
        if utils::format::ascii_str(cmd)
            .is_some_and(|s| utils::format::ascii_eq_ignore_case(s, "reboot"))
        {
            kernel::power::PowerControl::reboot();
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
        let mut raw_buf = ::core::mem::MaybeUninit::<[u8; MAX_RAW_BUF_LEN]>::uninit();
        let mut wire_out = ::core::mem::MaybeUninit::<[u8; MAX_WIRE_BUF_LEN]>::uninit();
        let raw_slice = unsafe { &mut *raw_buf.as_mut_ptr() };
        let wire_slice = unsafe { &mut *wire_out.as_mut_ptr() };

        let frame_len = match status {
            Some(st) => self.session.frame_response(
                service,
                opcode,
                sequence,
                st,
                payload_data,
                raw_slice,
                wire_slice,
            )?,
            None => self.session.frame_event(
                service,
                opcode,
                sequence,
                payload_data,
                raw_slice,
                wire_slice,
            )?,
        };

        self.port.write(&wire_slice[..frame_len]);
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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPort {
        rx: [u8; 256],
        rx_len: usize,
        rx_pos: usize,
        tx_len: usize,
    }

    impl MockPort {
        fn new() -> Self {
            Self {
                rx: [0; 256],
                rx_len: 0,
                rx_pos: 0,
                tx_len: 0,
            }
        }
    }

    impl SerialPort for MockPort {
        fn write(&mut self, data: &[u8]) {
            self.tx_len += data.len();
        }

        fn read(&mut self, buf: &mut [u8]) -> usize {
            let avail = self.rx_len.saturating_sub(self.rx_pos);
            let n = avail.min(buf.len());
            if n > 0 {
                buf[..n].copy_from_slice(&self.rx[self.rx_pos..self.rx_pos + n]);
                self.rx_pos += n;
            }
            n
        }
    }

    #[test]
    fn test_coalesced_frames_in_single_read_and_max_128b_response() {
        let mut session = WireSession::new();
        session.begin_session(0x1122_3344, 128);

        let mut raw = [0u8; MAX_RAW_BUF_LEN];
        let mut wire1 = [0u8; MAX_WIRE_BUF_LEN];
        let mut wire2 = [0u8; MAX_WIRE_BUF_LEN];
        let n1 = session
            .frame_record_raw(
                SERVICE_CONTROL,
                0x01,
                0,
                1,
                None,
                b"ONE!",
                &mut raw,
                &mut wire1,
            )
            .unwrap();
        let n2 = session
            .frame_record_raw(
                SERVICE_CONTROL,
                0x02,
                0,
                2,
                None,
                b"TWO!!",
                &mut raw,
                &mut wire2,
            )
            .unwrap();

        let mut port = MockPort::new();
        assert!(n1 + n2 <= 64);
        port.rx[..n1].copy_from_slice(&wire1[..n1]);
        port.rx[n1..n1 + n2].copy_from_slice(&wire2[..n2]);
        port.rx_len = n1 + n2;

        let mut drv = WireDriver::new(port);
        drv.session.begin_session(0x1122_3344, 128);

        let (hdr1, _) = drv.poll(100).expect("first coalesced frame");
        assert_eq!(hdr1.sequence, 1);
        assert_eq!(drv.current_payload(&hdr1), b"ONE!");

        let (hdr2, _) = drv.poll(100).expect("second coalesced frame");
        assert_eq!(hdr2.sequence, 2);
        assert_eq!(drv.current_payload(&hdr2), b"TWO!!");
        assert!(drv.poll(100).is_none());

        // Max 128-byte response payload (8B header + 120B data) must fit without BadLength.
        let max_payload = [0xA5u8; 128];
        let written = drv
            .send_response(&hdr2, WireStatus::Ok, &max_payload)
            .unwrap();
        assert!(written <= MAX_WIRE_BUF_LEN);
        assert_eq!(drv.port.tx_len, written);
    }
}
