//! Continuous Tone-Coded Squelch System (CTCSS) and Digitally Coded Squelch (DCS).

/// Subaudible squelch signaling tone or digital code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubaudibleTone {
    None,
    /// CTCSS frequency in deci-Hz (e.g. 670 = 67.0 Hz, 885 = 88.5 Hz, 2541 = 254.1 Hz).
    Ctcss(u16),
    /// Standard DCS octal code normal polarity (e.g. 0o023).
    DcsNormal(u16),
    /// Standard DCS octal code inverted polarity.
    DcsInverted(u16),
}

impl SubaudibleTone {
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub const fn is_ctcss(&self) -> bool {
        matches!(self, Self::Ctcss(_))
    }

    pub const fn is_dcs(&self) -> bool {
        matches!(self, Self::DcsNormal(_) | Self::DcsInverted(_))
    }
}

/// Squelch tail note elimination mode (reverse burst / CSS tail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssTailMode {
    None,
    /// 55 Hz audio tone burst prior to unkeying (traditional Motorola/Kenwood reverse burst).
    Tone55Hz,
    /// 120-degree phase shift in subaudible tone prior to unkeying.
    PhaseShift120Deg,
    /// 180-degree phase shift in subaudible tone prior to unkeying.
    PhaseShift180Deg,
}

/// 50 Standard EIA/TIA CTCSS Frequencies (in deci-Hz: 67.0 Hz to 254.1 Hz).
pub const CTCSS_TONES: [u16; 50] = [
    670, 693, 719, 744, 770, 797, 825, 854, 885, 915, 948, 974, 1000, 1035, 1072, 1109, 1148, 1188,
    1230, 1273, 1318, 1365, 1413, 1462, 1514, 1567, 1598, 1622, 1655, 1679, 1713, 1738, 1773, 1799,
    1835, 1862, 1899, 1928, 1966, 1995, 2035, 2065, 2107, 2181, 2257, 2291, 2336, 2418, 2503, 2541,
];

/// Declarative interface for Subaudible CTCSS/DCS hardware encoding, decoding, and tail elimination.
pub trait RfSubaudible {
    fn set_rx_subaudible(&mut self, tone: SubaudibleTone);
    fn set_tx_subaudible(&mut self, tone: SubaudibleTone);
    fn is_subaudible_matched(&mut self) -> bool;
    fn send_css_tail(&mut self, mode: CssTailMode);

    // Backward compatibility shim
    #[inline(always)]
    fn set_rx_tone(&mut self, tone: SubaudibleTone) {
        self.set_rx_subaudible(tone);
    }
    #[inline(always)]
    fn set_tx_tone(&mut self, tone: SubaudibleTone) {
        self.set_tx_subaudible(tone);
    }
    #[inline(always)]
    fn is_tone_matched(&mut self) -> bool {
        self.is_subaudible_matched()
    }
}
