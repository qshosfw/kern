//! Physical RF Transceiver Power Sequencing & TR Switching Contract.
//!
//! Provides bare-metal safe sequencing for switching transceiver hardware between
//! reception and transmission states, preventing RF transient clicks, harmonic splatter,
//! and hot-switching front-end PIN diodes.

/// Operational physical transceiver RF transmit state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransceiverTxState {
    #[default]
    IdleRx,
    PreTxMute,
    PaRampingUp,
    ActiveTransmit,
    PostTxTail,
    PaRampingDown,
}

/// Granular physical contract for transceiver transmit power sequencing and TR switching.
pub trait RfPowerSequencer {
    /// Safe pre-transmission preparation:
    /// - Mutes receiver audio path
    /// - Disables subaudible squelch tones during data setup
    /// - Asserts TX RF front-end switch and clears RX switch
    /// - Sets initial idle bias before PA activation
    fn tx_prepare(&mut self);

    /// Soft-start S-curve PA power ramping up to desired bias level.
    fn tx_ramp_up(&mut self, target_power_level: u8);

    /// Unmutes TX modulation path and begins active RF transmission.
    fn tx_unmute(&mut self);

    /// Mutes TX modulation path while maintaining carrier tail.
    fn tx_mute(&mut self);

    /// Soft-start S-curve PA power ramp down to zero bias.
    fn tx_ramp_down(&mut self);

    /// Complete post-transmission teardown:
    /// - Disables PA switch and restores RX front-end switch
    /// - Re-enables receiver audio filtering and squelch
    fn tx_conclude(&mut self);

    /// Current physical sequencer state.
    fn tx_state(&self) -> TransceiverTxState;
}
