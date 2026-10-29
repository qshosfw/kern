//! System Power Management, Power Profiles, and PowerService Implementation.

use super::curve::BatteryType;
use super::{BatteryDriver, BatteryState};

#[cfg(target_arch = "arm")]
use cortex_m::asm;

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

    pub fn battery_type(&self) -> BatteryType {
        self.battery_driver.battery_type()
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
    fn battery_type(&self) -> u8 {
        self.battery_driver.battery_type() as u8
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
