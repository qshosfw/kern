pub use kernel::service::PowerService;

pub trait PowerServiceConfigExt {
    fn load_battery_config(&mut self, cfg: &crate::config::BatteryConfig);
}

impl PowerServiceConfigExt for drivers::battery::SystemPower {
    fn load_battery_config(&mut self, cfg: &crate::config::BatteryConfig) {
        self.set_battery_type(cfg.battery_type);
        self.battery_driver.set_calib_tweak(cfg.calib_tweak_mv);
        self.battery_driver.charge_detect_enabled = cfg.charging_detect != 0;
        self.profile = match cfg.power_profile {
            0 => drivers::battery::PowerProfile::PowerSave,
            2 => drivers::battery::PowerProfile::Performance,
            _ => drivers::battery::PowerProfile::Balanced,
        };
        self.auto_power_off_min = cfg.auto_power_off_min;
        self.force_refresh();
    }
}
