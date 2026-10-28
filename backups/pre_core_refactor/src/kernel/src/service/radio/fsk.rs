//! Hardware Frequency-Shift Keying (FSK) Physical Layer Controller & Utilities.
//!
//! Provides granular, hardware-level physical layer control over the BK4819 / BK4829
//! modem hardware (registers 0x58..0x5F, 0x70, 0x72), FIFO controllers, preamble
//! generators, sync correlators, and bit-level stream utilities.
//!
//! Strictly physical-layer oriented: contains zero packet headers, envelopes, or checksums.

/// Physical FSK modulation profile on BK4819 / BK4829 transceiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FskModulation {
    /// Direct FM baseband FSK without audio subcarrier (1.2K .. 9.6K).
    DirectFsk,
    /// Fast Frequency Shift Keying with 1200 / 1800 Hz audio tones (Bell 202 / APRS).
    Ffsk1200_1800,
    /// Fast Frequency Shift Keying with 1200 / 2400 Hz audio tones.
    Ffsk1200_2400,
    /// NOAA Specific Area Message Encoding (SAME) 1050 Hz alert FSK.
    NoaaSame,
}

impl Default for FskModulation {
    fn default() -> Self {
        Self::DirectFsk
    }
}

/// Standard FSK modem transmission baud rates and custom clock divider options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FskBaudRate {
    Baud1200,
    Baud2400,
    Baud4800,
    Baud9600,
    /// Raw clock divider written directly to register 0x72.
    CustomDivider(u16),
}

impl Default for FskBaudRate {
    fn default() -> Self {
        Self::Baud1200
    }
}

impl FskBaudRate {
    pub const fn in_bps(self) -> u16 {
        match self {
            Self::Baud1200 => 1200,
            Self::Baud2400 => 2400,
            Self::Baud4800 => 4800,
            Self::Baud9600 => 9600,
            Self::CustomDivider(_) => 0,
        }
    }

    /// Calculate register 0x72 clock divider.
    pub const fn divider_word(self) -> u16 {
        match self {
            Self::Baud1200 => 0x3065, // Gold-standard 1200 baud divider
            Self::Baud2400 => 0x1832,
            Self::Baud4800 => 0x0C19,
            Self::Baud9600 => 0x060C,
            Self::CustomDivider(div) => div,
        }
    }
}

/// Preamble bit pattern selection (Register 0x58 bits <5:4>).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FskPreamblePattern {
    /// Auto select 0xAA or 0x55 depending on the MSB of sync byte 0.
    Auto,
    /// Explicit 0x55 pattern.
    Pattern55,
    /// Explicit 0xAA pattern.
    PatternAA,
}

impl Default for FskPreamblePattern {
    fn default() -> Self {
        Self::Auto
    }
}

/// Hardware receiver channel bandwidth filter (Register 0x58 bits <3:1>).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FskRxBandwidth {
    Bw1k2,
    Bw1k2_1k8,
    BwNoaa,
    Bw2k4,
}

impl Default for FskRxBandwidth {
    fn default() -> Self {
        Self::Bw1k2
    }
}

/// Hardware FSK receiver front-end gain (Register 0x58 bits <9:8>).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FskRxGain {
    Gain0 = 0,
    Gain1 = 1,
    Gain2 = 2,
    Gain3 = 3, // Maximum gain
}

impl Default for FskRxGain {
    fn default() -> Self {
        Self::Gain3
    }
}

/// Transmit and receive bit polarity inversion (Register 0x59 bits <10:9>).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FskPolarity {
    Normal,
    InvertTx,
    InvertRx,
    InvertBoth,
}

impl Default for FskPolarity {
    fn default() -> Self {
        Self::Normal
    }
}

/// Hardware frame synchronization word and bit correlator settings (Registers 0x59..0x5B).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FskSyncConfig {
    pub bytes: [u8; 4],
    /// Sync word length: 2 or 4 bytes.
    pub length: u8,
    /// Bit error tolerance in sync correlator: 0..=7 bit errors allowed.
    pub error_tolerance: u8,
}

impl Default for FskSyncConfig {
    fn default() -> Self {
        Self {
            bytes: [0x85, 0xCF, 0xAB, 0x45],
            length: 4,
            error_tolerance: 0,
        }
    }
}

impl FskSyncConfig {
    pub const K1_DEFAULT: Self = Self {
        bytes: [0x85, 0xCF, 0xAB, 0x45],
        length: 4,
        error_tolerance: 0,
    };

    pub const MATOZ_UVKX: Self = Self {
        bytes: [0x55, 0x55, 0x55, 0xAA],
        length: 4,
        error_tolerance: 0,
    };

    pub const AX25_FLAG: Self = Self {
        bytes: [0x7E, 0x7E, 0x00, 0x00],
        length: 2,
        error_tolerance: 0,
    };

    pub const NOAA_SAME: Self = Self {
        bytes: [0xAB, 0xAB, 0xAB, 0x00],
        length: 3,
        error_tolerance: 1,
    };

    pub const fn from_u32(val: u32) -> Self {
        Self {
            bytes: [
                (val >> 24) as u8,
                (val >> 16) as u8,
                (val >> 8) as u8,
                val as u8,
            ],
            length: 4,
            error_tolerance: 0,
        }
    }
}

/// FIFO threshold limits for almost-empty and almost-full interrupts (Register 0x5E).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FskFifoThresholds {
    /// TX almost-empty threshold in words (0..=127).
    pub almost_empty_words: u8,
    /// RX almost-full threshold in words (0..=7).
    pub almost_full_words: u8,
}

impl Default for FskFifoThresholds {
    fn default() -> Self {
        Self {
            almost_empty_words: 64,
            almost_full_words: 4,
        }
    }
}

/// Granular physical layer configuration specification for BK4819 / BK4829 modem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FskPhyConfig {
    pub modulation: FskModulation,
    pub baud_rate: FskBaudRate,
    pub rx_bandwidth: FskRxBandwidth,
    pub rx_gain: FskRxGain,
    pub preamble_pattern: FskPreamblePattern,
    /// Number of preamble bytes prepended on TX (1..=16 bytes).
    pub preamble_length: u8,
    pub sync: FskSyncConfig,
    pub polarity: FskPolarity,
    pub scrambler: bool,
    /// FM deviation in Hz (Register 0x40).
    pub deviation_hz: u16,
    pub thresholds: FskFifoThresholds,
    /// Raw payload length in bytes (Register 0x5D, even-rounded in hardware).
    pub raw_length_bytes: u16,
}

impl Default for FskPhyConfig {
    fn default() -> Self {
        Self {
            modulation: FskModulation::DirectFsk,
            baud_rate: FskBaudRate::Baud1200,
            rx_bandwidth: FskRxBandwidth::Bw1k2,
            rx_gain: FskRxGain::Gain3,
            preamble_pattern: FskPreamblePattern::Auto,
            preamble_length: 16, // Maximum 16-byte lead-in for reliable locking
            sync: FskSyncConfig::K1_DEFAULT,
            polarity: FskPolarity::Normal,
            scrambler: false,
            deviation_hz: 1050,
            thresholds: FskFifoThresholds::default(),
            raw_length_bytes: 128,
        }
    }
}

/// Instantaneous physical modem and FIFO hardware status (Registers 0x0B, 0x0C, 0x5E).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FskPhyStatus {
    /// Latched RX Sync detected (Reg 0x0C bit 11).
    pub sync_detected: bool,
    /// Real-time preamble detected by demodulator (Reg 0x0B bit 7).
    pub preamble_detected: bool,
    /// Real-time sync pattern matched (Reg 0x0B bit 6).
    pub sync_matched_live: bool,
    /// RX FIFO almost full (Reg 0x0C bit 12).
    pub fifo_almost_full: bool,
    /// TX FIFO almost empty (Reg 0x0C bit 14).
    pub fifo_almost_empty: bool,
    /// RX FIFO empty (Reg 0x5E bit 13).
    pub fifo_rx_empty: bool,
    /// TX FIFO full (Reg 0x5E bit 14).
    pub fifo_tx_full: bool,
    /// Frame reception finished (Reg 0x0C bit 13).
    pub rx_finished: bool,
    /// Frame transmission finished (Reg 0x0C bit 15).
    pub tx_finished: bool,
    /// Words currently waiting in RX FIFO (Reg 0x5E bits <6:0>).
    pub rx_words_available: u8,
}

// ── Physical Layer Word & Bit Utilities ─────────────────────────────────────

/// Packs raw bytes into 16-bit little-endian words (`[b0, b1]` -> `(b1 << 8) | b0`).
pub fn pack_bytes_le(bytes: &[u8], words: &mut [u16]) -> usize {
    let word_count = (bytes.len() + 1) / 2;
    let count = word_count.min(words.len());
    for i in 0..count {
        let b0 = bytes[i * 2];
        let b1 = if i * 2 + 1 < bytes.len() {
            bytes[i * 2 + 1]
        } else {
            0
        };
        words[i] = (b0 as u16) | ((b1 as u16) << 8);
    }
    count
}

/// Packs raw bytes into 16-bit big-endian words (`[b0, b1]` -> `(b0 << 8) | b1`).
pub fn pack_bytes_be(bytes: &[u8], words: &mut [u16]) -> usize {
    let word_count = (bytes.len() + 1) / 2;
    let count = word_count.min(words.len());
    for i in 0..count {
        let b0 = bytes[i * 2];
        let b1 = if i * 2 + 1 < bytes.len() {
            bytes[i * 2 + 1]
        } else {
            0
        };
        words[i] = ((b0 as u16) << 8) | (b1 as u16);
    }
    count
}

/// Unpacks 16-bit little-endian words into bytes.
pub fn unpack_words_le(words: &[u16], bytes: &mut [u8]) -> usize {
    let mut idx = 0;
    for &w in words {
        if idx < bytes.len() {
            bytes[idx] = (w & 0xFF) as u8;
            idx += 1;
        }
        if idx < bytes.len() {
            bytes[idx] = (w >> 8) as u8;
            idx += 1;
        }
    }
    idx
}

/// Unpacks 16-bit big-endian words into bytes.
pub fn unpack_words_be(words: &[u16], bytes: &mut [u8]) -> usize {
    let mut idx = 0;
    for &w in words {
        if idx < bytes.len() {
            bytes[idx] = (w >> 8) as u8;
            idx += 1;
        }
        if idx < bytes.len() {
            bytes[idx] = (w & 0xFF) as u8;
            idx += 1;
        }
    }
    idx
}
// ── Physical Layer Hardware Trait Contract ──────────────────────────────────

/// Granular physical layer control contract for BK4819 / BK4829 FSK hardware.
pub trait RfFskPhy {
    /// Full physical layer register configuration (0x58..0x5E, 0x70, 0x72).
    fn fsk_configure_phy(&mut self, config: &FskPhyConfig) -> Result<(), ()>;
    /// Enable or disable the physical FSK modem core (Register 0x58 bit 0).
    fn fsk_enable(&mut self, enable: bool);
    /// Assert or deassert FSK RX listening (Register 0x59 bit 12).
    fn fsk_enable_rx(&mut self, enable: bool);
    /// Assert or deassert FSK TX transmission (Register 0x59 bit 11).
    fn fsk_enable_tx(&mut self, enable: bool);
    /// Pulse-clear hardware RX FIFO (Register 0x59 bit 14).
    fn fsk_clear_rx_fifo(&mut self);
    /// Pulse-clear hardware TX FIFO (Register 0x59 bit 15).
    fn fsk_clear_tx_fifo(&mut self);
    /// Read instantaneous physical status and FIFO fullness (Registers 0x0B, 0x0C, 0x5E).
    fn fsk_read_status(&mut self) -> FskPhyStatus;
    /// Read raw 16-bit words directly from hardware FIFO register 0x5F.
    fn fsk_read_fifo(&mut self, out_words: &mut [u16]) -> usize;
    /// Write raw 16-bit words directly to hardware FIFO register 0x5F.
    fn fsk_write_fifo(&mut self, words: &[u16]) -> usize;
    /// Set Tone2 subcarrier amplitude / tuning gain (Register 0x70).
    fn fsk_set_tone_gain(&mut self, gain: u8);
    /// Set clock baud divider (Register 0x72).
    fn fsk_set_baud_divider(&mut self, divider: u16);
    /// Set hardware TX deviation (Register 0x40).
    fn fsk_set_deviation(&mut self, deviation_hz: u16);
    /// Fast non-blocking RX re-arm (stop RX -> clear FIFO -> start RX per Section 14).
    fn fsk_rearm_rx(&mut self);

    // ── Granular Convenience Bridges ──
    fn configure_fsk(&mut self, config: &FskPhyConfig) -> Result<(), ()> {
        self.fsk_configure_phy(config)
    }
    fn start_fsk_rx(&mut self) {
        self.fsk_clear_rx_fifo();
        self.fsk_enable_rx(true);
    }
    fn rearm_fsk_rx(&mut self) {
        self.fsk_rearm_rx();
    }
    fn poll_fsk_status(&mut self) -> FskPhyStatus {
        self.fsk_read_status()
    }
    fn read_fsk_fifo(&mut self, out_words: &mut [u16]) -> usize {
        self.fsk_read_fifo(out_words)
    }
    fn write_fsk_fifo(&mut self, words: &[u16]) -> Result<usize, ()> {
        Ok(self.fsk_write_fifo(words))
    }
    fn start_fsk_tx(&mut self) {
        self.fsk_enable_tx(true);
    }
    fn stop_fsk(&mut self) {
        self.fsk_enable_rx(false);
        self.fsk_enable_tx(false);
        self.fsk_enable(false);
    }

    /// Read a single 16-bit word from the hardware RX FIFO if available.
    fn fsk_read_word(&mut self) -> Option<u16> {
        let mut buf = [0u16; 1];
        if self.fsk_read_fifo(&mut buf) > 0 {
            Some(buf[0])
        } else {
            None
        }
    }

    /// Write a single 16-bit word to the hardware TX FIFO if space is available.
    fn fsk_write_word(&mut self, word: u16) -> bool {
        let buf = [word];
        self.fsk_write_fifo(&buf) > 0
    }

    /// Query if frame reception has finished (Reg 0x0C bit 13).
    fn fsk_is_rx_finished(&mut self) -> bool {
        self.fsk_read_status().rx_finished
    }

    /// Query if frame transmission has finished (Reg 0x0C bit 15).
    fn fsk_is_tx_finished(&mut self) -> bool {
        self.fsk_read_status().tx_finished
    }

    /// Query if hardware sync correlator detected sync pattern (Reg 0x0C bit 11).
    fn fsk_is_sync_detected(&mut self) -> bool {
        self.fsk_read_status().sync_detected
    }

    /// Query instantaneous count of words waiting in RX FIFO (Reg 0x5E bits <6:0>).
    fn fsk_words_available(&mut self) -> u8 {
        self.fsk_read_status().rx_words_available
    }

    /// Transmit raw FSK words and wait for TX completion up to timeout_ms.
    fn send_fsk_packet_with_timeout(&mut self, words: &[u16], timeout_ms: u16) -> Result<(), ()>;
}
