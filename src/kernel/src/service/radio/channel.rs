//! Channel Configurations, Repeater Offsets, Step Frequencies, and VFO Descriptors.

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

/// Dial increment as stored in a channel record, in units of 10 Hz.
/// A stored zero uses the default increment while retaining its record value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepSetting(u16);

impl StepSetting {
    pub const DEFAULT: Self = Self(1250);

    pub const fn in_hz_x10(self) -> u16 {
        self.0
    }

    pub const fn in_hz(self) -> u32 {
        let units = if self.0 == 0 { Self::DEFAULT.0 } else { self.0 };
        units as u32 * 10
    }

    pub const fn from_hz_x10(val: u16) -> Self {
        Self(val)
    }

    pub const fn from_request_hz(hz: u32) -> Option<Self> {
        let (units, remainder) = utils::math::div10_u32(hz);
        if units == 0 || units > u16::MAX as u32 || remainder != 0 {
            return None;
        }
        Some(Self(units as u16))
    }
}

#[cfg(test)]
mod tests {
    use super::StepSetting;

    #[test]
    fn tuning_request_rejects_unrepresentable_steps() {
        for hz in [0, 1, 11, 8_333, 655_351, 655_360, u32::MAX] {
            assert_eq!(StepSetting::from_request_hz(hz), None);
        }
        for units in 1..=u16::MAX {
            let step = StepSetting::from_request_hz(u32::from(units) * 10).unwrap();
            assert_eq!(step.in_hz_x10(), units);
        }
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
    fn configure_channel(&mut self, config: &super::record::ChannelRecord);
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
