//! Battery Driver, Multi-Chemistry Discharge Curves & Industry-Standard System Power Management.

use core::cell::Cell;

#[cfg(target_arch = "arm")]
use cortex_m::asm;

/// Battery chemistry / capacity type profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryType {
    Type1600mAh = 0,
    Type2200mAh = 1,
    Type3500mAh = 2,
    Type1500mAhK1 = 3,
    Type2500mAhK1 = 4,
    TypeExternal = 5,
}

impl BatteryType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => BatteryType::Type1600mAh,
            1 => BatteryType::Type2200mAh,
            2 => BatteryType::Type3500mAh,
            3 => BatteryType::Type1500mAhK1,
            4 => BatteryType::Type2500mAhK1,
            5 => BatteryType::TypeExternal,
            _ => BatteryType::Type1600mAh,
        }
    }
}

/// Power save profiles for MCU execution and peripherals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    Balanced = 0,
    Performance = 1,
    PowerSave = 2,
    DeepSleep = 3,
}

impl PowerProfile {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => PowerProfile::Balanced,
            1 => PowerProfile::Performance,
            2 => PowerProfile::PowerSave,
            3 => PowerProfile::DeepSleep,
            _ => PowerProfile::Balanced,
        }
    }
}

/// High-precision 2S Li-ion discharge curves (10mV resolution units -> Percentage 0..100)
/// Modeled after industry 2S Li-ion cell characteristics (8.4V max, 7.4V nominal, 6.0V cutoff).
const VOLTAGE_CURVES: [&[(u16, u8)]; 5] = [
    // 1600 mAh Standard Pack (UV-K5)
    &[
        (838, 100),
        (820, 95),
        (780, 75),
        (745, 45),
        (715, 20),
        (670, 7),
        (600, 0),
    ],
    // 2200 mAh Extended Pack
    &[
        (839, 100),
        (825, 96),
        (785, 78),
        (750, 50),
        (720, 22),
        (680, 8),
        (600, 0),
    ],
    // 3500 mAh High Capacity Pack
    &[
        (840, 100),
        (828, 97),
        (790, 80),
        (755, 55),
        (725, 25),
        (685, 9),
        (600, 0),
    ],
    // 1500 mAh Compact Pack (K1)
    &[
        (836, 100),
        (818, 94),
        (778, 72),
        (742, 42),
        (712, 18),
        (668, 6),
        (600, 0),
    ],
    // 2500 mAh Extended Pack (K1)
    &[
        (839, 100),
        (826, 96),
        (788, 79),
        (752, 52),
        (722, 23),
        (682, 8),
        (600, 0),
    ],
];

/// Peripheral operational state for real-time dynamic current estimation.
#[derive(Debug, Clone, Copy, Default)]
pub struct PeripheralState {
    pub backlight_level: u8, // 0..10
    pub tx_active: bool,
    pub tx_power_level: u8, // 0=Low, 1=Mid, 2=High
    pub rx_active: bool,
    pub audio_active: bool,
    pub keypad_led: bool,
}

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

    /// Apply a user calibration offset. This is a reference-frame change, not a
    /// physical voltage step, so every tracked voltage is shifted by the same delta
    /// and the charge-edge detector observes zero relative change.
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

    /// High-precision piecewise linear interpolation to convert OCV battery millivolts to percentage.
    pub fn mv_to_percentage(mv: u16, bat_type: BatteryType) -> u8 {
        if bat_type == BatteryType::TypeExternal {
            return 100;
        }
        let v_10mv = mv / 10;
        let curve = VOLTAGE_CURVES[bat_type as usize];

        if v_10mv >= curve[0].0 {
            return 100;
        }

        for i in 1..curve.len() {
            let (v_prev, p_prev) = curve[i - 1];
            let (v_curr, p_curr) = curve[i];

            if v_10mv >= v_curr {
                let v_range = (v_prev - v_curr) as u32;
                if v_range == 0 {
                    return p_curr;
                }
                let p_range = (p_prev - p_curr) as u32;
                let v_diff = (v_10mv - v_curr) as u32;
                let interpolated = (p_curr as u32) + (v_diff * p_range) / v_range;
                return (interpolated.min(100)) as u8;
            }
        }

        0
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
            // Transmitting: record baseline resting voltage right before high current draw
            if self.tx_recovery_secs == 0 {
                self.pre_tx_mv = self.smoothed_mv;
            }
            self.tx_recovery_secs = 125; // 25s post-TX recovery window (at 200ms ticks)
            self.charge_counter = 0;
        } else if self.tx_recovery_secs > 0 {
            self.tx_recovery_secs -= 1;
            self.charge_counter = 0;
            // Prevent post-TX cell rebound from being misinterpreted as charger insertion
            if self.smoothed_mv <= self.pre_tx_mv.saturating_add(20) {
                self.base_mv = self.smoothed_mv;
            } else {
                self.tx_recovery_secs = 0;
            }
        } else {
            // Dual-threshold step tracking (DeltaFW proven):
            // Insertion into charger steps battery terminal voltage up by >= 35mV.
            // Unplugging drops voltage from charging peak by >= 50mV.
            const CHARGE_START_DELTA_MV: u16 = 35;
            const CHARGE_STOP_DROP_MV: u16 = 50;

            if !self.is_charging {
                if self.smoothed_mv >= self.base_mv.saturating_add(CHARGE_START_DELTA_MV) {
                    self.charge_counter = self.charge_counter.saturating_add(1);
                    if self.charge_counter >= 3 {
                        // Sustained rise for ~600ms: active charging started!
                        self.is_charging = true;
                        self.base_mv = self.smoothed_mv; // Base now tracks highest charge voltage
                    }
                } else {
                    self.charge_counter = self.charge_counter.saturating_sub(1);
                    if self.smoothed_mv < self.base_mv {
                        // Normal discharge: immediately follow baseline downward
                        self.base_mv = self.smoothed_mv;
                    } else {
                        // Natural relaxation: slowly drift base up (1mV per 5 seconds: 25 * 200ms)
                        self.drift_counter = self.drift_counter.saturating_add(1);
                        if self.drift_counter >= 25 {
                            self.base_mv = self.base_mv.saturating_add(1);
                            self.drift_counter = 0;
                        }
                    }
                }
            } else {
                // In active charge: track peak voltage achieved as cells fill towards 8.4V
                if self.smoothed_mv > self.base_mv {
                    self.base_mv = self.smoothed_mv;
                } else if self.smoothed_mv <= self.base_mv.saturating_sub(CHARGE_STOP_DROP_MV) {
                    // Voltage dropped 50mV from peak: charger unplugged!
                    self.is_charging = false;
                    self.charge_counter = 0;
                    self.base_mv = self.smoothed_mv;
                }
            }
        }

        self.last_mv = self.smoothed_mv;

        let percentage = Self::mv_to_percentage(self.smoothed_mv, self.battery_type.get());
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

pub struct SystemPower {
    pub battery_pct: u8,
    pub battery_mv: u16,
    pub battery_ma: u16,
    pub mcu_temp_deci_c: i16,
    pub vdda_mv: u16,
    pub uptime_secs: u32,
    pub inactivity_secs: u32,
    pub auto_power_off_min: u8,
    pub apo_warning_active: bool,
    pub apo_triggered: bool,
    pub is_charging: bool,
    pub charging_started: bool,
    pub charging_stopped: bool,
    pub is_low: bool,
    pub is_critical: bool,
    pub profile: PowerProfile,
    pub battery_driver: BatteryDriver,
}

impl SystemPower {
    pub const fn empty() -> Self {
        Self {
            battery_pct: 0,
            battery_mv: 0,
            battery_ma: 0,
            mcu_temp_deci_c: 0,
            vdda_mv: 0,
            uptime_secs: 0,
            inactivity_secs: 0,
            auto_power_off_min: 0,
            apo_warning_active: false,
            apo_triggered: false,
            is_charging: false,
            charging_started: false,
            charging_stopped: false,
            is_low: false,
            is_critical: false,
            profile: PowerProfile::Balanced,
            battery_driver: BatteryDriver::empty(),
        }
    }

    pub const fn new() -> Self {
        Self {
            battery_pct: 100,
            battery_mv: 8200,
            battery_ma: 35,
            mcu_temp_deci_c: 250,
            vdda_mv: 3300,
            uptime_secs: 0,
            inactivity_secs: 0,
            auto_power_off_min: 0,
            apo_warning_active: false,
            apo_triggered: false,
            is_charging: false,
            charging_started: false,
            charging_stopped: false,
            is_low: false,
            is_critical: false,
            profile: PowerProfile::Balanced,
            battery_driver: BatteryDriver::new(),
        }
    }

    pub fn set_battery_type(&self, bat_type: BatteryType) {
        self.battery_driver.set_battery_type(bat_type);
    }

    pub fn load_calibration_data(&mut self, calib_data: &[u8]) {
        self.battery_driver.load_calibration_data(calib_data);
    }

    pub fn notify_activity(&mut self) {
        self.inactivity_secs = 0;
        self.apo_warning_active = false;
    }

    fn apply_state(&mut self, state: &BatteryState) {
        let prev_charging = self.is_charging;
        self.battery_pct = state.percentage;
        self.battery_mv = state.millivolts;
        self.battery_ma = state.current_ma;
        self.is_charging = state.is_charging;
        self.is_low = state.is_low;
        self.is_critical = state.is_critical;

        if !prev_charging && self.is_charging {
            self.charging_started = true;
        } else if prev_charging && !self.is_charging {
            self.charging_stopped = true;
        }
    }

    /// Poll for charging state edge transitions.
    /// Returns `Some(true)` if charging just started, `Some(false)` if charging just stopped,
    /// or `None` if steady state.
    pub fn poll_charging_transition(&mut self) -> Option<bool> {
        if self.charging_started {
            self.charging_started = false;
            Some(true)
        } else if self.charging_stopped {
            self.charging_stopped = false;
            Some(false)
        } else {
            None
        }
    }

    /// Forces recalculation of battery percentage and voltage with current profile/calibration
    pub fn force_refresh(&mut self) {
        let periph = PeripheralState::default();
        let state = self.battery_driver.recalculate(&periph, self.profile);
        self.apply_state(&state);
    }

    /// Update power system metrics from raw hardware ADC readings and active peripheral states.
    pub fn update_adc(
        &mut self,
        raw_adc: u16,
        vdda_mv: u16,
        mcu_temp_deci_c: i16,
        periph: &PeripheralState,
    ) -> BatteryState {
        self.vdda_mv = vdda_mv;
        self.mcu_temp_deci_c = mcu_temp_deci_c;

        let state =
            self.battery_driver
                .sample(raw_adc, vdda_mv, mcu_temp_deci_c, periph, self.profile);

        self.apply_state(&state);
        state
    }

    /// Increment OS system uptime and evaluate Auto-Power-Off (APO) timeouts.
    pub fn tick_second(&mut self) {
        self.uptime_secs = self.uptime_secs.saturating_add(1);
        self.inactivity_secs = self.inactivity_secs.saturating_add(1);

        if self.auto_power_off_min > 0 {
            let limit_secs = (self.auto_power_off_min as u32) * 60;
            if self.inactivity_secs >= limit_secs {
                self.apo_triggered = true;
            } else if self.inactivity_secs >= limit_secs.saturating_sub(10) {
                self.apo_warning_active = true;
            }
        }
    }

    /// Trigger Cortex-M0+ Wait-For-Interrupt (WFI) standby idle.
    pub fn enter_standby(&self) {
        #[cfg(target_arch = "arm")]
        asm::wfi();
    }

    /// Transition to Deep Sleep power saving state.
    pub fn enter_deep_sleep(&mut self) {
        self.profile = PowerProfile::DeepSleep;
        #[cfg(target_arch = "arm")]
        asm::wfi();
    }
}

impl kernel::service::PowerService for SystemPower {
    fn battery_percentage(&self) -> u8 {
        self.battery_pct
    }
    fn battery_voltage_mv(&self) -> u16 {
        self.battery_mv
    }
    fn battery_current_ma(&self) -> u16 {
        self.battery_ma
    }
    fn uptime_secs(&self) -> u32 {
        self.uptime_secs
    }
    fn raw_battery_adc(&self) -> u16 {
        self.battery_driver.last_raw_adc
    }
    fn mcu_temperature_deci_c(&self) -> i16 {
        self.mcu_temp_deci_c
    }
    fn vdda_voltage_mv(&self) -> u16 {
        self.vdda_mv
    }
    fn is_charging(&self) -> bool {
        self.is_charging
    }
    fn is_low(&self) -> bool {
        self.is_low
    }
    fn is_critical(&self) -> bool {
        self.is_critical
    }
    fn poll_charging_transition(&mut self) -> Option<bool> {
        self.poll_charging_transition()
    }
    fn set_battery_type(&mut self, bat_type: u8) {
        self.battery_driver
            .set_battery_type(BatteryType::from_u8(bat_type));
    }
    fn force_refresh(&mut self) {
        Self::force_refresh(self);
    }
}
