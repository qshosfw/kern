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

    pub const fn default_name(self) -> &'static str {
        match self {
            Self::VfoA => "MAIN VFO",
            Self::VfoB => "RPT 2M",
            _ => "CALL 2M",
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
    pub const ALL: [Self; 10] = [
        Self::Step1_25k,
        Self::Step2_5k,
        Self::Step5k,
        Self::Step6_25k,
        Self::Step8_33k,
        Self::Step10k,
        Self::Step12_5k,
        Self::Step25k,
        Self::Step50k,
        Self::Step100k,
    ];

    pub const STEP_HZ_X10: [u16; 10] = [125, 250, 500, 625, 833, 1000, 1250, 2500, 5000, 10000];

    #[inline(always)]
    pub const fn in_hz_x10(self) -> u16 {
        Self::STEP_HZ_X10[self as usize]
    }

    #[inline(always)]
    pub const fn in_hz(self) -> u32 {
        (Self::STEP_HZ_X10[self as usize] as u32) * 10
    }

    #[inline(never)]
    pub const fn from_hz_x10(val: u16) -> Self {
        let mut i = 0;
        while i < 10 {
            if Self::STEP_HZ_X10[i] == val {
                return Self::ALL[i];
            }
            i += 1;
        }
        Self::Step12_5k
    }
}

/// RF power stage amplifier level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RfPowerLevel {
    Low = 0,    // ~0.5 W
    Medium = 1, // ~2.0 W
    High = 2,   // ~5.0 W
}

impl RfPowerLevel {
    pub const fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Low,
            1 => Self::Medium,
            _ => Self::High,
        }
    }

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

    fn shifted_freq(&self, apply: bool) -> Frequency<Hertz> {
        if !apply {
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

    /// Calculate effective TX frequency taking into account repeater shift and reverse mode
    pub fn effective_tx_freq(&self) -> Frequency<Hertz> {
        self.shifted_freq(!self.reverse)
    }

    /// Calculate effective RX frequency taking into account reverse mode
    pub fn effective_rx_freq(&self) -> Frequency<Hertz> {
        self.shifted_freq(self.reverse)
    }
}

/// Pre-computed PLL divider and front-end filter state for zero-division sub-band interleaving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrecomputedTuneState {
    pub freq_hz: u32,
    pub pll_low: u16,
    pub pll_high: u16,
    pub uhf_path: bool,
}

impl PrecomputedTuneState {
    #[inline(always)]
    pub const fn from_freq_hz(freq_hz: u32) -> Self {
        let freq_10hz = freq_hz / 10;
        Self {
            freq_hz,
            pll_low: (freq_10hz & 0xFFFF) as u16,
            pll_high: ((freq_10hz >> 16) & 0xFFFF) as u16,
            uhf_path: freq_hz >= 280_000_000,
        }
    }
}

/// Declarative interface for RF channel frequency synthesis and RF power output.
pub trait RfChannel {
    fn configure_channel(&mut self, config: &ChannelConfig);
    fn set_rx_frequency(&mut self, freq: Frequency<Hertz>);
    fn set_tx_frequency(&mut self, freq: Frequency<Hertz>);
    fn set_power_level(&mut self, level: RfPowerLevel);

    /// Apply a pre-computed PLL & LNA filter state for high-speed sub-band interleaving.
    fn apply_precomputed_tune(&mut self, state: &PrecomputedTuneState) {
        self.set_rx_frequency(Frequency::new(state.freq_hz));
    }

    /// Poll hardware PLL / VCO lock detector up to `max_wait_us` microseconds.
    /// Returns `true` as soon as the synthesizer locks so RSSI can be sampled without blind sleeps.
    fn wait_pll_lock(&mut self, _max_wait_us: u16) -> bool {
        true
    }

    /// Read adjacent channel power ratio from silicon (`Reg 0x66`), returning `(upper_adj, lower_adj)`.
    fn get_adjacent_channel_power(&mut self) -> (u8, u8) {
        (0, 0)
    }

    /// Read LNA peak RSSI indicator from silicon (`Reg 0x62` bits `[15:8]`).
    fn get_lna_peak_rssi(&mut self) -> u8 {
        0
    }
}

/// Fine frequency synthesis, crystal trimming, and PLL fractional compensation.
pub trait RfFineTuning {
    /// Apply crystal oscillator trim in hardware units (-300..=300).
    fn set_crystal_trim(&mut self, trim: i16);
    /// Apply 10-Hz coarse PLL offset.
    fn set_pll_offset_10hz(&mut self, offset_10hz: i16);
    /// Get the active crystal trim word.
    fn get_crystal_trim(&mut self) -> i16;
    /// Set sub-Hz frequency offset relative to nominal carrier using Vernier calculation.
    /// `offset_dhz`: desired offset in deci-Hertz (0.1 Hz increments, e.g. +62 = +6.2 Hz, -1700 = -170 Hz).
    fn set_sub_hz_offset(&mut self, carrier_hz: u32, offset_dhz: i16);
    /// Read signed Automatic Frequency Control (AFC) frequency offset indicator from silicon (`Reg 0x6D`).
    fn get_afc_offset(&mut self) -> i16 {
        0
    }
}
