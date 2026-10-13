//! Comprehensive, Multi-Tiered RF Radio Subsystem Contracts.

pub mod af;
pub mod agc;
pub mod channel;
pub mod dtmf;
pub mod fsk;
pub mod modulation;
pub mod morse;
pub mod power_seq;
pub mod selcall;
pub mod squelch;
pub mod tone;
pub mod tone_gen;
pub mod vox;

pub use af::*;
pub use agc::*;
pub use channel::*;
pub use dtmf::*;
pub use fsk::*;
pub use modulation::*;
pub use morse::*;
pub use power_seq::*;
pub use selcall::*;
pub use squelch::*;
pub use tone::*;
pub use tone_gen::*;
pub use vox::*;

use utils::units::{Frequency, Hertz};

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

    // ── Backward-compatibility convenience bridges ──
    #[inline(always)]
    fn get_rssi(&mut self) -> i16 {
        self.get_rssi_dbm()
    }
    #[inline(always)]
    fn set_frequency(&mut self, freq: Frequency<Hertz>) {
        self.set_rx_frequency(freq);
    }
    #[inline(always)]
    fn set_squelch(&mut self, level: u8) {
        self.set_squelch_level(level);
    }
    #[inline(always)]
    fn is_squelch_open(&mut self) -> bool {
        self.is_carrier_open()
    }
    #[inline(always)]
    fn set_rf_power(&mut self, level: u8) {
        let pwr = match level {
            0 => RfPowerLevel::Low,
            1 => RfPowerLevel::Medium,
            _ => RfPowerLevel::High,
        };
        self.set_power_level(pwr);
    }
    #[inline(always)]
    fn set_volume(&mut self, level: u8) {
        self.set_rf_volume(level);
    }
    #[inline(always)]
    fn set_audio_profile(&mut self, _profile: u8) {}
}
