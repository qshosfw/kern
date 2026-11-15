//! Zero-Allocation Wire Record Parser & Session Validator.

use super::header::*;
use super::ops_core::*;
use crypto::cobs::Cobs;
use crypto::crc::CrcEngine;

pub struct WireSession {
    pub active_session_id: u32,
    pub expected_sequence: u32,
    pub last_sequence: u32,
    pub last_crc: u16,
    pub negotiated_max_payload: u16,
    pub is_authenticated: bool,
    pub bad_crc_count: u32,
    pub malformed_count: u32,
    pub duplicate_count: u32,
    pub valid_records_count: u32,
}

impl WireSession {
    pub const fn new() -> Self {
        Self {
            active_session_id: 0,
            expected_sequence: 0,
            last_sequence: 0,
            last_crc: 0,
            negotiated_max_payload: 128,
            is_authenticated: false,
            bad_crc_count: 0,
            malformed_count: 0,
            duplicate_count: 0,
            valid_records_count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.active_session_id = 0;
        self.expected_sequence = 0;
        self.last_sequence = 0;
        self.last_crc = 0;
        self.negotiated_max_payload = 128;
        self.is_authenticated = false;
    }

    pub fn begin_session(&mut self, session_id: u32, max_payload: u16) {
        self.active_session_id = session_id;
        self.expected_sequence = 1;
        self.last_sequence = 0;
        self.last_crc = 0;
        self.negotiated_max_payload = max_payload;
        self.is_authenticated = false;
    }

    #[inline(never)]
    pub fn validate_record(&mut self, raw_record: &[u8], out: &mut WireHeader) -> WireStatus {
        if raw_record.len() < MIN_RECORD_LEN {
            return WireStatus::BadLength;
        }

        let ident = raw_record[0];
        if ident != WIRE_SYNC_ID {
            return WireStatus::Unsupported;
        }

        let major = raw_record[1];
        if major != WIRE_MAJOR {
            return WireStatus::BadVersion;
        }

        let flags = raw_record[2];
        if flags > 2 {
            return WireStatus::BadFlags;
        }

        let service = raw_record[3];
        let opcode = raw_record[4];
        let payload_len = u16::from_le_bytes([raw_record[13], raw_record[14]]);

        let expected_total_len = HEADER_LEN + payload_len as usize + CRC_LEN;
        if raw_record.len() != expected_total_len {
            return WireStatus::BadLength;
        }

        if payload_len > self.negotiated_max_payload {
            return WireStatus::BadRange;
        }

        // Verify CRC-16/CCITT-FALSE
        let covered_bytes = HEADER_LEN + payload_len as usize;
        let expected_crc = CrcEngine::crc16_ccitt(&raw_record[..covered_bytes]);
        let wire_crc =
            u16::from_le_bytes([raw_record[covered_bytes], raw_record[covered_bytes + 1]]);

        if expected_crc != wire_crc {
            self.bad_crc_count = self.bad_crc_count.saturating_add(1);
            return WireStatus::Failed;
        }

        let session_id =
            u32::from_le_bytes([raw_record[5], raw_record[6], raw_record[7], raw_record[8]]);
        let sequence = u32::from_le_bytes([
            raw_record[9],
            raw_record[10],
            raw_record[11],
            raw_record[12],
        ]);

        // Validate session correlation
        if service == SERVICE_CORE {
            if opcode == OP_CORE_HELLO {
                // HELLO always establishes or resets the session
            } else if self.active_session_id == 0
                || session_id == 0
                || session_id == self.active_session_id
            {
                // Core discovery, ping, status, and time sync operations are permitted
                // before full session establishment or with session_id=0
            } else {
                return WireStatus::BadSession;
            }
        } else {
            if self.active_session_id == 0 || session_id != self.active_session_id {
                return WireStatus::BadSession;
            }

            if sequence == self.last_sequence && self.last_sequence != 0 {
                if expected_crc != self.last_crc {
                    return WireStatus::DuplicateConflict;
                }
                self.duplicate_count = self.duplicate_count.saturating_add(1);
            } else if sequence != self.expected_sequence {
                return WireStatus::StaleSequence;
            }

            self.last_sequence = sequence;
            self.last_crc = expected_crc;
            if sequence == self.expected_sequence {
                self.expected_sequence = self.expected_sequence.wrapping_add(1);
            }
        }

        self.valid_records_count = self.valid_records_count.saturating_add(1);
        *out = WireHeader {
            ident,
            major,
            flags,
            service,
            opcode,
            session_id,
            sequence,
            payload_len,
        };
        WireStatus::Ok
    }

    #[inline(never)]
    pub fn frame_record_raw(
        &self,
        service: u8,
        opcode: u8,
        flags: u8,
        sequence: u32,
        status_byte: Option<u8>,
        payload_data: &[u8],
        raw_buf: &mut [u8],
        wire_out: &mut [u8],
    ) -> Result<usize, WireStatus> {
        let app_payload_len = status_byte.map_or(0, |_| 1) + payload_data.len();
        if app_payload_len > u16::MAX as usize {
            return Err(WireStatus::BadLength);
        }
        let body_end = HEADER_LEN + app_payload_len;
        let raw_len = body_end + CRC_LEN;
        let required_wire_len = Cobs::max_encoded_len(raw_len) + 2;

        if raw_buf.len() < raw_len || wire_out.len() < required_wire_len {
            return Err(WireStatus::BadLength);
        }

        let buf = &mut raw_buf[..raw_len];
        let [s0, s1, s2, s3] = self.active_session_id.to_le_bytes();
        let [q0, q1, q2, q3] = sequence.to_le_bytes();
        let [l0, l1] = (app_payload_len as u16).to_le_bytes();
        buf[0] = WIRE_SYNC_ID;
        buf[1] = WIRE_MAJOR;
        buf[2] = flags;
        buf[3] = service;
        buf[4] = opcode;
        buf[5] = s0;
        buf[6] = s1;
        buf[7] = s2;
        buf[8] = s3;
        buf[9] = q0;
        buf[10] = q1;
        buf[11] = q2;
        buf[12] = q3;
        buf[13] = l0;
        buf[14] = l1;
        let payload_start = if let Some(s) = status_byte {
            buf[HEADER_LEN] = s;
            HEADER_LEN + 1
        } else {
            HEADER_LEN
        };
        buf[payload_start..body_end].copy_from_slice(payload_data);

        let crc = CrcEngine::crc16_ccitt(&buf[..body_end]);
        let [c0, c1] = crc.to_le_bytes();
        buf[body_end] = c0;
        buf[body_end + 1] = c1;

        wire_out[0] = crypto::cobs::COBS_DELIMITER;
        let cobs_len = Cobs::encode(&raw_buf[..raw_len], &mut wire_out[1..])
            .map_err(|_| WireStatus::BadLength)?;
        wire_out[1 + cobs_len] = crypto::cobs::COBS_DELIMITER;

        Ok(2 + cobs_len)
    }

    #[inline(always)]
    pub fn frame_response(
        &self,
        service: u8,
        opcode: u8,
        sequence: u32,
        status: WireStatus,
        payload_data: &[u8],
        raw_buf: &mut [u8],
        wire_out: &mut [u8],
    ) -> Result<usize, WireStatus> {
        self.frame_record_raw(
            service,
            opcode,
            FLAG_RESPONSE,
            sequence,
            Some(status.to_u8()),
            payload_data,
            raw_buf,
            wire_out,
        )
    }

    #[inline(always)]
    pub fn frame_event(
        &self,
        service: u8,
        opcode: u8,
        sequence: u32,
        payload_data: &[u8],
        raw_buf: &mut [u8],
        wire_out: &mut [u8],
    ) -> Result<usize, WireStatus> {
        self.frame_record_raw(
            service,
            opcode,
            FLAG_EVENT,
            sequence,
            None,
            payload_data,
            raw_buf,
            wire_out,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_session_clears_auth_and_stale_crc() {
        let mut session = WireSession::new();
        session.active_session_id = 11;
        session.last_sequence = 9;
        session.last_crc = 0xBEEF;
        session.is_authenticated = true;

        session.begin_session(42, 64);

        assert_eq!(session.active_session_id, 42);
        assert_eq!(session.expected_sequence, 1);
        assert_eq!(session.last_sequence, 0);
        assert_eq!(session.last_crc, 0);
        assert_eq!(session.negotiated_max_payload, 64);
        assert!(!session.is_authenticated);
    }

    #[test]
    fn frame_record_raw_rejects_short_wire_out_above_254_bytes() {
        let session = WireSession::new();
        let payload = [0xAAu8; 243]; // raw_len = 15 + 243 + 2 = 260 >= 254 (max COBS = 262)
        let mut raw_buf = [0u8; 260];
        let mut short_wire_out = [0u8; 263]; // previously passed raw_len + 3 and panicked

        let res = session.frame_event(
            SERVICE_DATA,
            0x01,
            1,
            &payload,
            &mut raw_buf,
            &mut short_wire_out,
        );
        assert_eq!(res, Err(WireStatus::BadLength));

        let mut ok_wire_out = [0u8; 264];
        let ok = session.frame_event(
            SERVICE_DATA,
            0x01,
            1,
            &payload,
            &mut raw_buf,
            &mut ok_wire_out,
        );
        assert!(ok.is_ok());
    }
}
