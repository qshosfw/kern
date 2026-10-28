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

pub struct BatteryDriver {
    pub calib_const: u16,
    pub calib_tweak_mv: i16,
    battery_type: Cell<BatteryType>,
    smoothed_mv: u16,
    ocv_mv: u16,
    last_mv: u16,
    base_mv: u16,
    pub last_raw_adc: u16,
    pub last_vdda: u16,
    pub last_temp: i16,
    #[allow(dead_code)]
    internal_res_m_ohm: u16,
    pub charge_detect_enabled: bool,
    is_charging: bool,
    charge_counter: u8,
    tx_recovery_secs: u8,
    pre_tx_mv: u16,
    drift_counter: u8,
}

impl BatteryDriver {
    pub const fn empty() -> Self {
        Self {
            calib_const: 0,
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
            tx_recovery_secs: 0,
            pre_tx_mv: 0,
            drift_counter: 0,
        }
    }

    pub const fn new() -> Self {
        Self {
            calib_const: 2227, // Default fallback to factory radio dump baseline (2227)
            calib_tweak_mv: 0,
            battery_type: Cell::new(BatteryType::Type1600mAh),
            smoothed_mv: 8200,
            ocv_mv: 8200,
            last_mv: 8200,
            base_mv: 8200,
            last_raw_adc: 0,
            last_vdda: 3300,
            last_temp: 250,
            internal_res_m_ohm: 150,
            charge_detect_enabled: true,
            is_charging: false,
            charge_counter: 0,
            tx_recovery_secs: 0,
            pre_tx_mv: 0,
            drift_counter: 0,
        }
    }

    pub fn set_battery_type(&self, bat_type: BatteryType) {
        self.battery_type.set(bat_type);
    }

    /// Converts raw 12-bit ADC reading (Channel 8 / PB0) to battery millivolts using dynamic or factory calibration + user tweak.
    #[inline(always)]
    pub fn adc_to_mv(&self, raw_adc: u16, _vdda_mv: u16) -> u16 {
        if raw_adc == 0 {
            return 0;
        }
        let cal = if self.calib_const >= 1000 && self.calib_const <= 3500 {
            self.calib_const as u32
        } else {
            2227
        };
        // Preserving 1mV resolution: (raw_adc * 7600) / cal
        let base_mv = ((raw_adc as u32 * 7600) / cal) as i32;
        (base_mv + self.calib_tweak_mv as i32).clamp(0, 15000) as u16
    }

    pub fn load_calibration_data(&mut self, calib_data: &[u8]) {
        if calib_data.len() >= 0x148 {
            let word3 = u16::from_le_bytes([calib_data[0x146], calib_data[0x147]]);
            if word3 >= 1000 && word3 <= 3500 {
                self.calib_const = word3;
            }
        }
    }

    /// Apply a user calibration offset.
    pub fn set_calib_tweak(&mut self, mv: i16) {
        let delta = mv as i32 - self.calib_tweak_mv as i32;
        if delta == 0 {
            return;
        }
        self.calib_tweak_mv = mv;
        let shift = |v: u16| {
            if v == 0 {
                0
            } else {
                (v as i32 + delta).clamp(0, 15000) as u16
            }
        };
        self.smoothed_mv = shift(self.smoothed_mv);
        self.ocv_mv = shift(self.ocv_mv);
        self.last_mv = shift(self.last_mv);
        self.base_mv = shift(self.base_mv);
        self.pre_tx_mv = shift(self.pre_tx_mv);
        self.charge_counter = 0;
        self.drift_counter = 0;
    }

    /// Calculates dynamic total system current draw in mA based on active hardware peripherals.
    pub fn estimate_sys_current(periph: &PeripheralState, profile: PowerProfile) -> u16 {
        let base_mcu = match profile {
            PowerProfile::Balanced => 14,
            PowerProfile::Performance => 22,
            PowerProfile::PowerSave => 8,
            PowerProfile::DeepSleep => 1,
        };

        let backlight_ma = (periph.backlight_level as u16) * 2; // 0..20mA
        let keypad_ma = if periph.keypad_led { 8 } else { 0 };
        let audio_ma = if periph.audio_active { 75 } else { 0 };

        let rf_ma = if periph.tx_active {
            match periph.tx_power_level {
                0 => 550,  // Low Power (500mW)
                1 => 1050, // Mid Power (2.5W)
                _ => 1650, // High Power (5W)
            }
        } else if periph.rx_active {
            90 // Receiver ON
        } else {
            0
        };

        base_mcu + backlight_ma + keypad_ma + audio_ma + rf_ma
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
                self.smoothed_mv = raw_mv;
                self.ocv_mv = raw_mv;
                self.base_mv = raw_mv;
                self.last_mv = raw_mv;
            } else {
                self.smoothed_mv = ((self.smoothed_mv as u32 * 7 + raw_mv as u32 + 4) / 8) as u16;
                self.ocv_mv = self.smoothed_mv;
            }
        }

        self.finish_state(raw_adc, raw_mv, periph, profile)
    }

    fn finish_state(
        &mut self,
        raw_adc: u16,
        _raw_mv: u16,
        periph: &PeripheralState,
        profile: PowerProfile,
    ) -> BatteryState {
        let sys_ma = Self::estimate_sys_current(periph, profile);

        let is_external = self.battery_type.get() == BatteryType::TypeExternal;

        if !self.charge_detect_enabled || is_external {
            self.is_charging = false;
            self.charge_counter = 0;
            self.base_mv = self.smoothed_mv;
        } else if periph.tx_active {
            if self.tx_recovery_secs == 0 {
                self.pre_tx_mv = self.smoothed_mv;
            }
            self.tx_recovery_secs = 125; // 25s post-TX recovery window (at 200ms ticks)
            self.charge_counter = 0;
        } else if self.tx_recovery_secs > 0 {
            self.tx_recovery_secs -= 1;
            self.charge_counter = 0;
            if self.smoothed_mv <= self.pre_tx_mv.saturating_add(20) {
                self.base_mv = self.smoothed_mv;
            } else {
                self.tx_recovery_secs = 0;
            }
        } else {
            const CHARGE_START_DELTA_MV: u16 = 35;
            const CHARGE_STOP_DROP_MV: u16 = 50;

            if !self.is_charging {
                if self.smoothed_mv >= self.base_mv.saturating_add(CHARGE_START_DELTA_MV) {
                    self.charge_counter = self.charge_counter.saturating_add(1);
                    if self.charge_counter >= 3 {
                        self.is_charging = true;
                        self.base_mv = self.smoothed_mv;
                    }
                } else {
                    self.charge_counter = self.charge_counter.saturating_sub(1);
                    if self.smoothed_mv < self.base_mv {
                        self.base_mv = self.smoothed_mv;
                    } else {
                        self.drift_counter = self.drift_counter.saturating_add(1);
                        if self.drift_counter >= 25 {
                            self.base_mv = self.base_mv.saturating_add(1);
                            self.drift_counter = 0;
                        }
                    }
                }
            } else {
                if self.smoothed_mv > self.base_mv {
                    self.base_mv = self.smoothed_mv;
                } else if self.smoothed_mv <= self.base_mv.saturating_sub(CHARGE_STOP_DROP_MV) {
                    self.is_charging = false;
                    self.charge_counter = 0;
                    self.base_mv = self.smoothed_mv;
                }
            }
        }

        self.last_mv = self.smoothed_mv;

        let percentage = mv_to_percentage(self.smoothed_mv, self.battery_type.get());
        let is_low = !is_external && (self.smoothed_mv < 6500 || percentage < 15);
        let is_critical = !is_external && (self.smoothed_mv < 6000 || percentage == 0);

        BatteryState {
            raw_adc,
            millivolts: self.smoothed_mv,
            ocv_millivolts: self.ocv_mv,
            percentage,
            current_ma: sys_ma,
            is_charging: self.is_charging,
            is_low,
            is_critical,
        }
    }

    /// Re-evaluates battery metrics immediately after calibration or chemistry tweak
    pub fn recalculate(&mut self, periph: &PeripheralState, profile: PowerProfile) -> BatteryState {
        let raw_adc = self.last_raw_adc;
        let vdda_mv = self.last_vdda;
        let raw_mv = self.adc_to_mv(raw_adc, vdda_mv);

        if raw_mv > 0 {
            self.smoothed_mv = raw_mv;
            self.ocv_mv = raw_mv;
            if !self.is_charging {
                self.base_mv = raw_mv;
            }
            self.charge_counter = 0;
        }

        self.finish_state(raw_adc, raw_mv, periph, profile)
    }
}
