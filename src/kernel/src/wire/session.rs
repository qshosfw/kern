//! Zero-Allocation RigWire v2 Record Validator & In-Place COBS Framer.

use super::header::*;
use abi::Status;
use crypto::cobs::Cobs;
use crypto::crc::CrcEngine;

#[repr(C)]
pub struct WireSession {
    pub session_epoch: u32,
    pub expected_seq: u16,
    pub last_seq: u16,
    pub last_crc: u16,
    pub negotiated_max_payload: u16,
    pub is_authenticated: bool,
    _pad: [u8; 3],
}

const _: () = assert!(core::mem::size_of::<WireSession>() == 16);

impl WireSession {
    pub const fn new() -> Self {
        Self {
            session_epoch: 0,
            expected_seq: 0,
            last_seq: 0,
            last_crc: 0,
            negotiated_max_payload: 128,
            is_authenticated: false,
            _pad: [0; 3],
        }
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.session_epoch != 0
    }

    pub fn reset(&mut self) {
        self.session_epoch = 0;
        self.expected_seq = 0;
        self.last_seq = 0;
        self.last_crc = 0;
        self.negotiated_max_payload = 128;
        self.is_authenticated = false;
    }

    pub fn begin_session(&mut self, epoch: u32, max_payload: u16) {
        self.session_epoch = epoch.max(1);
        self.expected_seq = 1;
        self.last_seq = 0;
        self.last_crc = 0;
        self.negotiated_max_payload = max_payload;
        self.is_authenticated = false;
    }

    #[inline(never)]
    pub fn validate_record(&mut self, raw: &[u8], out: &mut WireHeader) -> Status {
        let Some(hdr) = WireHeader::from_bytes(raw) else {
            return Status::BadLength;
        };
        let covered = HEADER_LEN + hdr.len as usize;
        if raw.len() != covered + CRC_LEN {
            return Status::BadLength;
        }
        if hdr.ident_ver != WIRE_IDENT_V2 {
            return Status::Unsupported;
        }
        if (hdr.flags & (FLAG_RESP | FLAG_ERR)) != 0 || hdr.len > self.negotiated_max_payload {
            return Status::InvalidArg;
        }

        let calc_crc = CrcEngine::crc16_ccitt(&raw[..covered]);
        let wire_crc = unsafe {
            let p = raw.as_ptr().add(covered);
            u16::from_le_bytes([*p, *p.add(1)])
        };
        if calc_crc != wire_crc {
            return Status::CorruptData;
        }

        if hdr.channel != CH_CORE {
            if !self.is_active() {
                return Status::BadSession;
            }
            if !hdr.is_no_seq() {
                if hdr.seq == self.last_seq && self.last_seq != 0 {
                    if calc_crc != self.last_crc {
                        return Status::InvalidArg;
                    }
                } else if hdr.seq != self.expected_seq {
                    return Status::BadSession;
                }
                self.last_seq = hdr.seq;
                self.last_crc = calc_crc;
                if hdr.seq == self.expected_seq {
                    self.expected_seq = self.expected_seq.wrapping_add(1).max(1);
                }
            }
        }

        *out = hdr;
        Status::Ok
    }

    #[inline(always)]
    fn write_raw_record(
        channel: u8,
        op: u8,
        flags: u8,
        seq: u16,
        payload: &[u8],
        raw_buf: &mut [u8],
    ) -> Result<usize, Status> {
        let plen = payload.len();
        let body_end = HEADER_LEN + plen;
        let raw_len = body_end + CRC_LEN;
        if plen > u16::MAX as usize || raw_buf.len() < raw_len {
            return Err(Status::BadLength);
        }
        let hdr = WireHeader {
            ident_ver: WIRE_IDENT_V2,
            flags,
            channel,
            op,
            seq,
            len: plen as u16,
        };
        unsafe {
            let p = raw_buf.as_mut_ptr();
            hdr.write_to(core::slice::from_raw_parts_mut(p, HEADER_LEN));
            core::ptr::copy_nonoverlapping(payload.as_ptr(), p.add(HEADER_LEN), plen);
            let crc = CrcEngine::crc16_ccitt(core::slice::from_raw_parts(p, body_end));
            let [c0, c1] = crc.to_le_bytes();
            *p.add(body_end) = c0;
            *p.add(body_end + 1) = c1;
        }
        Ok(raw_len)
    }

    /// Encode a terminator-delimited RigWire v2 frame in-place inside `wire_buf` (zero extra copies).
    /// Layout: `[COBS_BODY..., 0x00]`. Requires `raw_len <= 253` and `wire_buf.len() >= raw_len + 2`.
    #[inline(never)]
    pub fn frame_record_inplace(
        &self,
        channel: u8,
        op: u8,
        flags: u8,
        seq: u16,
        payload: &[u8],
        wire_buf: &mut [u8],
    ) -> Result<usize, Status> {
        if wire_buf.len() < MIN_RECORD_LEN + 2 {
            return Err(Status::BadLength);
        }
        let max_raw = (wire_buf.len() - 2).min(253);
        let raw_len = Self::write_raw_record(
            channel,
            op,
            flags,
            seq,
            payload,
            &mut wire_buf[1..1 + max_raw],
        )?;
        let cobs_len =
            Cobs::encode_inplace_short(wire_buf, raw_len).map_err(|_| Status::BadLength)?;
        wire_buf[cobs_len] = crypto::cobs::COBS_DELIMITER;
        Ok(cobs_len + 1)
    }

    #[inline(never)]
    pub fn frame_record_raw(
        &self,
        channel: u8,
        op: u8,
        flags: u8,
        seq: u16,
        payload: &[u8],
        raw_buf: &mut [u8],
        wire_out: &mut [u8],
    ) -> Result<usize, Status> {
        let raw_len = Self::write_raw_record(channel, op, flags, seq, payload, raw_buf)?;
        let required = Cobs::max_encoded_len(raw_len) + 1;
        if wire_out.len() < required {
            return Err(Status::BadLength);
        }
        let cobs_len =
            Cobs::encode(&raw_buf[..raw_len], wire_out).map_err(|_| Status::BadLength)?;
        wire_out[cobs_len] = crypto::cobs::COBS_DELIMITER;
        Ok(cobs_len + 1)
    }

    #[inline(always)]
    pub fn frame_response(
        &self,
        channel: u8,
        op: u8,
        seq: u16,
        status: Status,
        payload: &[u8],
        raw_buf: &mut [u8],
        wire_out: &mut [u8],
    ) -> Result<usize, Status> {
        let err_byte = [status.code()];
        let (flags, body) = if status.is_ok() {
            (FLAG_RESP, payload)
        } else {
            (FLAG_RESP | FLAG_ERR, &err_byte[..])
        };
        self.frame_record_raw(channel, op, flags, seq, body, raw_buf, wire_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_session_clears_auth_and_stale_crc() {
        let mut session = WireSession::new();
        session.session_epoch = 11;
        session.last_seq = 9;
        session.last_crc = 0xBEEF;
        session.is_authenticated = true;

        session.begin_session(42, 128);

        assert_eq!(session.session_epoch, 42);
        assert_eq!(session.expected_seq, 1);
        assert_eq!(session.last_seq, 0);
        assert_eq!(session.last_crc, 0);
        assert_eq!(session.negotiated_max_payload, 128);
        assert!(!session.is_authenticated);
    }

    #[test]
    fn frame_record_raw_and_inplace_roundtrip_with_status() {
        let mut session = WireSession::new();
        session.begin_session(1, 245);
        let payload = [0xAAu8; 244]; // raw_len = 8 + 244 + 2 = 254 (max COBS = 256, +1 term = 257)
        let mut raw_buf = [0u8; 256];
        let mut short_out = [0u8; 256];
        assert_eq!(
            session.frame_record_raw(
                CH_DATA,
                0x01,
                FLAG_NO_SEQ,
                1,
                &payload,
                &mut raw_buf,
                &mut short_out,
            ),
            Err(Status::BadLength)
        );

        let mut ok_out = [0u8; 257];
        let n = session
            .frame_record_raw(
                CH_DATA,
                0x01,
                FLAG_NO_SEQ,
                1,
                &payload,
                &mut raw_buf,
                &mut ok_out,
            )
            .unwrap();
        assert_eq!(ok_out[n - 1], 0);
        let dec = Cobs::decode_inplace(&mut ok_out, n - 1).unwrap();
        let mut hdr = WireHeader {
            ident_ver: 0,
            flags: 0,
            channel: 0,
            op: 0,
            seq: 0,
            len: 0,
        };
        assert_eq!(session.validate_record(&ok_out[..dec], &mut hdr), Status::Ok);
        assert_eq!(hdr.len, 244);

        // In-place error frame encodes Status::PermissionDenied with FLAG_ERR.
        let mut wire = [0u8; 64];
        let err_b = [Status::PermissionDenied.code()];
        let en = session
            .frame_record_inplace(CH_CONTROL, 0x02, FLAG_RESP | FLAG_ERR, 7, &err_b, &mut wire)
            .unwrap();
        assert_eq!(wire[en - 1], 0);
        let elen = Cobs::decode_inplace(&mut wire, en - 1).unwrap();
        let eh = WireHeader::from_bytes(&wire[..elen]).unwrap();
        assert!(eh.is_response() && eh.is_error());
        assert_eq!(eh.seq, 7);
        assert_eq!(eh.len, 1);
        assert_eq!(wire[HEADER_LEN], Status::PermissionDenied.code());
    }
}
