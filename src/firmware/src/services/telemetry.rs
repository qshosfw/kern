//! System Telemetry and Periodic ADC Battery / Temperature Sampling.

use crate::POWER_SERVICE;
use py32_hal::adc::Adc;

pub fn sample_battery_and_system(adc: &Adc, audio_active: bool) {
    let raw_adc = adc.read_channel_averaged(8, 8);
    let vdda_mv = adc.read_vref_mv();
    let mcu_temp = adc.read_mcu_temp_deci_c(vdda_mv);

    let radio_state = crate::radio_state_ref();
    let _master_cfg = crate::master_config_ref();
    let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };

    let active_vfo = radio_state.selected_vfo();
    let periph = drivers::battery::PeripheralState {
        backlight_level: drivers::backlight::BACKLIGHT_DUTY
            .load(core::sync::atomic::Ordering::Relaxed),
        tx_active: active_vfo.tx_active,
        tx_power_level: match active_vfo.power.as_bytes().first() {
            Some(b'L') => 0,
            Some(b'M') => 1,
            _ => 2,
        },
        rx_active: active_vfo.rx_active,
        audio_active,
        keypad_led: false,
    };

    power_service.update_adc(raw_adc, vdda_mv, mcu_temp, &periph);
}
