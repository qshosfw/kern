//! Decoupled Frequency & Memory Channel Scan State Machine (`ScanPlan`).
//!
//! Zero-heap, deterministic scanner engine usable by the microkernel coordinator,
//! background sweep services, and external `.krx` scanner applications.

/// Resume policy after a carrier is detected during scanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScanResumeMode {
    /// Pause on carrier until carrier drops + hang delay elapses (Carrier-Operated).
    Carrier = 0,
    /// Dwell for a fixed timeout even while carrier stays active (Time-Operated).
    Timed = 1,
    /// Stop scanning immediately upon the first valid carrier hit (Search / One-Shot).
    StopOnHit = 2,
}

/// Active state of a [`ScanPlan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScanState {
    Idle = 0,
    Stepping = 1,
    Holding = 2,
    Locked = 3,
}

/// Output action produced by [`ScanPlan::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStepResult {
    /// Remain on the current frequency; no retune needed.
    Stay,
    /// Retune receiver to the given frequency in Hz.
    Tune(u32),
    /// Carrier hit confirmed under [`ScanResumeMode::StopOnHit`]; scanner halted.
    HitStopped(u32),
}

/// Compact, zero-allocation frequency range scanner state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ScanPlan {
    pub start_hz: u32,
    pub end_hz: u32,
    pub current_hz: u32,
    pub step_hz: u32,
    pub dwell_ms: u16,
    pub hang_ms: u16,
    pub elapsed_ms: u16,
    pub resume: ScanResumeMode,
    pub state: ScanState,
}

impl ScanPlan {
    pub const fn new(
        start_hz: u32,
        end_hz: u32,
        step_hz: u32,
        dwell_ms: u16,
        hang_ms: u16,
        resume: ScanResumeMode,
    ) -> Self {
        let (lo, hi) = if start_hz <= end_hz {
            (start_hz, end_hz)
        } else {
            (end_hz, start_hz)
        };
        let step = if step_hz == 0 { 12_500 } else { step_hz };
        Self {
            start_hz: lo,
            end_hz: hi,
            current_hz: lo,
            step_hz: step,
            dwell_ms,
            hang_ms,
            elapsed_ms: 0,
            resume,
            state: ScanState::Idle,
        }
    }

    #[inline(always)]
    pub fn start(&mut self) -> u32 {
        self.current_hz = self.start_hz;
        self.elapsed_ms = 0;
        self.state = ScanState::Stepping;
        self.current_hz
    }

    #[inline(always)]
    pub fn stop(&mut self) {
        self.state = ScanState::Idle;
        self.elapsed_ms = 0;
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        matches!(self.state, ScanState::Stepping | ScanState::Holding)
    }

    /// Advance the scan state machine by `dt_ms` given current `carrier_active`.
    pub fn tick(&mut self, dt_ms: u16, carrier_active: bool) -> ScanStepResult {
        match self.state {
            ScanState::Idle | ScanState::Locked => ScanStepResult::Stay,
            ScanState::Stepping => {
                if carrier_active {
                    self.elapsed_ms = 0;
                    if self.resume == ScanResumeMode::StopOnHit {
                        self.state = ScanState::Locked;
                        return ScanStepResult::HitStopped(self.current_hz);
                    }
                    self.state = ScanState::Holding;
                    return ScanStepResult::Stay;
                }
                self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
                if self.elapsed_ms >= self.dwell_ms {
                    self.elapsed_ms = 0;
                    let next = self.current_hz.saturating_add(self.step_hz);
                    self.current_hz = if next > self.end_hz {
                        self.start_hz
                    } else {
                        next
                    };
                    ScanStepResult::Tune(self.current_hz)
                } else {
                    ScanStepResult::Stay
                }
            }
            ScanState::Holding => {
                let should_count = match self.resume {
                    ScanResumeMode::Carrier => !carrier_active,
                    ScanResumeMode::Timed => true,
                    ScanResumeMode::StopOnHit => false,
                };
                if should_count {
                    self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
                    if self.elapsed_ms >= self.hang_ms {
                        self.elapsed_ms = 0;
                        self.state = ScanState::Stepping;
                        let next = self.current_hz.saturating_add(self.step_hz);
                        self.current_hz = if next > self.end_hz {
                            self.start_hz
                        } else {
                            next
                        };
                        return ScanStepResult::Tune(self.current_hz);
                    }
                } else {
                    self.elapsed_ms = 0;
                }
                ScanStepResult::Stay
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_plan_steps_wraps_and_holds_on_carrier() {
        let mut plan = ScanPlan::new(
            144_000_000,
            144_025_000,
            12_500,
            40,
            200,
            ScanResumeMode::Carrier,
        );
        assert_eq!(plan.start(), 144_000_000);
        assert_eq!(plan.tick(20, false), ScanStepResult::Stay);
        assert_eq!(plan.tick(20, false), ScanStepResult::Tune(144_012_500));
        // Carrier detected -> enters Holding
        assert_eq!(plan.tick(10, true), ScanStepResult::Stay);
        assert_eq!(plan.state, ScanState::Holding);
        // Carrier remains active -> stays holding
        assert_eq!(plan.tick(500, true), ScanStepResult::Stay);
        // Carrier drops -> waits hang_ms before advancing
        assert_eq!(plan.tick(100, false), ScanStepResult::Stay);
        assert_eq!(plan.tick(100, false), ScanStepResult::Tune(144_025_000));
        // Next step wraps around to start_hz
        assert_eq!(plan.tick(40, false), ScanStepResult::Tune(144_000_000));
    }

    #[test]
    fn scan_plan_stop_on_hit_locks_frequency() {
        let mut plan = ScanPlan::new(
            430_000_000,
            440_000_000,
            25_000,
            30,
            100,
            ScanResumeMode::StopOnHit,
        );
        plan.start();
        assert_eq!(plan.tick(30, false), ScanStepResult::Tune(430_025_000));
        assert_eq!(plan.tick(5, true), ScanStepResult::HitStopped(430_025_000));
        assert_eq!(plan.state, ScanState::Locked);
        assert!(!plan.is_active());
    }
}
