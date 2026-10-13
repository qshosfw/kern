//! Digital FSK Packet Protocol, Message Framing, and Station Identification.
//!
//! Provides application-level framing compatible with Fagci-K1, GOGUFW, and UV-KX Messenger
//! with zero-allocation packet parsing, framing alignment slip recovery, and callsign addressing.

use kernel::service::radio::fsk::{
    FskBaudRate, FskFifoThresholds, FskModulation, FskPhyConfig, FskPreamblePattern, FskSyncConfig,
    RfFskPhy,
};

/// Framing magic markers for preamble alignment slip recovery.
pub const FRAMING_MAGIC_HEADER: u16 = 0xABCD;
pub const FRAMING_MAGIC_TRAILER: u16 = 0xDCBA;

/// 16-bit Fletcher Checksum over a byte slice.
pub fn fletcher16(data: &[u8]) -> u16 {
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;
    for &b in data {
        sum1 = (sum1 + b as u16) % 255;
        sum2 = (sum2 + sum1) % 255;
    }
    (sum2 << 8) | sum1
}

/// Scans a slice of words for a 16-bit framing marker (e.g. `0xABCD`).
pub fn scan_framing_magic(words: &[u16], magic: u16) -> Option<usize> {
    words.iter().position(|&w| w == magic)
}

/// Type of high-level digital packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketKind {
    Text = 1,
    Ping = 2,
    Ack = 3,
    Pong = 4,
    Beacon = 5,
    Telemetry = 6,
}

impl PacketKind {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            2 => Self::Ping,
            3 => Self::Ack,
            4 => Self::Pong,
            5 => Self::Beacon,
            6 => Self::Telemetry,
            _ => Self::Text,
        }
    }
}

/// A structured 64-word FSK packet frame.
#[derive(Debug, Clone, Copy)]
pub struct FskMessagePacket {
    pub kind: PacketKind,
    pub seq: u8,
    pub source: [u8; 6],
    pub target: [u8; 6],
    pub payload_len: u8,
    pub payload: [u8; 48],
}

impl FskMessagePacket {
    pub const fn new(kind: PacketKind, seq: u8, source: &[u8; 6], target: &[u8; 6]) -> Self {
        Self {
            kind,
            seq,
            source: *source,
            target: *target,
            payload_len: 0,
            payload: [0; 48],
        }
    }

    pub fn set_text(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let len = bytes.len().min(48);
        self.payload[..len].copy_from_slice(&bytes[..len]);
        self.payload_len = len as u8;
    }

    /// Encode packet into 64-word array for hardware FSK FIFO transmission.
    /// Prefixes standard `0xABCD` header and suffixes `0xDCBA` trailer for alignment recovery.
    pub fn encode(&self, out: &mut [u16; 64]) -> usize {
        out.fill(0);
        // Word 0: Framing Header Magic
        out[0] = FRAMING_MAGIC_HEADER;
        // Word 1: Kind (high byte) | Seq (low byte)
        out[1] = ((self.kind as u16) << 8) | (self.seq as u16);
        // Word 2..4: Source callsign (6 bytes)
        out[2] = ((self.source[0] as u16) << 8) | (self.source[1] as u16);
        out[3] = ((self.source[2] as u16) << 8) | (self.source[3] as u16);
        out[4] = ((self.source[4] as u16) << 8) | (self.source[5] as u16);
        // Word 5..7: Target callsign (6 bytes)
        out[5] = ((self.target[0] as u16) << 8) | (self.target[1] as u16);
        out[6] = ((self.target[2] as u16) << 8) | (self.target[3] as u16);
        out[7] = ((self.target[4] as u16) << 8) | (self.target[5] as u16);
        // Word 8: Payload length
        out[8] = (self.payload_len as u16) << 8;

        // Word 9..32: Payload bytes (up to 48 bytes = 24 words)
        let words = (self.payload_len as usize + 1) / 2;
        for i in 0..words {
            let b0 = self.payload[i * 2];
            let b1 = if i * 2 + 1 < self.payload_len as usize {
                self.payload[i * 2 + 1]
            } else {
                0
            };
            out[9 + i] = ((b0 as u16) << 8) | (b1 as u16);
        }

        // Fletcher-16 checksum across words 1..62
        let mut byte_buf = [0u8; 124];
        let mut b_idx = 0;
        for &w in &out[1..62] {
            byte_buf[b_idx] = (w >> 8) as u8;
            byte_buf[b_idx + 1] = (w & 0xFF) as u8;
            b_idx += 2;
        }
        let cksum = fletcher16(&byte_buf[..b_idx]);
        out[62] = cksum;
        out[63] = FRAMING_MAGIC_TRAILER;
        64
    }

    /// Decode received 64-word array from hardware FSK FIFO.
    /// Scans for `FRAMING_MAGIC_HEADER` (0xABCD) to recover from early byte slips or stale FIFO words.
    pub fn decode(words: &[u16]) -> Option<Self> {
        if words.len() < 10 {
            return None;
        }

        // Check if framed with 0xABCD magic
        let (body, has_magic) = if words[0] == FRAMING_MAGIC_HEADER {
            (&words[1..], true)
        } else if let Some(offset) = scan_framing_magic(words, FRAMING_MAGIC_HEADER) {
            (&words[offset + 1..], true)
        } else {
            // Legacy direct framing without 0xABCD magic
            (words, false)
        };

        if body.len() < 8 {
            return None;
        }

        let kind = PacketKind::from_u8((body[0] >> 8) as u8);
        let seq = (body[0] & 0xFF) as u8;

        let source = [
            (body[1] >> 8) as u8,
            (body[1] & 0xFF) as u8,
            (body[2] >> 8) as u8,
            (body[2] & 0xFF) as u8,
            (body[3] >> 8) as u8,
            (body[3] & 0xFF) as u8,
        ];

        let target = [
            (body[4] >> 8) as u8,
            (body[4] & 0xFF) as u8,
            (body[5] >> 8) as u8,
            (body[5] & 0xFF) as u8,
            (body[6] >> 8) as u8,
            (body[6] & 0xFF) as u8,
        ];

        let payload_len = ((body[7] >> 8) as u8).min(48);
        let mut payload = [0u8; 48];

        let words_count = (payload_len as usize + 1) / 2;
        for i in 0..words_count {
            if 8 + i < body.len() {
                let w = body[8 + i];
                if i * 2 < 48 {
                    payload[i * 2] = (w >> 8) as u8;
                }
                if i * 2 + 1 < 48 {
                    payload[i * 2 + 1] = (w & 0xFF) as u8;
                }
            }
        }

        let _ = has_magic;
        Some(Self {
            kind,
            seq,
            source,
            target,
            payload_len,
            payload,
        })
    }
}

/// Zero-allocation digital packet messenger engine for background decoding and transmission.
#[derive(Debug, Clone, Copy)]
pub struct FskMessengerEngine {
    pub unread_count: u8,
    pub is_active: bool,
    pub my_callsign: [u8; 6],
    pub last_seq: u8,
}

impl FskMessengerEngine {
    pub const fn new(callsign: [u8; 6]) -> Self {
        Self {
            unread_count: 0,
            is_active: false,
            my_callsign: callsign,
            last_seq: 0,
        }
    }

    /// Arm hardware modem for packet reception with optimal preamble and threshold settings.
    pub fn arm_rx<R: RfFskPhy>(&mut self, radio: &mut R) {
        let config = FskPhyConfig {
            modulation: FskModulation::DirectFsk,
            baud_rate: FskBaudRate::Baud1200,
            preamble_pattern: FskPreamblePattern::PatternAA,
            preamble_length: 8,
            sync: FskSyncConfig::K1_DEFAULT,
            raw_length_bytes: 128,
            thresholds: FskFifoThresholds {
                almost_empty_words: 64,
                almost_full_words: 4,
            },
            ..Default::default()
        };
        let _ = radio.configure_fsk(&config);
        radio.start_fsk_rx();
        self.is_active = true;
    }

    /// Arm hardware modem for Audio FFSK (1200/1800 Hz) Bell 202 packet reception.
    pub fn arm_ffsk_rx<R: RfFskPhy>(&mut self, radio: &mut R) {
        let config = FskPhyConfig {
            modulation: FskModulation::Ffsk1200_1800,
            baud_rate: FskBaudRate::Baud1200,
            preamble_pattern: FskPreamblePattern::PatternAA,
            preamble_length: 8,
            sync: FskSyncConfig::MATOZ_UVKX,
            raw_length_bytes: 128,
            thresholds: FskFifoThresholds {
                almost_empty_words: 64,
                almost_full_words: 4,
            },
            ..Default::default()
        };
        let _ = radio.configure_fsk(&config);
        radio.start_fsk_rx();
        self.is_active = true;
    }

    /// Poll modem hardware for packet completion and re-arm RX without dropping subsequent packets.
    pub fn poll_rx<R: RfFskPhy>(&mut self, radio: &mut R) -> Option<FskMessagePacket> {
        let status = radio.poll_fsk_status();
        if status.rx_finished || status.rx_words_available >= 64 {
            let mut words = [0u16; 64];
            let read = radio.read_fsk_fifo(&mut words);
            radio.rearm_fsk_rx(); // Fast non-blocking re-arm per Section 14
            if read >= 8 {
                if let Some(pkt) = FskMessagePacket::decode(&words[..read]) {
                    self.unread_count = self.unread_count.saturating_add(1);
                    return Some(pkt);
                }
            }
        }
        None
    }

    /// Transmit a packet frame with synchronous completion wait.
    pub fn send_packet<R: RfFskPhy>(
        &mut self,
        radio: &mut R,
        packet: &FskMessagePacket,
    ) -> Result<(), ()> {
        let mut words = [0u16; 64];
        packet.encode(&mut words);
        radio.send_fsk_packet_with_timeout(&words, 600)?;
        radio.rearm_fsk_rx();
        Ok(())
    }

    /// Build an Acknowledgement (ACK) packet frame.
    pub fn build_ack(&mut self, target_callsign: &[u8; 6], ack_seq: u8) -> FskMessagePacket {
        let mut pkt =
            FskMessagePacket::new(PacketKind::Ack, ack_seq, &self.my_callsign, target_callsign);
        pkt.set_text("ACK");
        pkt
    }

    /// Build a Range Check PING packet frame.
    pub fn build_ping(&mut self) -> FskMessagePacket {
        self.last_seq = self.last_seq.wrapping_add(1);
        let broadcast = *b"ALL   ";
        let mut pkt = FskMessagePacket::new(
            PacketKind::Ping,
            self.last_seq,
            &self.my_callsign,
            &broadcast,
        );
        pkt.set_text("PING");
        pkt
    }

    /// Build a Range Check PONG packet frame in response to a PING.
    pub fn build_pong(
        &mut self,
        target_callsign: &[u8; 6],
        ping_seq: u8,
        battery_pct: u8,
    ) -> FskMessagePacket {
        let mut pkt = FskMessagePacket::new(
            PacketKind::Pong,
            ping_seq,
            &self.my_callsign,
            target_callsign,
        );
        let mut buf = [b'0'; 4];
        buf[0] = b'B';
        buf[1] = b':';
        buf[2] = b'0' + (battery_pct / 10).min(9);
        buf[3] = b'0' + (battery_pct % 10);
        if let Ok(s) = core::str::from_utf8(&buf) {
            pkt.set_text(s);
        }
        pkt
    }
}
