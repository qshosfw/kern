//! RigWire v2 Streaming COBS Frame Engine (`WireEngine<S>`).
//!
//! Provides zero-allocation streaming reception, terminator-only COBS framing
//! (never drops the first packet when no leading `0x00` precedes it), in-place
//! decoding, and zero-copy sealing/transmission of verified 8-byte `WireHeader` records.

use crate::service::SerialPort;
use crate::wire::*;
use crypto::cobs::Cobs;

/// Maximum decoded RigWire v2 record (`8 B header + 264 B payload + 16 B Ascon tag + 2 B CRC-16 = 290 B`).
pub const MAX_RAW_BUF_LEN: usize = HEADER_LEN + MAX_PAYLOAD_LEN + AUTH_TAG_LEN + CRC_LEN;
/// Maximum COBS-encoded frame including overhead bytes and trailing `0x00` (`296 B`, 4-byte aligned).
pub const MAX_WIRE_BUF_LEN: usize = 296;

#[repr(C)]
pub struct WireEngine<S: SerialPort> {
    // Hot RX collector state in first 32 B for Cortex-M0+ Thumb-1 imm5 addressing.
    rx_stage_pos: u8,
    rx_stage_len: u8,
    rx_overflow: bool,
    _pad0: u8,
    pub transport_profile: u16,
    rx_len: u16,
    pub session: WireSession,
    pub port: S,
    rx_stage: [u8; 16],
    rx_buf: [u8; MAX_WIRE_BUF_LEN],
}

impl<S: SerialPort> WireEngine<S> {
    /// Initialize a `WireEngine` directly in uninitialized memory without a stack temporary.
    pub unsafe fn init_in_place(dst: *mut Self, port: S) {
        unsafe {
            core::ptr::write_bytes(dst as *mut u8, 0, core::mem::size_of::<Self>());
            core::ptr::write(core::ptr::addr_of_mut!((*dst).port), port);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).session), WireSession::new());
            core::ptr::write(core::ptr::addr_of_mut!((*dst).transport_profile), 0x0101);
        }
    }

    pub const fn new(port: S) -> Self {
        Self::new_with_transport(port, 0x0101)
    }

    pub const fn new_with_transport(port: S, transport_profile: u16) -> Self {
        Self {
            rx_stage_pos: 0,
            rx_stage_len: 0,
            rx_overflow: false,
            _pad0: 0,
            transport_profile,
            rx_len: 0,
            session: WireSession::new(),
            port,
            rx_stage: [0; 16],
            rx_buf: [0; MAX_WIRE_BUF_LEN],
        }
    }

    /// Reset the streaming frame collector, clearing partial frames and line noise.
    pub fn reset_collector(&mut self) {
        self.rx_len = 0;
        self.rx_stage_pos = 0;
        self.rx_stage_len = 0;
        self.rx_overflow = false;
    }

    /// Poll the serial byte stream with terminator-only COBS framing.
    ///
    /// Every `0x00` byte terminates the buffered frame (empty `0x00` runs are silent resyncs).
    /// Leading `0x00` delimiters are accepted cleanly and are never required, so the very
    /// first packet after connect is always decoded without loss.
    pub fn poll(&mut self) -> Option<WireHeader> {
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
                let len = self.rx_len as usize;
                let overflowed = self.rx_overflow;
                self.rx_len = 0;
                self.rx_overflow = false;
                if overflowed || len == 0 {
                    continue;
                }
                let mut hdr_uninit = core::mem::MaybeUninit::<WireHeader>::uninit();
                if let Ok(raw_len) = Cobs::decode_inplace(&mut self.rx_buf, len)
                    && self
                        .session
                        .validate_record(&mut self.rx_buf[..raw_len], unsafe {
                            &mut *hdr_uninit.as_mut_ptr()
                        })
                        == Status::Ok
                {
                    return Some(unsafe { hdr_uninit.assume_init() });
                }
            } else if !self.rx_overflow {
                let len = self.rx_len as usize;
                if len < MAX_WIRE_BUF_LEN {
                    self.rx_buf[len] = b;
                    self.rx_len = (len + 1) as u16;
                } else {
                    self.rx_overflow = true;
                }
            }
        }

        None
    }

    #[inline(always)]
    pub fn current_payload<'a>(&'a self, header: &WireHeader) -> &'a [u8] {
        let covered = HEADER_LEN + header.len as usize;
        &self.rx_buf[HEADER_LEN..covered]
    }

    /// Seal and transmit a response whose payload was written directly into `wire_buf[12..12+plen]`.
    pub fn send_response_in_place(
        &mut self,
        header: &WireHeader,
        status: Status,
        plen: usize,
        wire_buf: &mut [u8; MAX_WIRE_BUF_LEN],
    ) -> Result<usize, Status> {
        let sec_flags = if self.session.is_authenticated {
            header.flags & (FLAG_AUTH | FLAG_ENC)
        } else {
            0
        };
        let (flags, body_len) = if status.is_ok() {
            (FLAG_RESP | sec_flags, plen)
        } else {
            wire_buf[WIRE_PAYLOAD_OFFSET] = status.code();
            (FLAG_RESP | FLAG_ERR | (sec_flags & FLAG_AUTH), 1)
        };
        let frame_len = self.session.seal_wire_buf_in_place(
            header.channel,
            header.op,
            flags,
            header.seq,
            body_len,
            wire_buf,
        )?;
        self.port.write(&wire_buf[..frame_len]);
        Ok(frame_len)
    }

    pub fn send_event(
        &mut self,
        channel: u8,
        op: u8,
        seq: u16,
        payload: &[u8],
    ) -> Result<usize, Status> {
        let mut wire_uninit = core::mem::MaybeUninit::<[u8; MAX_WIRE_BUF_LEN]>::uninit();
        let wire_buf = unsafe { &mut *wire_uninit.as_mut_ptr() };
        let frame_len =
            self.session
                .frame_record_buf(channel, op, FLAG_NO_SEQ, seq, payload, wire_buf)?;
        self.port.write(&wire_buf[..frame_len]);
        Ok(frame_len)
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
        flushes: usize,
    }

    impl MockPort {
        fn new() -> Self {
            Self {
                rx: [0; 256],
                rx_len: 0,
                rx_pos: 0,
                tx_len: 0,
                flushes: 0,
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

        fn flush(&mut self) {
            self.flushes += 1;
        }
    }

    #[test]
    fn test_terminator_only_first_frame_coalesced_and_max_128b_response() {
        let mut session = WireSession::new();
        session.begin_session(0x1122_3344, 128);

        let mut raw = [0u8; MAX_RAW_BUF_LEN];
        let mut wire1 = [0u8; MAX_WIRE_BUF_LEN];
        let mut wire2 = [0u8; MAX_WIRE_BUF_LEN];
        let n1 = session
            .frame_record_raw(
                CH_CONTROL,
                0x01,
                FLAG_ACK_REQ,
                1,
                b"ONE!",
                &mut raw,
                &mut wire1,
            )
            .unwrap();
        let n2 = session
            .frame_record_raw(
                CH_CONTROL,
                0x02,
                FLAG_ACK_REQ,
                2,
                b"TWO!!",
                &mut raw,
                &mut wire2,
            )
            .unwrap();

        let mut port = MockPort::new();
        port.rx[..n1].copy_from_slice(&wire1[..n1]);
        port.rx[n1..n1 + n2].copy_from_slice(&wire2[..n2]);
        port.rx_len = n1 + n2;

        let mut drv = WireEngine::new(port);
        drv.session.begin_session(0x1122_3344, 128);

        let next_frame = |drv: &mut WireEngine<MockPort>| {
            for _ in 0..8 {
                if let Some(f) = drv.poll() {
                    return Some(f);
                }
            }
            None
        };

        let hdr1 = next_frame(&mut drv).expect("first frame without leading 0x00 must decode");
        assert_eq!(hdr1.seq, 1);
        assert_eq!(drv.current_payload(&hdr1), b"ONE!");

        let hdr2 = next_frame(&mut drv).expect("second coalesced frame must decode");
        assert_eq!(hdr2.seq, 2);
        assert_eq!(drv.current_payload(&hdr2), b"TWO!!");

        let mut resp_buf = [0u8; MAX_WIRE_BUF_LEN];
        resp_buf[WIRE_PAYLOAD_OFFSET..WIRE_PAYLOAD_OFFSET + 128].fill(0xA5);
        assert!(
            drv.send_response_in_place(&hdr2, Status::Ok, 128, &mut resp_buf)
                .is_ok()
        );
        assert!(drv.port.tx_len > 128);
    }
}
