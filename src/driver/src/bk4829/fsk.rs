//! Hardware FSK Physical Layer Register Controller for BK4819 / BK4829.

use super::registers::*;
use kernel::service::radio::fsk::{
    FskBaudRate, FskModulation, FskPhyConfig, FskPhyStatus, FskPolarity, FskPreamblePattern,
    FskRxBandwidth,
};

/// Configure hardware FSK modem control register (0x58) from modulation & baud.
pub fn build_fsk_ctrl(mod_type: FskModulation, baud: FskBaudRate) -> u16 {
    match mod_type {
        FskModulation::DirectFsk => match baud {
            FskBaudRate::Baud1200 | FskBaudRate::Baud2400 | FskBaudRate::Baud4800 => 0x00C1,
            FskBaudRate::Baud9600 => 0x00C0,
            FskBaudRate::CustomDivider(_) => 0x00C1,
        },
        FskModulation::Ffsk1200_1800 => {
            // Bits <15:13> = 1 (FFSK 1200/1800 TX)
            // Bits <12:10> = 7 (FFSK 1200/1800 RX)
            // Bits <9:8> = 3 (Max RX Gain)
            // Bits <3:1> = 1 (RX Bandwidth 1200/1800)
            // Bit 0 = 1 (FSK enable)
            (1 << 13) | (7 << 10) | (3 << 8) | (1 << 1) | 1
        }
        FskModulation::Ffsk1200_2400 => (3 << 13) | (4 << 10) | (3 << 8) | (4 << 1) | 1,
        FskModulation::NoaaSame => (5 << 13) | (0 << 10) | (3 << 8) | (2 << 1) | 1,
    }
}

/// Configure hardware FSK modem control register (0x58) from comprehensive FskPhyConfig.
pub fn build_fsk_ctrl_from_phy(config: &FskPhyConfig) -> u16 {
    let (tx_mode, rx_mode, bw) = match config.modulation {
        FskModulation::DirectFsk => (
            0u16,
            0u16,
            match config.rx_bandwidth {
                FskRxBandwidth::Bw1k2 => 0u16,
                FskRxBandwidth::Bw1k2_1k8 => 1u16,
                FskRxBandwidth::BwNoaa => 2u16,
                FskRxBandwidth::Bw2k4 => 4u16,
            },
        ),
        FskModulation::Ffsk1200_1800 => (1u16, 7u16, 1u16),
        FskModulation::Ffsk1200_2400 => (3u16, 4u16, 4u16),
        FskModulation::NoaaSame => (5u16, 0u16, 2u16),
    };
    let gain = (config.rx_gain as u16) & 0x03;
    let preamble_pat = match config.preamble_pattern {
        FskPreamblePattern::Auto => 0u16,
        FskPreamblePattern::Pattern55 => 1u16,
        FskPreamblePattern::PatternAA => 2u16,
    };
    (tx_mode << 13) | (rx_mode << 10) | (gain << 8) | (preamble_pat << 4) | (bw << 1) | 1
}

/// Calculate 0x72 Baud Rate Divider word.
pub fn build_baud_divider(baud: FskBaudRate) -> u16 {
    baud.divider_word()
}

/// Build register 0x59 FIFO and preamble control word.
pub fn build_fsk_fifo_ctrl(preamble_bytes: u8, sync_bytes: u8) -> u16 {
    let pre_sel = (preamble_bytes.clamp(1, 16) - 1) as u16;
    let sync_sel = if sync_bytes > 2 { 1u16 } else { 0u16 };
    (sync_sel << 3) | (pre_sel << 4)
}

/// Build register 0x59 FIFO and preamble control word from comprehensive FskPhyConfig.
pub fn build_fsk_fifo_ctrl_from_phy(config: &FskPhyConfig) -> u16 {
    let pre_sel = (config.preamble_length.clamp(1, 16) - 1) as u16;
    let sync_sel = if config.sync.length > 2 { 1u16 } else { 0u16 };
    let mut reg = (sync_sel << 3) | (pre_sel << 4);
    match config.polarity {
        FskPolarity::Normal => {}
        FskPolarity::InvertRx => reg |= 1 << 9,
        FskPolarity::InvertTx => reg |= 1 << 10,
        FskPolarity::InvertBoth => reg |= (1 << 9) | (1 << 10),
    }
    if config.scrambler {
        reg |= 1 << 8;
    }
    reg
}

/// Build register 0x5D packet length word with mandatory hardware even-byte rounding.
/// BK4819 / BK4829 hardware FIFO stalls if packet length is not even.
pub fn build_packet_length_reg(max_len_bytes: u16) -> u16 {
    let len = max_len_bytes.clamp(2, 128);
    let rounded_len = ((len + 1) / 2) * 2;
    let encoded = rounded_len - 1;
    ((encoded & 0xFF) << 8) | ((encoded >> 8) << 5)
}

/// Hardware register 0x5C: Disable hardware CRC checking/generation for transparent raw FIFO IO.
pub const REG_FSK_CRC_TRANSPARENT: u16 = 0x5665;

/// Parse registers 0x0B, 0x0C and 0x5E into high-level FskPhyStatus.
pub fn parse_fsk_status(reg0b: u16, reg0c: u16, reg5e: u16) -> FskPhyStatus {
    let words_avail = (reg5e & 0x007F) as u8;
    FskPhyStatus {
        sync_detected: (reg0c & INT_FSK_RX_SYNC) != 0,
        preamble_detected: (reg0b & (1 << 7)) != 0,
        sync_matched_live: (reg0b & (1 << 6)) != 0,
        fifo_almost_full: (reg0c & INT_FSK_FIFO_ALMOST_FULL) != 0,
        fifo_almost_empty: (reg0c & INT_FSK_FIFO_ALMOST_EMPTY) != 0,
        fifo_rx_empty: (reg5e & (1 << 13)) != 0,
        fifo_tx_full: (reg5e & (1 << 14)) != 0,
        rx_finished: (reg0c & INT_FSK_RX_FINISHED) != 0,
        tx_finished: (reg0c & INT_FSK_TX_FINISHED) != 0,
        rx_words_available: words_avail,
    }
}
