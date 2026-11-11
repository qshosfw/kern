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
    /// 240-degree phase shift in subaudible tone prior to unkeying.
    PhaseShift240Deg,
}

/// Compute the 23-bit TIA-603 Golay (23,12) codeword for an octal DCS code (e.g., `23` for D023N).
///
/// Uses fast reciprocal multiplication for digit extraction and the standard
/// DCS generator polynomial `g(x) = 0xC75` in 12 shift-XOR steps with zero lookup tables.
pub const fn dcs_octal_to_golay23(octal_code: u16) -> u32 {
    // Fast reciprocal division: ((x * 0xCCCD) >> 19) exact for all x <= 65,535
    let q1 = ((octal_code as u32 * 0xCCCD) >> 19) as u16;
    let d0 = (octal_code - q1 * 10) as u8;
    let d2 = ((q1 as u32 * 0xCCCD) >> 19) as u16;
    let d1 = (q1 - d2 * 10) as u8;

    let code_9bit = if octal_code > 0 && d0 < 8 && d1 < 8 && d2 < 8 && octal_code <= 777 {
        ((d2 as u32) << 6) | ((d1 as u32) << 3) | (d0 as u32)
    } else {
        (octal_code & 0x01FF) as u32
    };

    let info12 = (0b100u32 << 9) | (code_9bit as u32);
    let mut rem = info12;
    let mut i = 0;
    while i < 12 {
        if (rem & 1) != 0 {
            rem = (rem >> 1) ^ 0x0C75;
        } else {
            rem >>= 1;
        }
        i += 1;
    }
    ((rem & 0x07FF) << 12) | info12
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

    /// Check if hardware detected a subaudible reverse burst (120°/180°/240° phase inversion)
    /// or 55 Hz CTC2 squelch tail elimination burst (`Reg 0x0C` bits `[13:11]`).
    fn has_tail_burst_detected(&mut self) -> bool {
        false
    }
}

/// Receiver Squelch Tail Elimination (STE) operational state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxSteState {
    Idle,
    Monitoring,
    ToneLost { lost_count: u8 },
    Muted { mute_count: u8 },
}

/// Squelch Tail Elimination (STE) controller.
///
/// Detects loss of subaudible CTCSS/DCS tone 20ms before carrier drop,
/// commanding immediate audio mute to eliminate the loud squelch crash/noise burst on repeaters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxSteController {
    pub state: RxSteState,
}

impl RxSteController {
    pub const fn new() -> Self {
        Self {
            state: RxSteState::Idle,
        }
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.state = RxSteState::Idle;
    }

    /// Process a 10ms tick with optional hardware reverse-burst / 55 Hz tail detection.
    pub fn process_tick_with_tail(
        &mut self,
        carrier_open: bool,
        tone_configured: bool,
        tone_present: bool,
        tail_burst_detected: bool,
    ) -> Option<bool> {
        if !carrier_open {
            self.state = RxSteState::Idle;
            return None;
        }
        // Instantaneous reverse-burst (120°/180°/240° phase shift) or 55 Hz CTC2 tail detection:
        // Mute immediately on the first tick even if no RX CTCSS filter was configured!
        if tail_burst_detected && !matches!(self.state, RxSteState::Muted { .. }) {
            self.state = RxSteState::Muted { mute_count: 0 };
            return Some(true);
        }
        self.process_tick(carrier_open, tone_configured, tone_present)
    }

    /// Process a 10ms tick.
    ///
    /// Returns `Some(true)` if audio should be muted, `Some(false)` if audio should be unmuted,
    /// or `None` if no change in mute state is requested.
    pub fn process_tick(
        &mut self,
        carrier_open: bool,
        tone_configured: bool,
        tone_present: bool,
    ) -> Option<bool> {
        if !carrier_open || !tone_configured {
            self.state = RxSteState::Idle;
            return None;
        }

        match self.state {
            RxSteState::Idle => {
                self.state = RxSteState::Monitoring;
                None
            }
            RxSteState::Monitoring => {
                if !tone_present {
                    self.state = RxSteState::ToneLost { lost_count: 1 };
                }
                None
            }
            RxSteState::ToneLost { lost_count } => {
                if tone_present {
                    self.state = RxSteState::Monitoring;
                    None
                } else {
                    let next = lost_count + 1;
                    if next >= 2 {
                        // 20ms confirmed tone loss: MUTE NOW before noise burst!
                        self.state = RxSteState::Muted { mute_count: 0 };
                        Some(true)
                    } else {
                        self.state = RxSteState::ToneLost { lost_count: next };
                        None
                    }
                }
            }
            RxSteState::Muted { mute_count } => {
                let next = mute_count + 1;
                if tone_present && next > 3 {
                    // False alarm / fade recovered: unmute and resume monitoring
                    self.state = RxSteState::Monitoring;
                    Some(false)
                } else if next >= 15 {
                    // 150ms timeout: return to idle
                    self.state = RxSteState::Idle;
                    None
                } else {
                    self.state = RxSteState::Muted { mute_count: next };
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ste_initial_idle() {
        let ste = RxSteController::new();
        assert_eq!(ste.state, RxSteState::Idle);
    }

    #[test]
    fn test_ste_no_tone_configured_stays_idle() {
        let mut ste = RxSteController::new();
        let res = ste.process_tick(true, false, false);
        assert_eq!(res, None);
        assert_eq!(ste.state, RxSteState::Idle);
    }

    #[test]
    fn test_ste_carrier_open_and_monitoring() {
        let mut ste = RxSteController::new();
        let res = ste.process_tick(true, true, true);
        assert_eq!(res, None);
        assert_eq!(ste.state, RxSteState::Monitoring);

        // Keep monitoring while tone is present
        let res = ste.process_tick(true, true, true);
        assert_eq!(res, None);
        assert_eq!(ste.state, RxSteState::Monitoring);
    }

    #[test]
    fn test_ste_false_alarm_flutter() {
        let mut ste = RxSteController::new();
        ste.process_tick(true, true, true);
        assert_eq!(ste.state, RxSteState::Monitoring);

        // Tone lost for 10ms
        let res = ste.process_tick(true, true, false);
        assert_eq!(res, None);
        assert_eq!(ste.state, RxSteState::ToneLost { lost_count: 1 });

        // Tone recovered
        let res = ste.process_tick(true, true, true);
        assert_eq!(res, None);
        assert_eq!(ste.state, RxSteState::Monitoring);
    }

    #[test]
    fn test_ste_20ms_confirmed_loss_mutes() {
        let mut ste = RxSteController::new();
        ste.process_tick(true, true, true);

        // 1st tick of tone loss (10ms)
        let res1 = ste.process_tick(true, true, false);
        assert_eq!(res1, None);
        assert_eq!(ste.state, RxSteState::ToneLost { lost_count: 1 });

        // 2nd tick of tone loss (20ms confirmed): MUST MUTE
        let res2 = ste.process_tick(true, true, false);
        assert_eq!(res2, Some(true));
        assert_eq!(ste.state, RxSteState::Muted { mute_count: 0 });
    }

    #[test]
    fn test_ste_fade_recovery_unmutes() {
        let mut ste = RxSteController::new();
        ste.process_tick(true, true, true);
        ste.process_tick(true, true, false);
        ste.process_tick(true, true, false); // Muted { mute_count: 0 }

        // Mute ticks 1, 2, 3
        ste.process_tick(true, true, false);
        ste.process_tick(true, true, false);
        ste.process_tick(true, true, false);

        // Tone returns on mute_count 4 (> 3): MUST UNMUTE
        let res = ste.process_tick(true, true, true);
        assert_eq!(res, Some(false));
        assert_eq!(ste.state, RxSteState::Monitoring);
    }

    #[test]
    fn test_ste_150ms_timeout() {
        let mut ste = RxSteController::new();
        ste.process_tick(true, true, true);
        ste.process_tick(true, true, false);
        ste.process_tick(true, true, false); // Muted { mute_count: 0 }

        for _ in 0..14 {
            let res = ste.process_tick(true, true, false);
            assert_eq!(res, None);
        }
        // 15th tick (150ms)
        let res = ste.process_tick(true, true, false);
        assert_eq!(res, None);
        assert_eq!(ste.state, RxSteState::Idle);
    }

    #[test]
    fn test_ste_carrier_drop_resets() {
        let mut ste = RxSteController::new();
        ste.process_tick(true, true, true);
        assert_eq!(ste.state, RxSteState::Monitoring);

        ste.process_tick(false, true, true);
        assert_eq!(ste.state, RxSteState::Idle);
    }

    #[test]
    fn test_ste_instant_reverse_burst_tail_mute() {
        let mut ste = RxSteController::new();
        ste.process_tick_with_tail(true, true, true, false);
        assert_eq!(ste.state, RxSteState::Monitoring);

        // Hardware detects 120/180/240 deg phase shift or 55 Hz CTC2 tail -> mutes on very first tick!
        let res = ste.process_tick_with_tail(true, true, true, true);
        assert_eq!(res, Some(true));
        assert_eq!(ste.state, RxSteState::Muted { mute_count: 0 });
    }

    #[test]
    fn test_dcs_octal_to_golay23() {
        // D023N (octal 023 = 0x013, with bit 9 marker = 0x813) produces 23-bit codeword
        let cw023 = dcs_octal_to_golay23(23);
        assert_eq!(cw023 & 0x0FFF, (0b100 << 9) | 0o023);
        assert_ne!(cw023 >> 12, 0);
    }
}
