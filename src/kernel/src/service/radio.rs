//! Comprehensive, Multi-Tiered RF Radio Subsystem Contracts.

pub mod af;
pub mod agc;
pub mod arbitrator;
pub mod channel;
pub mod event;
pub mod fsk;
pub mod modulation;
pub mod power_seq;
pub mod scan;
pub mod squelch;
pub mod tone;
pub mod tone_gen;
pub mod tuner;
pub mod vernier;
pub mod vox;

pub use af::*;
pub use agc::*;
pub use arbitrator::*;
pub use channel::*;
pub use event::*;
pub use fsk::*;
pub use modulation::*;
pub use power_seq::*;
pub use scan::*;
pub use squelch::*;
pub use tone::*;
pub use tone_gen::*;
pub use tuner::*;
pub use vernier::*;
pub use vox::*;

/// Full composite RF Radio Service contract combining granular facets with audio, AGC, and tone generation.
pub trait RadioService:
    RfChannel
    + RfFineTuning
    + RfModulation
    + RfSquelch
    + RfSubaudible
    + RfFskPhy
    + RfToneGenerator
    + RfAudioPath
    + RfAgc
    + RfPowerSequencer
{
    fn sleep(&mut self);
    fn wake(&mut self);
    fn set_xtal_freq_trim(&mut self, trim: i16);

    /// Enable or disable internal TX-to-RX analog loopback (`Reg 0x30 = 0xFFF7`, PA disabled).
    ///
    /// Couples the internal modulator and tone/CTCSS synthesizer directly into the receiver
    /// mixer through an internal attenuation path with zero RF emitted to the antenna.
    /// Ideal for silent CTCSS deviation calibration, mic audio level checking, and AFC offset tuning.
    fn set_internal_loopback(&mut self, _enabled: bool) {}

    /// Query whether internal TX-to-RX analog loopback mode is currently active.
    fn is_internal_loopback(&self) -> bool {
        false
    }
}
