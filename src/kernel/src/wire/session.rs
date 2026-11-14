//! Zero-Allocation Wire Record Parser & Session Validator.

use super::header::*;
use super::ops_core::*;
use crypto::cobs::Cobs;
use crypto::crc::CrcEngine;
use utils::bytes::ByteWriter;

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

    pub fn validate_record<'a>(
        &mut self,
        raw_record: &'a [u8],
    ) -> Result<(WireHeader, &'a [u8]), WireStatus> {
        if raw_record.len() < MIN_RECORD_LEN {
            return Err(WireStatus::BadLength);
        }

        let ident = raw_record[0];
        if ident != WIRE_SYNC_ID {
            return Err(WireStatus::Unsupported);
        }

        let major = raw_record[1];
        if major != WIRE_MAJOR {
            return Err(WireStatus::BadVersion);
        }

        let flags = raw_record[2];
        if (flags & 0xFC) != 0 || ((flags & FLAG_RESPONSE) != 0 && (flags & FLAG_EVENT) != 0) {
            return Err(WireStatus::BadFlags);
        }

        let service = raw_record[3];
        let opcode = raw_record[4];
        let session_id =
            u32::from_le_bytes([raw_record[5], raw_record[6], raw_record[7], raw_record[8]]);
        let sequence = u32::from_le_bytes([
            raw_record[9],
            raw_record[10],
            raw_record[11],
            raw_record[12],
        ]);
        let payload_len = u16::from_le_bytes([raw_record[13], raw_record[14]]);

        let expected_total_len = HEADER_LEN + payload_len as usize + CRC_LEN;
        if raw_record.len() != expected_total_len {
            return Err(WireStatus::BadLength);
        }

        if payload_len > self.negotiated_max_payload {
            return Err(WireStatus::BadRange);
        }

        // Verify CRC-16/CCITT-FALSE
        let covered_bytes = HEADER_LEN + payload_len as usize;
        let expected_crc = CrcEngine::crc16_ccitt(&raw_record[..covered_bytes]);
        let wire_crc =
            u16::from_le_bytes([raw_record[covered_bytes], raw_record[covered_bytes + 1]]);

        if expected_crc != wire_crc {
            self.bad_crc_count = self.bad_crc_count.saturating_add(1);
            return Err(WireStatus::Failed);
        }

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
                return Err(WireStatus::BadSession);
            }
        } else {
            if self.active_session_id == 0 || session_id != self.active_session_id {
                return Err(WireStatus::BadSession);
            }

            if sequence == self.last_sequence && self.last_sequence != 0 {
                if expected_crc != self.last_crc {
                    return Err(WireStatus::DuplicateConflict);
                }
                self.duplicate_count = self.duplicate_count.saturating_add(1);
            } else if sequence != self.expected_sequence {
                return Err(WireStatus::StaleSequence);
            }

            self.last_sequence = sequence;
            self.last_crc = expected_crc;
            if sequence == self.expected_sequence {
                self.expected_sequence = self.expected_sequence.wrapping_add(1);
            }
        }

        self.valid_records_count = self.valid_records_count.saturating_add(1);
        let header = WireHeader {
            ident,
            major,
            flags,
            service,
            opcode,
            session_id,
            sequence,
            payload_len,
        };
        let payload = &raw_record[HEADER_LEN..covered_bytes];
        Ok((header, payload))
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
        let raw_len = HEADER_LEN + app_payload_len + CRC_LEN;

        if raw_buf.len() < raw_len || wire_out.len() < raw_len + 3 {
            return Err(WireStatus::BadLength);
        }

        {
            let mut w = ByteWriter::new(&mut raw_buf[..raw_len]);
            w.write_u8(WIRE_SYNC_ID);
            w.write_u8(WIRE_MAJOR);
            w.write_u8(flags);
            w.write_u8(service);
            w.write_u8(opcode);
            w.write_u32_le(self.active_session_id);
            w.write_u32_le(sequence);
            w.write_u16_le(app_payload_len as u16);
            if let Some(s) = status_byte {
                w.write_u8(s);
            }
            w.write_bytes(payload_data);
        }

        let crc = CrcEngine::crc16_ccitt(&raw_buf[..HEADER_LEN + app_payload_len]);
        let mut crc_w = ByteWriter::new(&mut raw_buf[HEADER_LEN + app_payload_len..raw_len]);
        crc_w.write_u16_le(crc);

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
