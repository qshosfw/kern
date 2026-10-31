//! Channel Configurations, Repeater Offsets, Step Frequencies, and VFO Descriptors.

use super::af::AfResponse;
use super::modulation::{Bandwidth, ModulationMode};
use super::tone::SubaudibleTone;
use utils::units::{Frequency, Hertz};

/// VFO Slot identifier supporting up to 4 concurrent VFOs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfoSlot {
    VfoA = 0,
    VfoB = 1,
    VfoC = 2,
    VfoD = 3,
}

impl VfoSlot {
    #[inline(always)]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline(always)]
    pub const fn other(self) -> Self {
        match self {
            Self::VfoA => Self::VfoB,
            Self::VfoB => Self::VfoA,
            Self::VfoC => Self::VfoD,
            Self::VfoD => Self::VfoC,
        }
    }
}

/// Instantaneous operational state of a VFO.
/// Directly mirrors the VfoState_t taxonomy from DeltaFW / F4HWN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfoState {
    Normal,
    Busy,
    RxCarrier,
    RxToneMatched,
    TxActive,
    TxDisable,
    Timeout,
    BatteryLow,
    VoltageHigh,
}

/// Frequency synthesizer tuning step options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSetting {
    Step1_25k, // 1.25 kHz
    Step2_5k,  // 2.50 kHz
    Step5k,    // 5.00 kHz
    Step6_25k, // 6.25 kHz (PMR446)
    Step8_33k, // 8.33 kHz (Airband)
    Step10k,   // 10.0 kHz
    Step12_5k, // 12.5 kHz
    Step25k,   // 25.0 kHz
    Step50k,   // 50.0 kHz
    Step100k,  // 100.0 kHz
}

impl StepSetting {
    pub const fn in_hz(self) -> u32 {
        match self {
            Self::Step1_25k => 1_250,
            Self::Step2_5k => 2_500,
            Self::Step5k => 5_000,
            Self::Step6_25k => 6_250,
            Self::Step8_33k => 8_330,
            Self::Step10k => 10_000,
            Self::Step12_5k => 12_500,
            Self::Step25k => 25_000,
            Self::Step50k => 50_000,
            Self::Step100k => 100_000,
        }
    }
}

/// RF power stage amplifier level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RfPowerLevel {
    Low,    // ~0.5 W
    Medium, // ~2.0 W
    High,   // ~5.0 W
}

impl RfPowerLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "L",
            Self::Medium => "M",
            Self::High => "H",
        }
    }
}

/// Repeater offset shift direction and magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeaterShift {
    Simplex,
    Positive(Frequency<Hertz>),
    Negative(Frequency<Hertz>),
}

/// Rich, comprehensive Channel and VFO profile.
#[derive(Debug, Clone, Copy)]
pub struct ChannelConfig {
    pub rx_freq: Frequency<Hertz>,
    pub tx_shift: RepeaterShift,
    pub step: StepSetting,
    pub bandwidth: Bandwidth,
    pub modulation: ModulationMode,
    pub power: RfPowerLevel,
    pub rx_subaudible: SubaudibleTone,
    pub tx_subaudible: SubaudibleTone,
    pub squelch_level: u8,
    pub reverse: bool,
    pub busy_lock: bool,
    pub af_response: AfResponse,
}

impl ChannelConfig {
    pub const fn new(rx_freq_hz: u32) -> Self {
        Self {
            rx_freq: Frequency::new(rx_freq_hz),
            tx_shift: RepeaterShift::Simplex,
            step: StepSetting::Step12_5k,
            bandwidth: Bandwidth::Narrow12k5,
            modulation: ModulationMode::Fm,
            power: RfPowerLevel::High,
            rx_subaudible: SubaudibleTone::None,
            tx_subaudible: SubaudibleTone::None,
            squelch_level: 4,
            reverse: false,
            busy_lock: false,
            af_response: AfResponse {
                rx_300: 0,
                rx_3k: 0,
                tx_300: 0,
                tx_3k: 0,
            },
        }
    }

    /// Calculate effective TX frequency taking into account repeater shift and reverse mode
    pub fn effective_tx_freq(&self) -> Frequency<Hertz> {
        if self.reverse {
            return self.rx_freq;
        }
        match self.tx_shift {
            RepeaterShift::Simplex => self.rx_freq,
            RepeaterShift::Positive(offset) => {
                Frequency::new(self.rx_freq.raw().saturating_add(offset.raw()))
            }
            RepeaterShift::Negative(offset) => {
                Frequency::new(self.rx_freq.raw().saturating_sub(offset.raw()))
            }
        }
    }

    /// Calculate effective RX frequency taking into account reverse mode
    pub fn effective_rx_freq(&self) -> Frequency<Hertz> {
        if !self.reverse {
            return self.rx_freq;
        }
        match self.tx_shift {
            RepeaterShift::Simplex => self.rx_freq,
            RepeaterShift::Positive(offset) => {
                Frequency::new(self.rx_freq.raw().saturating_add(offset.raw()))
            }
            RepeaterShift::Negative(offset) => {
                Frequency::new(self.rx_freq.raw().saturating_sub(offset.raw()))
            }
        }
    }
}

/// Declarative interface for RF channel frequency synthesis and RF power output.
pub trait RfChannel {
    fn configure_channel(&mut self, config: &ChannelConfig);
    fn set_rx_frequency(&mut self, freq: Frequency<Hertz>);
    fn set_tx_frequency(&mut self, freq: Frequency<Hertz>);
    fn set_power_level(&mut self, level: RfPowerLevel);
}

/// Fine frequency synthesis, crystal trimming, and PLL fractional compensation.
pub trait RfFineTuning {
    /// Apply crystal oscillator trim in hardware units (-300..=300).
    fn set_crystal_trim(&mut self, trim: i16);
    /// Apply 10-Hz coarse PLL offset.
    fn set_pll_offset_10hz(&mut self, offset_10hz: i16);
    /// Get the active crystal trim word.
    fn get_crystal_trim(&mut self) -> i16;
}
