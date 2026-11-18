//! I/O & Hardware Peripheral System Call Handlers.
//!
//! Exposes Keypad (`KeypadApi`), Power/PMU (`PowerApi`),
//! Serial CDC (`SerialApi`), Hardware Timers/RTC (`TimerApi`),
//! and Raw Hardware Buses (`HwBusApi`).

#[cfg(test)]
use super::active_ctx_opt;
use kernel::proc::abi::{HwBusApi, KeypadApi, PowerApi, SerialApi, TimerApi};

// ── 5. CapKeypad C-ABI Handlers ─────────────────────────────────────────────

pub unsafe extern "C" fn api_sys_poll_key() -> u8 {
    bsp::Board::wdt_feed();
    drivers::keyboard::KeypadScanner::poll_matrix()
        .map(|k| k as u8)
        .unwrap_or(0)
}

pub unsafe extern "C" fn api_sys_nav_dir(key: u8) -> i8 {
    match key {
        1 => -1, // Up
        2 => 1,  // Down
        _ => 0,
    }
}

pub unsafe extern "C" fn api_sys_key_matrix() -> u32 {
    drivers::keyboard::KeypadScanner::poll_raw_mask()
}

pub unsafe extern "C" fn api_sys_is_key_pressed(key_code: u8) -> u8 {
    let mask = drivers::keyboard::KeypadScanner::poll_raw_mask();
    if let Some(kc) = kernel::event::KeyCode::from_u8(key_code) {
        if (mask & drivers::keyboard::key_to_bit(kc)) != 0 {
            return 1;
        }
    }
    0
}

pub unsafe extern "C" fn api_sys_is_f_active() -> u8 {
    #[cfg(not(test))]
    {
        crate::kern_hw().orchestrator.is_f_active() as u8
    }
    #[cfg(test)]
    {
        active_ctx_opt().map_or(0, |ctx| ctx.is_f_active as u8)
    }
}

pub unsafe extern "C" fn api_sys_is_ptt_pressed() -> u8 {
    unsafe { api_sys_is_key_pressed(kernel::event::KeyCode::Ptt as u8) }
}

pub unsafe extern "C" fn api_sys_is_side1_pressed() -> u8 {
    unsafe { api_sys_is_key_pressed(kernel::event::KeyCode::Side1 as u8) }
}

pub unsafe extern "C" fn api_sys_is_side2_pressed() -> u8 {
    unsafe { api_sys_is_key_pressed(kernel::event::KeyCode::Side2 as u8) }
}

pub unsafe extern "C" fn api_sys_clear_key_buffer() {
    drivers::keyboard::KeypadScanner::reset_gpio_pins();
}

pub unsafe extern "C" fn api_sys_is_keypad_locked() -> u8 {
    #[cfg(not(test))]
    {
        crate::kern_hw().orchestrator.is_keypad_locked() as u8
    }
    #[cfg(test)]
    {
        active_ctx_opt().map_or(0, |ctx| ctx.is_keypad_locked as u8)
    }
}

pub static KEYPAD_VTABLE: KeypadApi = KeypadApi {
    sys_poll_key: api_sys_poll_key,
    sys_nav_dir: api_sys_nav_dir,
    sys_key_matrix: api_sys_key_matrix,
    sys_is_key_pressed: api_sys_is_key_pressed,
    sys_is_f_active: api_sys_is_f_active,
    sys_is_ptt_pressed: api_sys_is_ptt_pressed,
    sys_is_side1_pressed: api_sys_is_side1_pressed,
    sys_is_side2_pressed: api_sys_is_side2_pressed,
    sys_clear_key_buffer: api_sys_clear_key_buffer,
    sys_is_keypad_locked: api_sys_is_keypad_locked,
};

// ── 6. CapPower C-ABI Handlers ──────────────────────────────────────────────

#[inline(always)]
fn power_ref() -> &'static drivers::battery::SystemPower {
    unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) }
}

#[inline(never)]
fn pwr_u16(op: u8) -> u16 {
    use kernel::service::PowerService;
    let p = power_ref();
    match op {
        0 => p.battery_voltage_mv(),
        1 => p.battery_percentage() as u16,
        2 => p.battery_current_ma(),
        3 => p.battery_type() as u16,
        4 => p.raw_battery_adc(),
        5 => p.mcu_temperature_deci_c() as u16,
        6 => u16::from(p.is_charging),
        7 => u16::from(p.is_low()),
        _ => u16::from(p.is_critical()),
    }
}

pub unsafe extern "C" fn api_sys_battery_mv() -> u16 {
    pwr_u16(0)
}

pub unsafe extern "C" fn api_sys_battery_pct() -> u8 {
    pwr_u16(1) as u8
}

pub unsafe extern "C" fn api_sys_battery_ma() -> u16 {
    pwr_u16(2)
}

pub unsafe extern "C" fn api_sys_battery_type() -> u8 {
    pwr_u16(3) as u8
}

pub unsafe extern "C" fn api_sys_raw_battery_adc() -> u16 {
    pwr_u16(4)
}

pub unsafe extern "C" fn api_sys_mcu_temp() -> i16 {
    pwr_u16(5) as i16
}

pub unsafe extern "C" fn api_sys_is_charging() -> u8 {
    pwr_u16(6) as u8
}

pub unsafe extern "C" fn api_sys_is_low() -> u8 {
    pwr_u16(7) as u8
}

pub unsafe extern "C" fn api_sys_is_critical() -> u8 {
    pwr_u16(8) as u8
}

pub unsafe extern "C" fn api_sys_set_backlight(mode: u8, timeout_s: u8) {
    #[cfg(not(test))]
    {
        let bmode = match mode {
            0 => drivers::backlight::BacklightMode::Off,
            1 => drivers::backlight::BacklightMode::Auto(((timeout_s as u16) * 1000).min(60000)),
            _ => drivers::backlight::BacklightMode::On,
        };
        crate::kern_hw().backlight.set_mode(bmode);
    }
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        if let Some(ref mut d) = ctx.display {
            let bmode = match mode {
                0 => kernel::service::BacklightMode::Off,
                1 => kernel::service::BacklightMode::Auto(((timeout_s as u16) * 1000).min(60000)),
                _ => kernel::service::BacklightMode::On,
            };
            kernel::service::DisplayService::set_backlight_mode(*d, bmode);
        }
    }
}

pub unsafe extern "C" fn api_sys_set_backlight_brightness(max: u8, min: u8) {
    #[cfg(not(test))]
    crate::kern_hw().backlight.set_levels(max, min);
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        if let Some(ref mut d) = ctx.display {
            kernel::service::DisplayService::set_backlight_brightness(*d, max, min);
        }
    }
}

pub unsafe extern "C" fn api_sys_wake_backlight() {
    #[cfg(not(test))]
    crate::kern_hw().backlight.wake();
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        if let Some(ref mut d) = ctx.display {
            kernel::service::DisplayService::wake(*d);
        }
    }
}

pub unsafe extern "C" fn api_sys_set_flashlight(on: u8) {
    if on != 0 {
        bsp::pins::FLASHLIGHT.set_high();
    } else {
        bsp::pins::FLASHLIGHT.set_low();
    }
}

pub unsafe extern "C" fn api_sys_cpu_freq_hz() -> u32 {
    py32_hal::rcc::sysclk_hz()
}

pub unsafe extern "C" fn api_sys_reboot(mode: u8) {
    if !super::super::native_runner::active_native_caps().contains(kernel::caps::Cap::PowerManage) {
        return;
    }
    let reboot_mode = match mode {
        1 => kernel::power::RebootMode::Bootloader,
        2 => kernel::power::RebootMode::SafeRecovery,
        _ => kernel::power::RebootMode::Firmware,
    };
    kernel::power::PowerControl::reboot_with(reboot_mode);
}

pub unsafe extern "C" fn api_sys_flush_config() -> u8 {
    #[cfg(not(test))]
    {
        u8::from(
            crate::master_config()
                .flush_dirty(crate::kern_fs())
                .is_err(),
        )
    }
    #[cfg(test)]
    {
        active_ctx_opt().map_or(0, |ctx| u8::from(ctx.flush_config().is_err()))
    }
}

pub static POWER_VTABLE: PowerApi = PowerApi {
    sys_battery_mv: api_sys_battery_mv,
    sys_battery_pct: api_sys_battery_pct,
    sys_battery_ma: api_sys_battery_ma,
    sys_battery_type: api_sys_battery_type,
    sys_raw_battery_adc: api_sys_raw_battery_adc,
    sys_mcu_temp: api_sys_mcu_temp,
    sys_is_charging: api_sys_is_charging,
    sys_is_low: api_sys_is_low,
    sys_is_critical: api_sys_is_critical,
    sys_set_backlight: api_sys_set_backlight,
    sys_set_backlight_brightness: api_sys_set_backlight_brightness,
    sys_wake_backlight: api_sys_wake_backlight,
    sys_set_flashlight: api_sys_set_flashlight,
    sys_cpu_freq_hz: api_sys_cpu_freq_hz,
    sys_reboot: api_sys_reboot,
    sys_flush_config: api_sys_flush_config,
};

// ── 7. CapSerial C-ABI Handlers ─────────────────────────────────────────────

pub unsafe extern "C" fn api_serial_write(data: *const u8, len: u16) -> u16 {
    let Some(slice) = (unsafe { super::ptr_slice(data, len as usize) }) else {
        return 0;
    };
    #[cfg(not(test))]
    {
        #[cfg(feature = "usb")]
        if crate::kern_hw().usb.is_connected() {
            let _ = crate::kern_hw().usb.cdc_write(slice);
            return len;
        }
        #[cfg(feature = "uart")]
        {
            let _ = py32_hal::uart::write_bytes(slice);
            len
        }
        #[cfg(not(feature = "uart"))]
        0
    }
    #[cfg(test)]
    {
        if let Some(ctx) = active_ctx_opt() {
            #[cfg(feature = "usb")]
            if let Some(ref mut usb) = ctx.usb {
                if usb.is_connected() {
                    let _ = usb.cdc_write(slice);
                    return len;
                }
            }
            #[cfg(feature = "uart")]
            ctx.uart.write(slice);
            len
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_serial_read(buf: *mut u8, max_len: u16) -> u16 {
    let Some(slice) = (unsafe { super::ptr_slice_mut(buf, max_len as usize) }) else {
        return 0;
    };
    #[cfg(not(test))]
    {
        #[cfg(feature = "usb")]
        if crate::kern_hw().usb.is_connected() {
            return crate::kern_hw().usb.cdc_read(slice) as u16;
        }
        #[cfg(feature = "uart")]
        {
            crate::kern_hw().uart.read(slice) as u16
        }
        #[cfg(not(feature = "uart"))]
        0
    }
    #[cfg(test)]
    {
        if let Some(ctx) = active_ctx_opt() {
            #[cfg(feature = "usb")]
            if let Some(ref mut usb) = ctx.usb {
                if usb.is_connected() {
                    return usb.cdc_read(slice) as u16;
                }
            }
            #[cfg(feature = "uart")]
            {
                return ctx.uart.read(slice) as u16;
            }
            #[cfg(not(feature = "uart"))]
            0
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_serial_is_connected() -> u8 {
    #[cfg(all(not(test), feature = "usb"))]
    {
        crate::kern_hw().usb.is_connected() as u8
    }
    #[cfg(all(test, feature = "usb"))]
    {
        active_ctx_opt().map_or(0, |ctx| {
            ctx.usb.as_ref().is_some_and(|u| u.is_connected()) as u8
        })
    }
    #[cfg(not(feature = "usb"))]
    {
        0
    }
}

pub unsafe extern "C" fn api_serial_flush() {
    py32_hal::uart::wait_tx_complete();
}

pub static SERIAL_VTABLE: SerialApi = SerialApi {
    serial_write: api_serial_write,
    serial_read: api_serial_read,
    serial_is_connected: api_serial_is_connected,
    serial_flush: api_serial_flush,
};

// ── 8. CapTimer C-ABI Handlers ──────────────────────────────────────────────

pub unsafe extern "C" fn api_sys_ticks_ms() -> u32 {
    py32_hal::delay::millis()
}

pub unsafe extern "C" fn api_sys_uptime_secs() -> u32 {
    utils::math::div1000_u32(py32_hal::delay::millis())
}

pub unsafe extern "C" fn api_sys_delay_ms(ms: u32) {
    let mut rem = ms;
    while rem > 50 {
        bsp::Board::wdt_feed();
        py32_hal::delay::delay_ms(50);
        rem -= 50;
    }
    if rem > 0 {
        bsp::Board::wdt_feed();
        py32_hal::delay::delay_ms(rem);
    }
}

pub unsafe extern "C" fn api_sys_delay_us(us: u32) {
    py32_hal::delay::delay_us(us);
}

pub unsafe extern "C" fn api_sys_rtc_get(out_time: *mut u8) -> u8 {
    if !out_time.is_null() {
        let dst = unsafe { core::slice::from_raw_parts_mut(out_time, 7) };
        dst.copy_from_slice(&[0xEA, 0x07, 1, 1, 0, 0, 0]);
        0
    } else {
        1
    }
}

pub static TIMER_VTABLE: TimerApi = TimerApi {
    sys_ticks_ms: api_sys_ticks_ms,
    sys_uptime_secs: api_sys_uptime_secs,
    sys_delay_ms: api_sys_delay_ms,
    sys_delay_us: api_sys_delay_us,
    sys_rtc_get: api_sys_rtc_get,
};

// ── 13. CapHwBus C-ABI Handlers ─────────────────────────────────────────────

pub unsafe extern "C" fn api_hw_i2c_transfer(
    addr: u8,
    reg: u8,
    tx: *const u8,
    tx_len: u16,
    rx: *mut u8,
    rx_len: u16,
) -> u8 {
    let mut i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
    let tx_opt = unsafe { super::ptr_slice(tx, tx_len as usize) };
    let rx_opt = unsafe { super::ptr_slice_mut(rx, rx_len as usize) };
    u8::from(i2c.xfer_reg(addr, reg, tx_opt, rx_opt).is_err())
}

pub unsafe extern "C" fn api_hw_spi_transfer(
    cs_pin: u8,
    tx: *const u8,
    rx: *mut u8,
    len: u16,
) -> u8 {
    if len == 0 {
        return 1;
    }
    let tx_slice = unsafe { super::slice_or_empty(tx, len as usize) };
    let rx_slice = unsafe { super::ptr_slice_mut(rx, len as usize) }.unwrap_or(&mut []);
    u8::from(crate::services::primitives::hw_spi_transfer(cs_pin, tx_slice, rx_slice).is_err())
}

pub unsafe extern "C" fn api_hw_gpio_set(pin: u8, high: u8) {
    crate::services::primitives::hw_gpio_write(pin, high != 0);
}

pub unsafe extern "C" fn api_hw_gpio_get(pin: u8) -> u8 {
    crate::services::primitives::hw_gpio_read(pin) as u8
}

pub unsafe extern "C" fn api_hw_rf_read_reg(reg: u8) -> u16 {
    #[cfg(not(test))]
    {
        crate::kern_hw().radio.read_reg(reg)
    }
    #[cfg(test)]
    {
        active_ctx_opt().map_or(0, |ctx| ctx.radio.read_reg(reg))
    }
}

pub unsafe extern "C" fn api_hw_rf_write_reg(reg: u8, val: u16) {
    #[cfg(not(test))]
    crate::kern_hw().radio.write_reg(reg, val);
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        ctx.radio.write_reg(reg, val);
    }
}

pub unsafe extern "C" fn api_hw_flash_read(addr: u32, buf: *mut u8, len: u16) -> u8 {
    if let Some(dst) = unsafe { super::ptr_slice_mut(buf, len as usize) } {
        #[cfg(not(test))]
        {
            return u8::from(crate::kern_fs().fs.dev.read_data(addr, dst).is_err());
        }
        #[cfg(test)]
        if let Some(ctx) = active_ctx_opt() {
            return u8::from(ctx.storage.fs.dev.read_data(addr, dst).is_err());
        }
    }
    1
}

pub static HW_BUS_VTABLE: HwBusApi = HwBusApi {
    hw_i2c_transfer: api_hw_i2c_transfer,
    hw_spi_transfer: api_hw_spi_transfer,
    hw_gpio_set: api_hw_gpio_set,
    hw_gpio_get: api_hw_gpio_get,
    hw_rf_read_reg: api_hw_rf_read_reg,
    hw_rf_write_reg: api_hw_rf_write_reg,
    hw_flash_read: api_hw_flash_read,
};
