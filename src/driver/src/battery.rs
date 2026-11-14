//! Battery Driver, Multi-Chemistry Discharge Curves & Industry-Standard System Power Management.

pub mod curve;
pub mod power;

pub use curve::*;
pub use power::*;

use core::cell::Cell;

/// Battery telemetry state representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryState {
    pub raw_adc: u16,
    pub millivolts: u16,
    pub ocv_millivolts: u16,
    pub percentage: u8,
    pub current_ma: u16,
    pub is_charging: bool,
    pub is_low: bool,
    pub is_critical: bool,
}

const DEFAULT_CALIB_CONST: u16 = 2227;
const DEFAULT_CAL_SCALE_Q16: u32 = (7600u32 << 16) / DEFAULT_CALIB_CONST as u32;

#[repr(C)]
pub struct BatteryDriver {
    // ── Hot 1-byte scalars (offset 0..10: 1-insn ldrb/strb) ─────────
    battery_type: Cell<BatteryType>,
    pub has_calib_curve: bool,
    pub charge_detect_enabled: bool,
    is_charging: bool,
    charge_counter: u8,
    tx_recovery_ticks: u8,
    tx_active_prev: bool,
    drift_counter: u8,
    display_pct: u8,
    is_low_state: bool,
    is_critical_state: bool,
    _pad: u8,
    // ── 2-byte scalars (offset 12..34) ─────────────────────────────
    pub calib_const: u16,
    pub calib_tweak_mv: i16,
    smoothed_mv: u16,
    ocv_mv: u16,
    last_mv: u16,
    base_mv: u16,
    pub last_raw_adc: u16,
    pub last_vdda: u16,
    pub last_temp: i16,
    pub internal_res_m_ohm: u16,
    pre_tx_ocv: u16,
    pub warn_threshold_mv: u16,
    pub shut_threshold_mv: u16,
    _pad2: u16,
    // ── 4-byte scalars & curve array ───────────────────────────────
    cal_scale_q16: u32,
    pub calib_curve: [u16; 6],
}

impl BatteryDriver {
    pub const fn empty() -> Self {
        Self {
            calib_const: 0,
            cal_scale_q16: 0,
            calib_curve: [0; 6],
            has_calib_curve: false,
            calib_tweak_mv: 0,
            battery_type: Cell::new(BatteryType::Type1600mAh),
            smoothed_mv: 0,
            ocv_mv: 0,
            last_mv: 0,
            base_mv: 0,
            last_raw_adc: 0,
            last_vdda: 0,
            last_temp: 0,
            internal_res_m_ohm: 0,
            charge_detect_enabled: false,
            is_charging: false,
            charge_counter: 0,
            tx_recovery_ticks: 0,
            pre_tx_ocv: 0,
            tx_active_prev: false,
            drift_counter: 0,
            display_pct: 0,
            is_low_state: false,
            is_critical_state: false,
            warn_threshold_mv: 0,
            shut_threshold_mv: 0,
            _pad: 0,
            _pad2: 0,
        }
    }

    pub const fn new() -> Self {
        Self {
            calib_const: DEFAULT_CALIB_CONST, // Default fallback to factory radio dump baseline (2227)
            cal_scale_q16: DEFAULT_CAL_SCALE_Q16,
            calib_curve: [1438, 2050, 2138, 2227, 2314, 2600],
            has_calib_curve: true,
            calib_tweak_mv: 0,
            battery_type: Cell::new(BatteryType::Type1600mAh),
            smoothed_mv: 8200,
            ocv_mv: 8200,
            last_mv: 8200,
            base_mv: 8200,
            last_raw_adc: 0,
            last_vdda: 3300,
            last_temp: 250,
            internal_res_m_ohm: 180,
            charge_detect_enabled: true,
            is_charging: false,
            charge_counter: 0,
            tx_recovery_ticks: 0,
            pre_tx_ocv: 8200,
            tx_active_prev: false,
            drift_counter: 0,
            display_pct: 100,
            is_low_state: false,
            is_critical_state: false,
            warn_threshold_mv: 6800,
            shut_threshold_mv: 6300,
            _pad: 0,
            _pad2: 0,
        }
    }

    pub fn set_thresholds(&mut self, warn_mv: u16, shut_mv: u16) {
        // Enforce physical limits for 2S Li-ion battery pack:
        // Warn threshold: 6400 mV (3.2V/cell) to 7100 mV (3.55V/cell), default 6800 mV (3.4V/cell)
        // Shut threshold: 6000 mV (3.0V/cell) to 6500 mV (3.25V/cell), default 6300 mV (3.15V/cell)
        self.warn_threshold_mv = if warn_mv >= 6400 && warn_mv <= 7100 {
            warn_mv
        } else {
            6800
        };
        self.shut_threshold_mv = if shut_mv >= 6000 && shut_mv <= 6500 {
            shut_mv
        } else {
            6300
        };
    }

    pub fn set_battery_type(&self, bat_type: BatteryType) {
        self.battery_type.set(bat_type);
    }

    #[inline(always)]
    pub fn battery_type(&self) -> BatteryType {
        self.battery_type.get()
    }

    #[inline(never)]
    pub fn set_calib_const(&mut self, calib_const: u16) {
        if calib_const >= 1000 && calib_const <= 3500 {
            self.calib_const = calib_const;
            self.cal_scale_q16 = (7600u32 << 16) / calib_const as u32;
        }
    }

    /// Converts raw 12-bit ADC reading (Channel 8 / PB0) to battery millivolts using precomputed Q16 calibration + user tweak.
    #[inline(always)]
    pub fn adc_to_mv(&self, raw_adc: u16, _vdda_mv: u16) -> u16 {
        if raw_adc == 0 {
            return 0;
        }
        let scale_q16 = if self.cal_scale_q16 != 0 {
            self.cal_scale_q16
        } else {
            DEFAULT_CAL_SCALE_Q16
        };
        let base_mv = (((raw_adc as u32 * scale_q16) + 0x8000) >> 16) as i32;
        (base_mv + self.calib_tweak_mv as i32).clamp(0, 15000) as u16
    }

    pub fn load_calibration_data(&mut self, calib_data: &[u8]) {
        if calib_data.len() >= 0x14C {
            for i in 0..6 {
                self.calib_curve[i] =
                    u16::from_le_bytes([calib_data[0x140 + i * 2], calib_data[0x140 + i * 2 + 1]]);
            }
            self.has_calib_curve = true;
            self.set_calib_const(self.calib_curve[3]);
        }
    }

    pub fn load_calibration_curve(&mut self, curve: [u16; 6]) {
        self.calib_curve = curve;
        self.has_calib_curve = true;
        self.set_calib_const(curve[3]);
    }

    /// Apply a user calibration offset.
    pub fn set_calib_tweak(&mut self, mv: i16) {
        let delta = mv as i32 - self.calib_tweak_mv as i32;
        if delta == 0 {
            return;
        }
        self.calib_tweak_mv = mv;
        for field in [
            &mut self.smoothed_mv,
            &mut self.ocv_mv,
            &mut self.last_mv,
            &mut self.base_mv,
            &mut self.pre_tx_ocv,
        ] {
            if *field != 0 {
                *field = (*field as i32 + delta).clamp(0, 15000) as u16;
            }
        }
        self.charge_counter = 0;
        self.drift_counter = 0;
        if self.ocv_mv > 0 {
            self.display_pct = self.calculate_percentage(self.ocv_mv);
        }
    }

    fn ema_u16(cur: u16, sample: u16, shift: u32) -> u16 {
        let weight = (1u32 << shift) - 1;
        let round = 1u32 << (shift - 1);
        ((cur as u32 * weight + sample as u32 + round) >> shift) as u16
    }

    fn eval_hysteresis(active: bool, ocv_mv: u16, pct: u8, limit_mv: u16, enter_pct: u8) -> bool {
        if active {
            !(ocv_mv >= limit_mv.saturating_add(150) && pct >= enter_pct - 2)
        } else {
            (ocv_mv < limit_mv && pct <= enter_pct) || pct <= enter_pct - 5
        }
    }

    /// Calculates dynamic total system current draw in mA from measured voltage sag (Thévenin load model)
    /// without relying on arbitrary hardcoded current tables.
    pub fn measure_load_ma(
        v_term: u16,
        v_ocv: u16,
        internal_res_m_ohm: u16,
        _periph: &PeripheralState,
        profile: PowerProfile,
    ) -> u16 {
        // Quiescent base current for MCU and background peripherals
        let quiescent_ma = match profile {
            PowerProfile::Balanced => 15,
            PowerProfile::Performance => 22,
            PowerProfile::PowerSave => 8,
            PowerProfile::DeepSleep => 1,
        };

        let r_int = if internal_res_m_ohm >= 80 {
            internal_res_m_ohm as u32
        } else {
            180
        };

        if v_term >= v_ocv {
            return quiescent_ma;
        }

        let sag_mv = v_ocv.saturating_sub(v_term);

        // Filter out ADC quantization noise floor (~4 mV = ~2 LSBs)
        if sag_mv <= 4 {
            return quiescent_ma;
        }

        let dynamic_ma = ((sag_mv as u32 * 1000) / r_int) as u16;
        quiescent_ma.saturating_add(dynamic_ma)
    }

    #[inline(never)]
    fn calculate_percentage(&self, ocv: u16) -> u8 {
        if self.has_calib_curve {
            let cal = if self.calib_const >= 1000 && self.calib_const <= 3500 {
                self.calib_const as u32
            } else {
                2227
            };
            let eff_adc = utils::math::div7600_u32(ocv as u32 * cal) as u16;
            calib::calculate_battery_percentage(eff_adc, &self.calib_curve)
        } else {
            mv_to_percentage(ocv, self.battery_type.get())
        }
    }

    fn seed_mv(&mut self, raw_mv: u16) {
        self.smoothed_mv = raw_mv;
        self.ocv_mv = raw_mv;
        self.pre_tx_ocv = raw_mv;
        if !self.is_charging {
            self.base_mv = raw_mv;
        }
        self.charge_counter = 0;
        self.display_pct = self.calculate_percentage(raw_mv);
    }

    /// Sample ADC with live responsive EMA filtering matching DeltaFW.
    pub fn sample(
        &mut self,
        raw_adc: u16,
        vdda_mv: u16,
        mcu_temp_deci_c: i16,
        periph: &PeripheralState,
        profile: PowerProfile,
    ) -> BatteryState {
        let first_sample = self.last_raw_adc == 0;
        self.last_raw_adc = raw_adc;
        self.last_vdda = vdda_mv;
        self.last_temp = mcu_temp_deci_c;

        let raw_mv = self.adc_to_mv(raw_adc, vdda_mv);

        // Symmetric 8-tap EMA smoothing: filters sound beeps, screen dimming, and inrush noise
        if raw_mv > 0 {
            if first_sample || self.base_mv == 0 {
                self.seed_mv(raw_mv);
            } else {
                self.smoothed_mv = Self::ema_u16(self.smoothed_mv, raw_mv, 3);
            }
        }

        self.finish_state(raw_adc, raw_mv, periph, profile)
    }

    fn finish_state(
        &mut self,
        raw_adc: u16,
        raw_mv: u16,
        periph: &PeripheralState,
        profile: PowerProfile,
    ) -> BatteryState {
        let is_external = self.battery_type.get() == BatteryType::TypeExternal;
        let sm = self.smoothed_mv;

        let tx_was_active = self.tx_active_prev;
        self.tx_active_prev = periph.tx_active;

        if !self.charge_detect_enabled || is_external {
            self.is_charging = false;
            self.charge_counter = 0;
            self.base_mv = sm;
        } else if periph.tx_active {
            if !tx_was_active {
                self.pre_tx_ocv = self.ocv_mv;
            } else if self.pre_tx_ocv > raw_mv {
                // Refine internal resistance estimate from measured instantaneous raw step sag during TX
                let step_sag = self.pre_tx_ocv - raw_mv;
                if (50..=600).contains(&step_sag) {
                    let r_est = match periph.tx_power_level {
                        0 => step_sag << 1,
                        1 => step_sag,
                        _ => (((step_sag as u32) * 171) >> 8) as u16,
                    };
                    if (100..=500).contains(&r_est) {
                        self.internal_res_m_ohm = Self::ema_u16(self.internal_res_m_ohm, r_est, 3);
                    }
                }
            }
            self.tx_recovery_ticks = 125; // 25s post-TX recovery window (at 200ms ticks)
            self.charge_counter = 0;
            self.ocv_mv = self.pre_tx_ocv;
        } else if self.tx_recovery_ticks > 0 {
            self.tx_recovery_ticks -= 1;
            self.charge_counter = 0;
            self.ocv_mv = self.pre_tx_ocv;

            // If terminal voltage has recovered within 15 mV of resting OCV, end recovery early
            if sm >= self.pre_tx_ocv.saturating_sub(15) {
                self.tx_recovery_ticks = 0;
                self.base_mv = sm;
            }
        } else {
            // Normal operation / discharge / charge detection
            const CHARGE_START_DELTA_MV: u16 = 35;
            const CHARGE_STOP_DROP_MV: u16 = 50;

            if !self.is_charging {
                if sm >= self.base_mv.saturating_add(CHARGE_START_DELTA_MV) {
                    self.charge_counter = self.charge_counter.saturating_add(1);
                    if self.charge_counter >= 3 {
                        self.is_charging = true;
                        self.base_mv = sm;
                        self.ocv_mv = sm;
                    }
                } else {
                    self.charge_counter = self.charge_counter.saturating_sub(1);
                    if sm < self.base_mv {
                        self.base_mv = sm;
                    } else {
                        self.drift_counter = self.drift_counter.saturating_add(1);
                        if self.drift_counter >= 25 {
                            self.base_mv = self.base_mv.saturating_add(1);
                            self.drift_counter = 0;
                        }
                    }

                    // Steady-state normal discharge tracking:
                    // If load sag is small (<= 25 mV), it is normal steady-state discharge.
                    // Slowly track OCV downwards with a 32-sample EMA (~6.4s time constant at 5 Hz).
                    if sm <= self.ocv_mv {
                        if self.ocv_mv - sm <= 25 {
                            self.ocv_mv = Self::ema_u16(self.ocv_mv, sm, 5);
                        }
                    } else if sm < self.base_mv.saturating_add(35) {
                        // Slight upward thermal drift <= 35 mV
                        self.ocv_mv = Self::ema_u16(self.ocv_mv, sm, 6);
                    }
                }
            } else {
                // Active charging
                self.ocv_mv = sm;
                if sm > self.base_mv {
                    self.base_mv = sm;
                } else if sm <= self.base_mv.saturating_sub(CHARGE_STOP_DROP_MV) {
                    self.is_charging = false;
                    self.charge_counter = 0;
                    self.base_mv = sm;
                }
            }
        }

        self.last_mv = sm;

        let raw_pct = self.calculate_percentage(self.ocv_mv);

        // Monotonic SoC Governor:
        // When discharging, the displayed percentage must never bounce upward due to transient load removal.
        // When charging or using external power, percentage tracks freely.
        let percentage = if is_external {
            100
        } else if self.is_charging {
            self.display_pct = raw_pct;
            raw_pct
        } else {
            if raw_pct < self.display_pct {
                self.display_pct = raw_pct;
            }
            self.display_pct
        };

        // Debounced Hysteresis State Evaluation:
        // Exiting low/critical state requires +150 mV margin or active charging to prevent threshold flutter.
        // Low battery is strictly guarded: percentage must be <= 15% (or <= 20% if OCV is below threshold).
        // It can NEVER spuriously trigger when battery is healthy (e.g. 77%).
        let warn_limit = if self.warn_threshold_mv > 0 {
            self.warn_threshold_mv
        } else {
            6800
        };
        let shut_limit = if self.shut_threshold_mv > 0 {
            self.shut_threshold_mv
        } else {
            6300
        };

        if raw_adc == 0 || self.smoothed_mv == 0 || is_external {
            self.is_low_state = false;
            self.is_critical_state = false;
        } else if self.is_charging {
            self.is_critical_state = self.ocv_mv < shut_limit && percentage == 0;
            self.is_low_state = self.ocv_mv < warn_limit && percentage < 5;
        } else {
            self.is_critical_state = Self::eval_hysteresis(
                self.is_critical_state,
                self.ocv_mv,
                percentage,
                shut_limit,
                5,
            );
            self.is_low_state =
                Self::eval_hysteresis(self.is_low_state, self.ocv_mv, percentage, warn_limit, 20);

            if self.is_critical_state {
                self.is_low_state = true;
            }
        }

        let sys_ma = Self::measure_load_ma(
            raw_mv,
            self.ocv_mv,
            self.internal_res_m_ohm,
            periph,
            profile,
        );

        BatteryState {
            raw_adc,
            millivolts: self.smoothed_mv,
            ocv_millivolts: self.ocv_mv,
            percentage,
            current_ma: sys_ma,
            is_charging: self.is_charging,
            is_low: self.is_low_state,
            is_critical: self.is_critical_state,
        }
    }

    /// Re-evaluates battery metrics immediately after calibration or chemistry tweak
    pub fn recalculate(&mut self, periph: &PeripheralState, profile: PowerProfile) -> BatteryState {
        let raw_adc = self.last_raw_adc;
        let vdda_mv = self.last_vdda;
        let raw_mv = self.adc_to_mv(raw_adc, vdda_mv);

        if raw_mv > 0 {
            self.seed_mv(raw_mv);
        }

        self.finish_state(raw_adc, raw_mv, periph, profile)
    }
}

impl Default for BatteryDriver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_low_battery_at_77_percent() {
        let mut bat = BatteryDriver::new();
        // ADC 2305 yields 77% on standard factory calibration curve
        let raw_adc = 2305u16;
        let periph = PeripheralState::default();
        let state = bat.sample(raw_adc, 3300, 250, &periph, PowerProfile::Balanced);

        assert_eq!(state.percentage, 77);
        assert!(!state.is_low, "Battery at 77% must NOT be flagged as low!");
        assert!(
            !state.is_critical,
            "Battery at 77% must NOT be flagged as critical!"
        );
    }

    #[test]
    fn test_uninitialized_adc_does_not_trigger_low_battery() {
        let mut bat = BatteryDriver::empty();
        let periph = PeripheralState::default();
        let state = bat.sample(0, 0, 0, &periph, PowerProfile::Balanced);

        assert!(!state.is_low, "Uninitialized ADC must not trigger low!");
        assert!(
            !state.is_critical,
            "Uninitialized ADC must not trigger critical!"
        );
    }

    #[test]
    fn test_monotonic_discharge_prevents_bounce() {
        let mut bat = BatteryDriver::new();
        let periph = PeripheralState::default();

        // Initial sample at ~79-80% (7900 mV)
        let adc_high = ((7900u32 * 2227) / 7600) as u16;
        let s1 = bat.sample(adc_high, 3300, 250, &periph, PowerProfile::Balanced);
        let initial_pct = s1.percentage;
        assert!(initial_pct >= 75);

        // Load causes temporary drop to 7600 mV (60%)
        let adc_load = ((7600u32 * 2227) / 7600) as u16;
        let s2 = bat.sample(adc_load, 3300, 250, &periph, PowerProfile::Balanced);
        // Voltage sag under load does not drag down resting OCV immediately
        assert_eq!(s2.percentage, initial_pct);

        // Load removed, terminal voltage rebounds
        let s3 = bat.sample(adc_high, 3300, 250, &periph, PowerProfile::Balanced);
        assert_eq!(s3.percentage, initial_pct);
    }

    #[test]
    fn test_dynamic_load_measurement() {
        let mut bat = BatteryDriver::new();
        let mut periph = PeripheralState::default();

        // Resting at 8200 mV
        let adc_rest = ((8200u32 * 2227) / 7600) as u16;
        let s_rest = bat.sample(adc_rest, 3300, 250, &periph, PowerProfile::Balanced);
        assert_eq!(s_rest.current_ma, 15); // Quiescent baseline

        // Key TX on High power, causing instantaneous raw voltage sag of 270 mV (to 7930 mV)
        periph.tx_active = true;
        periph.tx_power_level = 2;
        let adc_tx = ((7930u32 * 2227) / 7600) as u16;
        let s_tx = bat.sample(adc_tx, 3300, 250, &periph, PowerProfile::Balanced);

        // Expected load ~15 + (270 * 1000 / 180) = 1515 mA
        assert!(
            s_tx.current_ma >= 1400 && s_tx.current_ma <= 1650,
            "TX load should measure ~1.5A from physical voltage sag, got {}",
            s_tx.current_ma
        );
        // During TX, percentage remains stable at resting OCV!
        assert_eq!(s_tx.percentage, s_rest.percentage);
    }
}
