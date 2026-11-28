//! Zero-Allocation RigWire v2 Record Validator, Ascon-128 Session Auth & COBS Framer.

use super::header::*;
use abi::Status;
use crypto::ascon::Ascon128;
use crypto::cobs::Cobs;
use crypto::crc::CrcEngine;

fn ascon_mac(key: &[u8; 16], nonce: &[u8; 16], data: &[u8]) -> [u8; 16] {
    Ascon128::new(*key).compute_mac(nonce, &[], data)
}

#[inline(always)]
fn ct_eq_16(a: &[u8; 16], b: &[u8]) -> bool {
    if b.len() != 16 {
        return false;
    }
    let mut diff = 0u8;
    let mut i = 0;
    while i < 16 {
        diff |= a[i] ^ b[i];
        i += 1;
    }
    diff == 0
}

#[repr(C)]
pub struct WireSession {
    pub session_epoch: u32,
    pub expected_seq: u16,
    pub last_seq: u16,
    pub last_crc: u16,
    pub negotiated_max_payload: u16,
    pub rx_highest_seq: u16,
    pub is_authenticated: bool,
    pub require_auth_writes: bool,
    pub rx_replay_window: u32,
    pub session_salt: [u8; 8],
    pub auth_key: [u8; 16],
    pub session_key: [u8; 16],
    _pad: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<WireSession>() == 64);

impl WireSession {
    pub const fn new() -> Self {
        Self {
            session_epoch: 0,
            expected_seq: 0,
            last_seq: 0,
            last_crc: 0,
            negotiated_max_payload: MAX_PAYLOAD_LEN as u16,
            rx_highest_seq: 0,
            is_authenticated: false,
            require_auth_writes: false,
            rx_replay_window: 0,
            session_salt: [0; 8],
            auth_key: [0; 16],
            session_key: [0; 16],
            _pad: [0; 4],
        }
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.session_epoch != 0
    }

    pub fn set_auth_key(&mut self, key: [u8; 16], require_writes: bool) {
        self.auth_key = key;
        self.require_auth_writes = require_writes;
    }

    pub fn reset(&mut self) {
        self.session_epoch = 0;
        self.expected_seq = 0;
        self.last_seq = 0;
        self.last_crc = 0;
        self.negotiated_max_payload = MAX_PAYLOAD_LEN as u16;
        self.rx_highest_seq = 0;
        self.is_authenticated = false;
        self.rx_replay_window = 0;
        self.session_salt = [0; 8];
        self.session_key = [0; 16];
    }

    pub fn begin_session(&mut self, epoch: u32, max_payload: u16) {
        self.session_epoch = epoch.max(1);
        self.expected_seq = 1;
        self.last_seq = 0;
        self.last_crc = 0;
        self.negotiated_max_payload = max_payload.clamp(32, MAX_PAYLOAD_LEN as u16);
        self.rx_highest_seq = 0;
        self.is_authenticated = false;
        self.rx_replay_window = 0;
        self.session_salt = [0; 8];
        self.session_key = [0; 16];
    }

    fn build_frame_nonce(&self, dir: u8, channel: u8, seq: u16) -> [u8; 16] {
        let mut n = [0u8; 16];
        n[..8].copy_from_slice(&self.session_salt);
        n[8] = dir;
        n[9] = channel;
        n[10..14].copy_from_slice(&self.session_epoch.to_le_bytes());
        n[14..16].copy_from_slice(&seq.to_le_bytes());
        n
    }

    fn build_chal_nonce(&self) -> [u8; 16] {
        self.build_frame_nonce(0xFE, 0, 1)
    }

    /// Execute `OP_CORE_AUTH` (`0x00` = Challenge, `0x01` = Verify Proof).
    pub fn handle_auth_step(
        &mut self,
        payload: &[u8],
        entropy: u32,
        out: &mut [u8; 20],
    ) -> Result<usize, Status> {
        if !self.is_active() {
            return Err(Status::BadSession);
        }
        let step = *payload.first().ok_or(Status::BadLength)?;
        match step {
            0 => {
                let mut seed = [0u8; 16];
                seed[..4].copy_from_slice(&self.session_epoch.to_le_bytes());
                seed[4..8].copy_from_slice(&entropy.to_le_bytes());
                if let Some(cn) = payload.get(1..9) {
                    seed[8..16].copy_from_slice(cn);
                }
                let tag = ascon_mac(&self.auth_key, &seed, b"RW2-SALT");
                self.session_salt.copy_from_slice(&tag[..8]);
                self.is_authenticated = false;
                out[0] = 0;
                out[1..9].copy_from_slice(&self.session_salt);
                Ok(9)
            }
            1 => {
                if payload.len() < 17 || self.session_salt == [0; 8] {
                    return Err(Status::BadLength);
                }
                let nonce = self.build_chal_nonce();
                let expected = ascon_mac(&self.auth_key, &nonce, b"KERN-RW2-AUTH");
                if !ct_eq_16(&expected, &payload[1..17]) {
                    self.is_authenticated = false;
                    return Err(Status::PermissionDenied);
                }
                self.session_key = ascon_mac(&self.auth_key, &nonce, b"KERN-RW2-SESS");
                self.is_authenticated = true;
                self.rx_highest_seq = 0;
                self.rx_replay_window = 0;
                let proof = ascon_mac(&self.session_key, &nonce, b"KERN-RW2-OK");
                out[0] = 1;
                out[1..17].copy_from_slice(&proof);
                Ok(17)
            }
            _ => Err(Status::InvalidArg),
        }
    }

    /// Compute the client proof expected by `handle_auth_step` step `1` (used by hosts and tests).
    pub fn compute_client_auth_proof(auth_key: &[u8; 16], salt: &[u8; 8], epoch: u32) -> [u8; 16] {
        let mut nonce = [0u8; 16];
        nonce[..8].copy_from_slice(salt);
        nonce[8] = 0xFE;
        nonce[9] = 0;
        nonce[10..14].copy_from_slice(&epoch.to_le_bytes());
        nonce[14..16].copy_from_slice(&1u16.to_le_bytes());
        ascon_mac(auth_key, &nonce, b"KERN-RW2-AUTH")
    }

    fn check_and_advance_replay(&mut self, seq: u16) -> bool {
        if seq == 0 {
            return false;
        }
        if self.rx_highest_seq == 0 {
            self.rx_highest_seq = seq;
            self.rx_replay_window = 1;
            return true;
        }
        if seq > self.rx_highest_seq {
            let shift = (seq - self.rx_highest_seq) as u32;
            self.rx_replay_window = if shift >= 32 {
                1
            } else {
                (self.rx_replay_window << shift) | 1
            };
            self.rx_highest_seq = seq;
            return true;
        }
        let back = (self.rx_highest_seq - seq) as u32;
        if back >= 32 {
            return false;
        }
        let bit = 1u32 << back;
        if (self.rx_replay_window & bit) != 0 {
            return false;
        }
        self.rx_replay_window |= bit;
        true
    }

    pub fn validate_record(&mut self, raw: &mut [u8], out: &mut WireHeader) -> Status {
        let Some(hdr) = WireHeader::from_bytes(raw) else {
            return Status::BadLength;
        };
        let has_enc = (hdr.flags & FLAG_ENC) != 0;
        let has_auth = has_enc || (hdr.flags & FLAG_AUTH) != 0;
        let tag_len = if has_auth { AUTH_TAG_LEN } else { 0 };
        let covered = HEADER_LEN + hdr.len as usize;
        let crc_off = covered + tag_len;
        if raw.len() != crc_off + CRC_LEN {
            return Status::BadLength;
        }
        if hdr.ident_ver != WIRE_IDENT_V2 {
            return Status::Unsupported;
        }
        if (hdr.flags & (FLAG_RESP | FLAG_ERR)) != 0 || hdr.len > self.negotiated_max_payload {
            return Status::InvalidArg;
        }

        let calc_crc = CrcEngine::crc16_ccitt(&raw[..crc_off]);
        let wire_crc = unsafe {
            let p = raw.as_ptr().add(crc_off);
            u16::from_le_bytes([*p, *p.add(1)])
        };
        if calc_crc != wire_crc {
            return Status::CorruptData;
        }

        if has_auth {
            if !self.is_authenticated || !self.check_and_advance_replay(hdr.seq) {
                return Status::PermissionDenied;
            }
            let mut wire_tag = [0u8; 16];
            wire_tag.copy_from_slice(&raw[covered..crc_off]);
            let calc_tag = self.compute_or_crypt_frame(
                0,
                hdr.channel,
                hdr.seq,
                has_enc,
                true,
                &mut raw[..covered],
            );
            if !ct_eq_16(&calc_tag, &wire_tag) {
                if has_enc {
                    raw[HEADER_LEN..covered].fill(0);
                }
                return Status::PermissionDenied;
            }
        }

        if hdr.channel != CH_CORE {
            if !self.is_active() {
                return Status::BadSession;
            }
            if !hdr.is_no_seq() {
                if !has_auth && hdr.seq == self.last_seq && self.last_seq != 0 {
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
}

/// Offset of the raw `WireHeader` inside a `296 B` COBS staging buffer (`wire_buf[4..12]`).
pub const WIRE_RAW_OFFSET: usize = 4;
/// Offset of the response/request payload inside a `296 B` COBS staging buffer (`wire_buf[12..276]`).
pub const WIRE_PAYLOAD_OFFSET: usize = WIRE_RAW_OFFSET + HEADER_LEN;

impl WireSession {
    fn compute_or_crypt_frame(
        &self,
        dir: u8,
        channel: u8,
        seq: u16,
        has_enc: bool,
        decrypt: bool,
        frame: &mut [u8],
    ) -> [u8; 16] {
        let nonce = self.build_frame_nonce(dir, channel, seq);
        if has_enc {
            let (ad, body) = frame.split_at_mut(HEADER_LEN);
            Ascon128::new(self.session_key).duplex_crypt(&nonce, ad, body, decrypt)
        } else {
            ascon_mac(&self.session_key, &nonce, frame)
        }
    }

    /// Seal a RigWire v2 record in-place where `raw_buf[HEADER_LEN..HEADER_LEN + plen]` already holds the payload.
    pub fn seal_raw_in_place(
        &self,
        channel: u8,
        op: u8,
        flags: u8,
        seq: u16,
        plen: usize,
        raw_buf: &mut [u8],
    ) -> Result<usize, Status> {
        let has_enc = (flags & FLAG_ENC) != 0;
        let has_auth = has_enc || (flags & FLAG_AUTH) != 0;
        if has_auth && !self.is_authenticated {
            return Err(Status::PermissionDenied);
        }
        let tag_len = if has_auth { AUTH_TAG_LEN } else { 0 };
        let body_end = HEADER_LEN + plen;
        let crc_off = body_end + tag_len;
        let raw_len = crc_off + CRC_LEN;
        if plen > u16::MAX as usize || raw_buf.len() < raw_len {
            return Err(Status::BadLength);
        }
        WireHeader {
            ident_ver: WIRE_IDENT_V2,
            flags,
            channel,
            op,
            seq,
            len: plen as u16,
        }
        .write_to(&mut raw_buf[..HEADER_LEN]);

        if has_auth {
            let dir = u8::from(
                (flags & FLAG_RESP) != 0 || ((flags & FLAG_NO_SEQ) != 0 && channel == CH_EVENTS),
            );
            let tag = self.compute_or_crypt_frame(
                dir,
                channel,
                seq,
                has_enc,
                false,
                &mut raw_buf[..body_end],
            );
            raw_buf[body_end..crc_off].copy_from_slice(&tag);
        }

        let crc = CrcEngine::crc16_ccitt(&raw_buf[..crc_off]);
        raw_buf[crc_off..raw_len].copy_from_slice(&crc.to_le_bytes());
        Ok(raw_len)
    }

    fn write_raw_record(
        &self,
        channel: u8,
        op: u8,
        flags: u8,
        seq: u16,
        payload: &[u8],
        raw_buf: &mut [u8],
    ) -> Result<usize, Status> {
        let plen = payload.len();
        let body_end = HEADER_LEN.saturating_add(plen);
        if raw_buf.len() < body_end.saturating_add(CRC_LEN) {
            return Err(Status::BadLength);
        }
        raw_buf[HEADER_LEN..body_end].copy_from_slice(payload);
        self.seal_raw_in_place(channel, op, flags, seq, plen, raw_buf)
    }

    /// Seal and COBS-encode a frame in-place inside `wire_buf` (`payload` already at `wire_buf[12..12+plen]`).
    pub fn seal_wire_buf_in_place(
        &self,
        channel: u8,
        op: u8,
        flags: u8,
        seq: u16,
        plen: usize,
        wire_buf: &mut [u8; 296],
    ) -> Result<usize, Status> {
        let raw_len = self.seal_raw_in_place(
            channel,
            op,
            flags,
            seq,
            plen,
            &mut wire_buf[WIRE_RAW_OFFSET..],
        )?;
        let cobs_len = Cobs::encode_from_offset(wire_buf, WIRE_RAW_OFFSET, raw_len)
            .map_err(|_| Status::BadLength)?;
        wire_buf[cobs_len] = crypto::cobs::COBS_DELIMITER;
        Ok(cobs_len + 1)
    }

    /// Encode a terminator-delimited RigWire v2 frame in-place inside `wire_buf` (`raw_len <= 253`).
    #[cfg(test)]
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
        let raw_len = self.write_raw_record(
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

    /// Encode a terminator-delimited RigWire v2 frame inside a single `296 B` buffer (zero second stack buffer).
    pub fn frame_record_buf(
        &self,
        channel: u8,
        op: u8,
        flags: u8,
        seq: u16,
        payload: &[u8],
        wire_buf: &mut [u8; 296],
    ) -> Result<usize, Status> {
        let plen = payload.len();
        if WIRE_PAYLOAD_OFFSET.saturating_add(plen) > wire_buf.len() - CRC_LEN {
            return Err(Status::BadLength);
        }
        wire_buf[WIRE_PAYLOAD_OFFSET..WIRE_PAYLOAD_OFFSET + plen].copy_from_slice(payload);
        self.seal_wire_buf_in_place(channel, op, flags, seq, plen, wire_buf)
    }

    /// Encode a terminator-delimited RigWire v2 frame of arbitrary MTU (`<= 264 B` payload + optional Ascon tag).
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
        let raw_len = self.write_raw_record(channel, op, flags, seq, payload, raw_buf)?;
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

        session.begin_session(42, 256);

        assert_eq!(session.session_epoch, 42);
        assert_eq!(session.expected_seq, 1);
        assert_eq!(session.last_seq, 0);
        assert_eq!(session.last_crc, 0);
        assert_eq!(session.negotiated_max_payload, 256);
        assert!(!session.is_authenticated);
    }

    #[test]
    fn frame_record_raw_256b_page_and_ascon_auth_enc_roundtrip() {
        let auth_key = *b"0123456789ABCDEF";
        let mut dev = WireSession::new();
        dev.set_auth_key(auth_key, true);
        dev.begin_session(0xCAFE_BABE, 264);

        // Step 0: Challenge
        let mut chal_out = [0u8; 20];
        let n0 = dev
            .handle_auth_step(&[0x00, 1, 2, 3, 4, 5, 6, 7, 8], 0x1234_5678, &mut chal_out)
            .unwrap();
        assert_eq!(n0, 9);
        let mut salt = [0u8; 8];
        salt.copy_from_slice(&chal_out[1..9]);

        // Step 1: Verify Proof
        let proof = WireSession::compute_client_auth_proof(&auth_key, &salt, 0xCAFE_BABE);
        let mut verify_req = [0u8; 17];
        verify_req[0] = 1;
        verify_req[1..17].copy_from_slice(&proof);
        let n1 = dev.handle_auth_step(&verify_req, 0, &mut chal_out).unwrap();
        assert_eq!(n1, 17);
        assert!(dev.is_authenticated);

        // Mirror authenticated session on host
        let mut host = WireSession::new();
        host.begin_session(0xCAFE_BABE, 264);
        host.session_salt = salt;
        host.session_key = dev.session_key;
        host.is_authenticated = true;

        // Send a full 256-byte encrypted + authenticated page frame
        let mut page = [0u8; 256];
        for (i, b) in page.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7) ^ 0x5A;
        }
        let mut raw_buf = [0u8; 300];
        let mut wire_buf = [0u8; 304];
        let wn = host
            .frame_record_raw(
                CH_DATA,
                0x02,
                FLAG_ENC | FLAG_AUTH,
                1,
                &page,
                &mut raw_buf,
                &mut wire_buf,
            )
            .unwrap();
        assert_eq!(wire_buf[wn - 1], 0);

        let dec_len = Cobs::decode_inplace(&mut wire_buf, wn - 1).unwrap();
        let mut hdr = WireHeader {
            ident_ver: 0,
            flags: 0,
            channel: 0,
            op: 0,
            seq: 0,
            len: 0,
        };
        assert_eq!(
            dev.validate_record(&mut wire_buf[..dec_len], &mut hdr),
            Status::Ok
        );
        assert_eq!(hdr.len, 256);
        assert_eq!(&wire_buf[HEADER_LEN..HEADER_LEN + 256], &page[..]);

        // Replay of exact same authenticated frame must be rejected!
        let wn_replay = host
            .frame_record_raw(
                CH_DATA,
                0x02,
                FLAG_ENC | FLAG_AUTH,
                1,
                &page,
                &mut raw_buf,
                &mut wire_buf,
            )
            .unwrap();
        let dec_replay = Cobs::decode_inplace(&mut wire_buf, wn_replay - 1).unwrap();
        assert_eq!(
            dev.validate_record(&mut wire_buf[..dec_replay], &mut hdr),
            Status::PermissionDenied
        );
    }
}
