//! System Telemetry and Periodic ADC Battery / Temperature Sampling.

use py32_hal::adc::Adc;

pub fn sample_battery_and_system(adc: &Adc, audio_active: bool) {
    let (raw_adc, vdda_mv, mcu_temp) = adc.sample_injected_telemetry();

    let coord = &crate::radio_state_ref().coord;
    let power_service = crate::power_service();

    let slot = coord.active_vfo;
    let ch = coord.active_channel();
    let periph = drivers::battery::PeripheralState {
        backlight_level: drivers::backlight::BACKLIGHT_DUTY
            .load(core::sync::atomic::Ordering::Relaxed),
        tx_active: coord.is_slot_tx_active(slot),
        tx_power_level: ch.power_level() as u8,
        rx_active: coord.is_slot_rx_active(slot),
        audio_active,
        keypad_led: false,
    };

    power_service.update_adc(raw_adc, vdda_mv, mcu_temp, &periph);
}
