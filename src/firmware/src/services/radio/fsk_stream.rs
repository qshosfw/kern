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
}
