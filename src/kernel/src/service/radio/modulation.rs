//! RF Modulation, Filtering, Deviation and Audio Processing contracts.

use super::af::AfResponse;
use super::agc::{AgcListeningMode, AgcSpeed};
use super::squelch::SquelchType;

/// Modulation and demodulation schemes supported across radio receivers and transmitters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ModulationMode {
    /// Narrowband Frequency Modulation (standard analog voice)
    Fm = 0,
    /// Amplitude Modulation (standard envelope detection)
    Am = 1,
    /// AM with active Baseband Demodulator & AGC correction (IJV FW5 / DeltaFW AMB).
    AmB = 2,
    /// Upper Sideband (SSB DSP envelope / BFO offset).
    Usb = 3,
    /// Double Sideband.
    Dsb = 4,
    /// Continuous Wave (Morse code / carrier keying).
    Cw = 5,
    /// Broadcast Wide FM (demodulator bypass filter).
    Wfm = 6,
    /// Packet Flat FM (de-emphasis/pre-emphasis bypassed, 20Hz..6kHz flat response for Direwolf/TNC/APRS).
    PktFm = 7,
    /// Direct Digital Carrier Modulation (Sub-Hz Fractional-N Vernier M-FSK: FT8, WSPR, JS8Call, narrow FSK).
    Dig = 8,
    /// Raw discriminator audio.
    Raw = 9,
    /// Demodulator bypass (raw baseband tapped for decoders / pin PB1).
    Byp = 10,
}

impl ModulationMode {
    pub const fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Fm,
            1 => Self::Am,
            2 => Self::AmB,
            3 => Self::Usb,
            4 => Self::Dsb,
            5 => Self::Cw,
            6 => Self::Wfm,
            7 => Self::PktFm,
            8 => Self::Dig,
            9 => Self::Raw,
            10 => Self::Byp,
            _ => Self::Fm,
        }
    }

    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fm => "FM",
            Self::Am => "AM",
            Self::AmB => "AMB",
            Self::Usb => "USB",
            Self::Dsb => "DSB",
            Self::Cw => "CW",
            Self::Wfm => "WFM",
            Self::PktFm => "PKT",
            Self::Dig => "DIG",
            Self::Raw => "RAW",
            Self::Byp => "BYP",
        }
    }

    /// Return the generic, standard operational profile defining the physical RF,
    /// audio shaping, routing, and AGC requirements for this modulation mode.
    pub const fn default_profile(self) -> ModulationProfile {
        match self {
            Self::Fm => ModulationProfile {
                mode: Self::Fm,
                default_bandwidth: Bandwidth::Narrow12k5,
                af_response: AfResponse {
                    rx_300: 0,
                    rx_3k: 0,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::Speaker,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Fm,
                agc_speed: AgcSpeed::Medium,
                bfo_offset_hz: 0,
                default_step_hz: 12_500,
            },
            Self::Am => ModulationProfile {
                mode: Self::Am,
                default_bandwidth: Bandwidth::Narrow12k5,
                af_response: AfResponse {
                    rx_300: -2,
                    rx_3k: -2,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::Speaker,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::AmListening,
                agc_speed: AgcSpeed::Slow,
                bfo_offset_hz: 0,
                default_step_hz: 8_333,
            },
            Self::AmB => ModulationProfile {
                mode: Self::AmB,
                default_bandwidth: Bandwidth::Narrow12k5,
                af_response: AfResponse {
                    rx_300: -2,
                    rx_3k: -2,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::Speaker,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::AmListening,
                agc_speed: AgcSpeed::Slow,
                bfo_offset_hz: 0,
                default_step_hz: 8_333,
            },
            Self::Usb => ModulationProfile {
                mode: Self::Usb,
                default_bandwidth: Bandwidth::Narrow6k25,
                af_response: AfResponse {
                    rx_300: 0,
                    rx_3k: 0,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::Speaker,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Ssb,
                agc_speed: AgcSpeed::Fast,
                bfo_offset_hz: 1500,
                default_step_hz: 1_000,
            },
            Self::Dsb => ModulationProfile {
                mode: Self::Dsb,
                default_bandwidth: Bandwidth::Narrow6k25,
                af_response: AfResponse {
                    rx_300: 0,
                    rx_3k: 0,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::Speaker,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Ssb,
                agc_speed: AgcSpeed::Fast,
                bfo_offset_hz: 0,
                default_step_hz: 1_000,
            },
            Self::Cw => ModulationProfile {
                mode: Self::Cw,
                default_bandwidth: Bandwidth::Narrow6k25,
                af_response: AfResponse {
                    rx_300: 0,
                    rx_3k: 0,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::Speaker,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Ssb,
                agc_speed: AgcSpeed::Fast,
                bfo_offset_hz: 700,
                default_step_hz: 500,
            },
            Self::Wfm => ModulationProfile {
                mode: Self::Wfm,
                default_bandwidth: Bandwidth::Wide25k,
                af_response: AfResponse {
                    rx_300: -4,
                    rx_3k: -4,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::Speaker,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Fm,
                agc_speed: AgcSpeed::Medium,
                bfo_offset_hz: 0,
                default_step_hz: 50_000,
            },
            Self::PktFm => ModulationProfile {
                mode: Self::PktFm,
                default_bandwidth: Bandwidth::Narrow12k5,
                af_response: AfResponse {
                    rx_300: -4,
                    rx_3k: -4,
                    tx_300: -4,
                    tx_3k: -4,
                },
                audio_routing: AudioRoutingPolicy::BasebandModem,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Fm,
                agc_speed: AgcSpeed::Fast,
                bfo_offset_hz: 0,
                default_step_hz: 12_500,
            },
            Self::Dig => ModulationProfile {
                mode: Self::Dig,
                default_bandwidth: Bandwidth::Narrow6k25,
                af_response: AfResponse {
                    rx_300: -4,
                    rx_3k: -4,
                    tx_300: -4,
                    tx_3k: -4,
                },
                audio_routing: AudioRoutingPolicy::BasebandModem,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Fm,
                agc_speed: AgcSpeed::Fast,
                bfo_offset_hz: 0,
                default_step_hz: 10,
            },
            Self::Raw => ModulationProfile {
                mode: Self::Raw,
                default_bandwidth: Bandwidth::Wide25k,
                af_response: AfResponse {
                    rx_300: 0,
                    rx_3k: 0,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::DiscriminatorTap,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Fm,
                agc_speed: AgcSpeed::Medium,
                bfo_offset_hz: 0,
                default_step_hz: 12_500,
            },
            Self::Byp => ModulationProfile {
                mode: Self::Byp,
                default_bandwidth: Bandwidth::Wide25k,
                af_response: AfResponse {
                    rx_300: 0,
                    rx_3k: 0,
                    tx_300: 0,
                    tx_3k: 0,
                },
                audio_routing: AudioRoutingPolicy::BasebandModem,
                squelch_type: SquelchType::Carrier,
                agc_mode: AgcListeningMode::Fm,
                agc_speed: AgcSpeed::Medium,
                bfo_offset_hz: 0,
                default_step_hz: 12_500,
            },
        }
    }
}

/// Audio baseband routing policy for an RF mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioRoutingPolicy {
    /// Audio routed to speaker / headphones when squelch opens.
    Speaker,
    /// Demodulated baseband streamed directly to modem / DMA buffer (silent to user).
    BasebandModem,
    /// Raw discriminator audio tapped for external decoders (pin PB1 / USB CDC).
    DiscriminatorTap,
    /// Receiver audio muted regardless of carrier.
    Muted,
}

/// Comprehensive, generic RF mode descriptor defining the physical, audio,
/// AGC, filtering, and squelch requirements of a modulation scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModulationProfile {
    pub mode: ModulationMode,
    pub default_bandwidth: Bandwidth,
    pub af_response: AfResponse,
    pub audio_routing: AudioRoutingPolicy,
    pub squelch_type: SquelchType,
    pub agc_mode: AgcListeningMode,
    pub agc_speed: AgcSpeed,
    /// Beat Frequency Oscillator (BFO) pitch offset in Hz for CW / SSB.
    pub bfo_offset_hz: i16,
    /// Default channel raster step in Hz.
    pub default_step_hz: u32,
}

/// Standard repeater tone burst frequencies (e.g. 1750 Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeaterAccessTone {
    Hz1750, // European repeater access
    Hz1450,
    Hz2100,
    Custom(u16),
}

impl RepeaterAccessTone {
    pub const fn in_hz(self) -> u16 {
        match self {
            Self::Hz1750 => 1750,
            Self::Hz1450 => 1450,
            Self::Hz2100 => 2100,
            Self::Custom(hz) => hz,
        }
    }
}

/// Upconverter LO frequency shift for HF reception (e.g. +125 MHz Ham It Up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpconverterShift {
    None,
    Plus(utils::units::Frequency<utils::units::Hertz>),
    Minus(utils::units::Frequency<utils::units::Hertz>),
}

/// DSP channel filter bandwidth selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bandwidth {
    Wide25k,
    Narrow12k5,
    Narrow6k25,
}

impl Bandwidth {
    pub const fn in_hz(self) -> u32 {
        match self {
            Self::Wide25k => 25_000,
            Self::Narrow12k5 => 12_500,
            Self::Narrow6k25 => 6_250,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wide25k => "W",
            Self::Narrow12k5 => "N",
            Self::Narrow6k25 => "VN",
        }
    }
}

/// FM Deviation limit presets (in Hz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationLimit {
    Dev1050Hz, // Narrow FSK / digital telemetry
    Dev2500Hz, // Standard Narrow FM (12.5 kHz channel)
    Dev4500Hz, // Standard Wide FM (20 kHz channel)
    Dev5000Hz, // Full Wide FM (25 kHz channel)
    Custom(u16),
}

impl DeviationLimit {
    pub const fn in_hz(self) -> u32 {
        match self {
            Self::Dev1050Hz => 1050,
            Self::Dev2500Hz => 2500,
            Self::Dev4500Hz => 4500,
            Self::Dev5000Hz => 5000,
            Self::Custom(hz) => hz as u32,
        }
    }
}

/// Dynamic range compression and expansion (compander).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompanderMode {
    Off,
    TxCompressor,
    RxExpander,
    Both,
}

/// Voice frequency inversion scrambler modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScramblerMode {
    Off,
    InvertFrequency(u8), // 1..8 preset inversion points
}

/// Declarative interface for RF modulation, DSP bandwidth, and audio dynamics.
pub trait RfModulation {
    fn set_modulation(&mut self, mode: ModulationMode);
    fn set_bandwidth(&mut self, bw: Bandwidth);
    fn set_deviation(&mut self, dev: DeviationLimit);
    fn set_compander(&mut self, compander: CompanderMode);
    fn set_scrambler(&mut self, scrambler: ScramblerMode);
}
