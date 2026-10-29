//! Comprehensive, Multi-Tiered RF Radio Subsystem Contracts.

pub mod af;
pub mod agc;
pub mod arbitrator;
pub mod channel;
pub mod event;
pub mod fsk;
pub mod modulation;
pub mod power_seq;
pub mod squelch;
pub mod tone;
pub mod tone_gen;
pub mod tuner;
pub mod vox;

pub use af::*;
pub use agc::*;
pub use arbitrator::*;
pub use channel::*;
pub use event::*;
pub use fsk::*;
pub use modulation::*;
pub use power_seq::*;
pub use squelch::*;
pub use tone::*;
pub use tone_gen::*;
pub use tuner::*;
pub use vox::*;

/// Full composite RF Radio Service contract combining granular facets with audio, AGC, and tone generation.
pub trait RadioService:
    RfChannel
    + RfTuning
    + RfFineTuning
    + RfModulation
    + RfSquelch
    + RfSubaudible
    + RfFskPhy
    + RfToneGenerator
    + RfAudioPath
    + RfAgc
{
    fn sleep(&mut self);
    fn wake(&mut self);
    fn set_xtal_freq_trim(&mut self, trim: i16);
}
