//! Firmware Physical FSK Streaming Service.
//!
//! Provides granular, hardware-direct FSK raw word streaming and transceiver
//! management without imposing higher-level packet headers, envelopes, or checksums.

use kernel::service::radio::{FskPhyConfig, RfFskPhy, RfPowerSequencer};

/// Raw physical FSK stream transceiver service.
pub struct FskStreamService;

impl FskStreamService {
    /// Configure physical modem and arm receiver.
    pub fn arm_rx<R: RfFskPhy>(radio: &mut R, config: &FskPhyConfig) -> Result<(), ()> {
        radio.fsk_configure_phy(config)?;
        radio.fsk_clear_rx_fifo();
        radio.fsk_enable_rx(true);
        radio.fsk_enable(true);
        Ok(())
    }

    /// Disarm FSK modem and power down physical blocks.
    pub fn disarm<R: RfFskPhy>(radio: &mut R) {
        radio.fsk_enable_rx(false);
        radio.fsk_enable_tx(false);
        radio.fsk_enable(false);
    }

    /// Poll for received words and read into `out_words`.
    /// Automatically performs Section 14 fast re-arm if frame finished.
    pub fn poll_and_read<R: RfFskPhy>(radio: &mut R, out_words: &mut [u16]) -> usize {
        let status = radio.fsk_read_status();
        let mut total_read = 0;

        if status.rx_words_available > 0 {
            total_read = radio.fsk_read_fifo(out_words);
        }

        if status.rx_finished {
            radio.fsk_rearm_rx();
        }

        total_read
    }

    /// Transmit raw word buffer with full RF TR power sequencing.
    pub fn transmit_words<R: RfFskPhy + RfPowerSequencer>(
        radio: &mut R,
        words: &[u16],
        power_level: u8,
        timeout_ms: u16,
    ) -> Result<(), ()> {
        radio.tx_prepare();
        radio.tx_ramp_up(power_level);
        radio.tx_unmute();

        let result = radio.send_fsk_packet_with_timeout(words, timeout_ms);

        py32_hal::delay::delay_ms(20); // Post-burst carrier tail
        radio.tx_mute();
        radio.tx_ramp_down();
        radio.tx_conclude();

        result
    }

    #[cfg(feature = "rf_fsk")]
    pub fn arm_rx_baud<R: RfFskPhy>(radio: &mut R, baud: u16) {
        use kernel::service::radio::FskBaudRate;
        let baud_rate = match baud {
            2400 => FskBaudRate::Baud2400,
            4800 => FskBaudRate::Baud4800,
            9600 => FskBaudRate::Baud9600,
            _ => FskBaudRate::Baud1200,
        };
        let mut config = FskPhyConfig::default();
        config.baud_rate = baud_rate;
        config.raw_length_bytes = 64;
        let _ = Self::arm_rx(radio, &config);
    }

    #[cfg(feature = "rf_fsk")]
    pub fn transmit_bytes<R: RfFskPhy + RfPowerSequencer>(
        radio: &mut R,
        data: &[u8],
    ) -> Result<(), ()> {
        let mut words = [0u16; 64];
        let limit = data.len().div_ceil(2).min(words.len());
        for i in 0..limit {
            let b0 = data[i * 2] as u16;
            let b1 = data.get(i * 2 + 1).copied().unwrap_or(0) as u16;
            words[i] = (b0 << 8) | b1;
        }
        Self::transmit_words(radio, &words[..limit], 3, 500)
    }

    #[cfg(feature = "rf_fsk")]
    pub fn receive_bytes<R: RfFskPhy>(radio: &mut R, buf: &mut [u8]) -> usize {
        let mut words = [0u16; 32];
        let count = Self::poll_and_read(radio, &mut words);
        let mut out_len = 0;
        for &w in &words[..count] {
            if out_len < buf.len() {
                buf[out_len] = (w >> 8) as u8;
                out_len += 1;
            }
            if out_len < buf.len() {
                buf[out_len] = (w & 0xFF) as u8;
                out_len += 1;
            }
        }
        out_len
    }
}
