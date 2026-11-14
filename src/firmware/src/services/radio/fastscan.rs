//! FastScan RSSI Spectrum Analyzer Engine.
//!
//! Sweeps up to 128 frequency bins (matching the 128-pixel LCD canvas width),
//! performing low-latency PLL frequency stepping, settling, and calibrated dBm
//! RSSI sampling with peak hold and noise floor estimation.

use kernel::service::radio::{PrecomputedTuneState, RfChannel, RfSquelch};
use utils::units::{Frequency, Hertz};

pub const FASTSCAN_COLUMNS: usize = 128;

/// FastScan operating mode and status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Idle,
    Sweeping { current_bin: u8 },
    Complete,
}

/// 128-bin Spectrum and Channel Sweep Engine.
#[derive(Debug, Clone)]
pub struct FastScanEngine {
    pub start_freq: Frequency<Hertz>,
    pub step_hz: u32,
    pub rssi_bins: [i8; FASTSCAN_COLUMNS],
    pub peak_bin: u8,
    pub peak_rssi: i16,
    pub peak_freq: Frequency<Hertz>,
    pub min_rssi: i16,
    pub status: ScanStatus,
    pub continuous: bool,
}

impl FastScanEngine {
    pub const fn new(start_freq_hz: u32, step_hz: u32) -> Self {
        Self {
            start_freq: Frequency::new(start_freq_hz),
            step_hz,
            rssi_bins: [-128; FASTSCAN_COLUMNS],
            peak_bin: 0,
            peak_rssi: -130,
            peak_freq: Frequency::new(start_freq_hz),
            min_rssi: -80,
            status: ScanStatus::Idle,
            continuous: true,
        }
    }

    /// Reset scan state and begin a new sweep from column 0.
    pub fn start(&mut self) {
        self.status = ScanStatus::Sweeping { current_bin: 0 };
        self.peak_rssi = -130;
        self.min_rssi = 0;
    }

    /// Stop scanning.
    pub fn stop(&mut self) {
        self.status = ScanStatus::Idle;
    }

    /// Begin spectrum sweep and acquire RF resource lock.
    pub fn start_with_coordinator(
        &mut self,
        coord: &mut super::coordinator::RadioCoordinator,
        slot: kernel::service::radio::VfoSlot,
    ) -> bool {
        let res = super::coordinator::RfResourceMask::for_vfo(slot)
            .union(super::coordinator::RfResourceMask::AUDIO_OUTPUT);
        if coord.request_resource_lock(
            super::coordinator::ClientToken::new(
                super::coordinator::ClientId::SCANNER,
                super::coordinator::LockTier::SERVICE,
            ),
            res,
            slot,
            10_000,
            true,
        ) {
            self.start();
            true
        } else {
            false
        }
    }

    /// Stop spectrum sweep and release RF resource lock.
    pub fn stop_with_coordinator(&mut self, coord: &mut super::coordinator::RadioCoordinator) {
        self.stop();
        coord.release_vfo_lock(super::coordinator::ClientToken::new(
            super::coordinator::ClientId::SCANNER,
            super::coordinator::LockTier::SERVICE,
        ));
    }

    /// Configure frequency range for the sweep.
    pub fn set_range(&mut self, start_freq_hz: u32, step_hz: u32) {
        self.start_freq = Frequency::new(start_freq_hz);
        self.step_hz = step_hz;
        self.start();
    }

    /// Center-span configuration: centers the 128 bins on `center_freq_hz`.
    pub fn set_center_span(&mut self, center_freq_hz: u32, span_hz: u32) {
        const _: () = assert!(FASTSCAN_COLUMNS == 128);
        let step_hz = (span_hz >> 7).max(1000);
        let half_span = step_hz << 6;
        let start_freq_hz = center_freq_hz.saturating_sub(half_span);
        self.set_range(start_freq_hz, step_hz);
    }

    /// Calculate frequency in Hertz for a specific bin index (0..127).
    #[inline(always)]
    pub fn bin_freq(&self, bin: u8) -> Frequency<Hertz> {
        let offset = (bin as u32).saturating_mul(self.step_hz);
        Frequency::new(self.start_freq.raw().saturating_add(offset))
    }

    /// Execute a single step of the spectrum sweep.
    /// Returns `true` if a full 128-column sweep completed on this tick.
    pub fn step<R: RfChannel + RfSquelch>(&mut self, radio: &mut R) -> bool {
        let cur_bin = match self.status {
            ScanStatus::Sweeping { current_bin } => current_bin,
            _ => return false,
        };

        // Tune to current bin frequency using zero-division precomputed PLL state
        let freq = self.bin_freq(cur_bin);
        let tune_state = PrecomputedTuneState::from_freq_hz(freq.raw());
        radio.apply_precomputed_tune(&tune_state);
        radio.wait_pll_lock(125);

        // Sample RSSI
        let rssi = radio.get_rssi_dbm();
        self.rssi_bins[cur_bin as usize] = rssi.clamp(-128, 127) as i8;

        // Track peak
        if rssi > self.peak_rssi {
            self.peak_rssi = rssi;
            self.peak_bin = cur_bin;
            self.peak_freq = freq;
        }

        // Track noise floor minimum
        if rssi < self.min_rssi {
            self.min_rssi = rssi;
        }

        let next_bin = cur_bin + 1;
        if next_bin as usize >= FASTSCAN_COLUMNS {
            if self.continuous {
                self.status = ScanStatus::Sweeping { current_bin: 0 };
            } else {
                self.status = ScanStatus::Complete;
            }
            true
        } else {
            self.status = ScanStatus::Sweeping {
                current_bin: next_bin,
            };
            false
        }
    }

    /// Execute a pipelined batch of `count` frequency bins in a single frame tick.
    /// Enables real-time waterfall sweep rates (hundreds of bins/sec) without stalling UI.
    pub fn step_batch<R: RfChannel + RfSquelch>(&mut self, radio: &mut R, count: u8) -> bool {
        let mut completed = false;
        for _ in 0..count.max(1) {
            if self.step(radio) {
                completed = true;
                if !self.continuous {
                    break;
                }
            }
        }
        completed
    }
}

impl super::coordinator::RfClientParticipant for FastScanEngine {
    #[inline(always)]
    fn client_id(&self) -> super::coordinator::ClientId {
        super::coordinator::ClientId::SCANNER
    }

    #[inline(always)]
    fn default_tier(&self) -> super::coordinator::LockTier {
        super::coordinator::LockTier::SERVICE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastscan_center_span_and_saturation() {
        let mut engine = FastScanEngine::new(144_000_000, 12_500);
        engine.set_center_span(146_520_000, 1_280_000);
        assert_eq!(engine.step_hz, 10_000);
        assert_eq!(engine.start_freq.raw(), 145_880_000);
        assert_eq!(engine.bin_freq(64).raw(), 146_520_000);

        engine.set_range(u32::MAX - 500, 1000);
        assert_eq!(engine.bin_freq(127).raw(), u32::MAX);
    }
}
