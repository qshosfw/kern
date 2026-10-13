//! Modular Dual Watch & Scanner Dwell Controller.
//!
//! Encapsulates time-division channel dwelling timers, post-carrier resume delays,
//! priority check intervals, and adaptive dwelling calculations for multi-VFO coordination.

/// Controller for channel dwelling, priority scanning intervals, and post-carrier resume delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DwellController {
    /// Active dwell timer (accumulates elapsed ms on the current channel).
    pub dwell_timer_ms: u16,
    /// Post-carrier hold countdown timer in ms.
    pub rx_hold_timer_ms: u16,
    /// Configurable duration to hold on a channel after carrier drops before resuming scan/watch. Default: 1500 ms.
    pub resume_delay_ms: u16,
}

impl DwellController {
    pub const DEFAULT_RESUME_DELAY_MS: u16 = 1500;

    pub const fn new() -> Self {
        Self {
            dwell_timer_ms: 0,
            rx_hold_timer_ms: 0,
            resume_delay_ms: Self::DEFAULT_RESUME_DELAY_MS,
        }
    }

    /// Reset the dwell accumulation timer to 0.
    #[inline(always)]
    pub fn reset_dwell(&mut self) {
        self.dwell_timer_ms = 0;
    }

    /// Configure custom post-carrier resume delay.
    #[inline(always)]
    pub fn set_resume_delay(&mut self, delay_ms: u16) {
        self.resume_delay_ms = delay_ms;
    }

    /// Triggered when carrier squelch opens on the active channel.
    #[inline(always)]
    pub fn on_carrier_opened(&mut self) {
        self.rx_hold_timer_ms = self.resume_delay_ms;
    }

    /// Refresh carrier hold timer while signal remains active.
    #[inline(always)]
    pub fn on_carrier_refresh(&mut self) {
        self.rx_hold_timer_ms = self.resume_delay_ms;
    }

    /// Check if the post-carrier hold period is still active.
    #[inline(always)]
    pub fn is_rx_held(&self) -> bool {
        self.rx_hold_timer_ms > 0
    }

    /// Triggered when transmission concludes to hold the channel for potential replies.
    #[inline(always)]
    pub fn on_tx_finished(&mut self) {
        self.rx_hold_timer_ms = self.resume_delay_ms;
        self.dwell_timer_ms = 0;
    }

    /// Check if current dwell duration has exceeded the specified target.
    #[inline(always)]
    pub fn is_dwell_expired(&self, target_ms: u16) -> bool {
        self.dwell_timer_ms >= target_ms
    }

    /// Advance timers by 10ms.
    ///
    /// If post-carrier hold is active, decrements `rx_hold_timer_ms`.
    /// Otherwise, increments `dwell_timer_ms`.
    pub fn tick_10ms(&mut self) {
        if self.rx_hold_timer_ms > 0 {
            self.rx_hold_timer_ms = self.rx_hold_timer_ms.saturating_sub(10);
        } else {
            self.dwell_timer_ms = self.dwell_timer_ms.saturating_add(10);
        }
    }

    /// Calculate adaptive dwell target based on signal strength and historical activity.
    #[inline(always)]
    pub fn calc_adaptive_dwell(rssi_dbm: i16, activity_count: u16) -> u16 {
        if rssi_dbm > -105 || activity_count > 5 {
            500
        } else {
            250
        }
    }
}
