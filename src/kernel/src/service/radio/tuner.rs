//! Physical RF Tuner Identifier and Audio Cross-Bar Routing Contracts (§1.5).

/// Physical RF Tuner Identifier for multi-receiver architectures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TunerId {
    /// Primary transceiver tuner (e.g. BK4829 covering VHF/UHF/Airband).
    Primary = 0,
    /// Auxiliary broad-band HF / AM / SSB receiver (e.g. Si4732 covering 150 kHz - 30 MHz).
    Secondary = 1,
    /// Broadcast FM receiver (e.g. BK1080 covering 64 MHz - 108 MHz).
    BroadcastFm = 2,
}

impl TunerId {
    pub const ALL: [Self; 3] = [Self::Primary, Self::Secondary, Self::BroadcastFm];

    #[inline(always)]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Primary => "Primary (BK4829)",
            Self::Secondary => "Secondary (Si4732)",
            Self::BroadcastFm => "FM (BK1080)",
        }
    }

    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Primary),
            1 => Some(Self::Secondary),
            2 => Some(Self::BroadcastFm),
            _ => None,
        }
    }
}

/// Dynamic Audio Cross-Bar Routing Source.
///
/// Directs audio from a designated physical tuner to the audio power amplifier,
/// while allowing other tuners and demodulators to continue background operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioCrossbarSource {
    /// Audio routed from Primary transceiver (BK4829).
    Primary = 0,
    /// Audio routed from Secondary broad-band HF receiver (Si4732).
    Secondary = 1,
    /// Audio routed from Broadcast FM receiver (BK1080).
    BroadcastFm = 2,
    /// Audio muted (e.g. during squelch closed, digital preamble, or priority transition).
    Muted = 3,
}

impl AudioCrossbarSource {
    #[inline(always)]
    pub const fn is_muted(self) -> bool {
        matches!(self, Self::Muted)
    }

    #[inline(always)]
    pub const fn tuner(self) -> Option<TunerId> {
        match self {
            Self::Primary => Some(TunerId::Primary),
            Self::Secondary => Some(TunerId::Secondary),
            Self::BroadcastFm => Some(TunerId::BroadcastFm),
            Self::Muted => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tuner_id_and_crossbar() {
        assert_eq!(TunerId::Primary.name(), "Primary (BK4829)");
        assert_eq!(TunerId::from_u8(0), Some(TunerId::Primary));
        assert_eq!(TunerId::from_u8(1), Some(TunerId::Secondary));
        assert_eq!(TunerId::from_u8(2), Some(TunerId::BroadcastFm));
        assert_eq!(TunerId::from_u8(3), None);

        let src = AudioCrossbarSource::BroadcastFm;
        assert_eq!(src.tuner(), Some(TunerId::BroadcastFm));
        assert!(!src.is_muted());

        let muted = AudioCrossbarSource::Muted;
        assert_eq!(muted.tuner(), None);
        assert!(muted.is_muted());
    }
}
