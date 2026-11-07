//! I/O & Hardware Peripheral System Call Handlers.
//!
//! Exposes Keypad (`CapKeypadVTable`), Power/PMU (`CapPowerVTable`),
//! Serial CDC (`CapSerialVTable`), Hardware Timers/RTC (`CapTimerVTable`),
//! and Raw Hardware Buses (`CapHwBusVTable`).

use super::active_ctx_opt;
use kernel::proc::abi::{
    CapHwBusVTable, CapKeypadVTable, CapPowerVTable, CapSerialVTable, CapTimerVTable,
};
use kernel::proc::primitives::*;

// ── 5. CapKeypad C-ABI Handlers ─────────────────────────────────────────────

pub unsafe extern "C" fn api_sys_poll_key() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.poll_key().unwrap_or(0)
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_nav_dir(key: u8) -> i8 {
    match key {
        1 => -1, // Up
        2 => 1,  // Down
        _ => 0,
    }
}

pub unsafe extern "C" fn api_sys_key_matrix() -> u32 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.get_key_matrix()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_is_key_pressed(key_code: u8) -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_key_pressed(key_code) { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_is_f_active() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_f_active() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_is_ptt_pressed() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_ptt_pressed() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_is_side1_pressed() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_side1_pressed() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_is_side2_pressed() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_side2_pressed() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_clear_key_buffer() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.clear_key_buffer();
    }
}

pub unsafe extern "C" fn api_sys_is_keypad_locked() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_keypad_locked() { 1 } else { 0 }
    } else {
        0
    }
}

pub static KEYPAD_VTABLE: CapKeypadVTable = CapKeypadVTable {
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

pub unsafe extern "C" fn api_sys_battery_mv() -> u16 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.battery_mv()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_battery_pct() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.battery_pct()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_battery_ma() -> u16 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.battery_ma()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_battery_type() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.battery_type()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_raw_battery_adc() -> u16 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.raw_battery_adc()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_mcu_temp() -> i16 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.mcu_temp_deci_c()
    } else {
        250
    }
}

pub unsafe extern "C" fn api_sys_is_charging() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_charging() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_is_low() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_low() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_is_critical() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_critical() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_sys_set_backlight(mode: u8, timeout_s: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_backlight(mode, timeout_s);
    }
}

pub unsafe extern "C" fn api_sys_set_backlight_brightness(max: u8, min: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_backlight_brightness(max, min);
    }
}

pub unsafe extern "C" fn api_sys_wake_backlight() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.wake_backlight();
    }
}

pub unsafe extern "C" fn api_sys_set_flashlight(on: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_flashlight(on != 0);
    }
}

pub unsafe extern "C" fn api_sys_cpu_freq_hz() -> u32 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.cpu_freq_hz()
    } else {
        48_000_000
    }
}

pub static POWER_VTABLE: CapPowerVTable = CapPowerVTable {
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
};

// ── 7. CapSerial C-ABI Handlers ─────────────────────────────────────────────

pub unsafe extern "C" fn api_serial_write(data: *const u8, len: u16) -> u16 {
    if !data.is_null() && len > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
            ctx.write(slice);
            len
        } else {
            0
        }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_serial_read(buf: *mut u8, max_len: u16) -> u16 {
    if !buf.is_null() && max_len > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let slice = unsafe { core::slice::from_raw_parts_mut(buf, max_len as usize) };
            ctx.read(slice) as u16
        } else {
            0
        }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_serial_is_connected() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_usb_active() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_serial_flush() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.flush();
    }
}

pub static SERIAL_VTABLE: CapSerialVTable = CapSerialVTable {
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
    py32_hal::delay::millis() / 1000
}

pub unsafe extern "C" fn api_sys_delay_ms(ms: u32) {
    py32_hal::delay::delay_ms(ms);
}

pub unsafe extern "C" fn api_sys_delay_us(us: u32) {
    py32_hal::delay::delay_us(us);
}

pub unsafe extern "C" fn api_sys_rtc_get(out_time: *mut u8) -> u8 {
    if !out_time.is_null() {
        if let Some(ctx) = active_ctx_opt() {
            let (y, mo, d, h, mi, s) = ctx.get_rtc_time();
            let dst = unsafe { core::slice::from_raw_parts_mut(out_time, 7) };
            dst[0..2].copy_from_slice(&y.to_le_bytes());
            dst[2] = mo;
            dst[3] = d;
            dst[4] = h;
            dst[5] = mi;
            dst[6] = s;
            0
        } else {
            1
        }
    } else {
        1
    }
}

pub static TIMER_VTABLE: CapTimerVTable = CapTimerVTable {
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
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    if !tx.is_null() && tx_len > 0 {
        let tx_slice = unsafe { core::slice::from_raw_parts(tx, tx_len as usize) };
        if ctx.i2c_write(addr, reg, tx_slice).is_err() {
            return 1;
        }
    }
    if !rx.is_null() && rx_len > 0 {
        let rx_slice = unsafe { core::slice::from_raw_parts_mut(rx, rx_len as usize) };
        if ctx.i2c_read(addr, reg, rx_slice).is_err() {
            return 1;
        }
    }
    0
}

pub unsafe extern "C" fn api_hw_spi_transfer(
    cs_pin: u8,
    tx: *const u8,
    rx: *mut u8,
    len: u16,
) -> u8 {
    let Some(ctx) = active_ctx_opt() else {
        return 1;
    };
    if len > 0 {
        let tx_slice = if tx.is_null() {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(tx, len as usize) }
        };
        let rx_slice = if rx.is_null() {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(rx, len as usize) }
        };
        if ctx.spi_transfer(cs_pin, tx_slice, rx_slice).is_ok() {
            0
        } else {
            1
        }
    } else {
        1
    }
}

pub unsafe extern "C" fn api_hw_gpio_set(pin: u8, high: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.gpio_write(pin, high != 0);
    }
}

pub unsafe extern "C" fn api_hw_gpio_get(pin: u8) -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.gpio_read(pin) { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_hw_rf_read_reg(reg: u8) -> u16 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.read_register(reg)
    } else {
        0
    }
}

pub unsafe extern "C" fn api_hw_rf_write_reg(reg: u8, val: u16) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.write_register(reg, val);
    }
}

pub unsafe extern "C" fn api_hw_flash_read(addr: u32, buf: *mut u8, len: u16) -> u8 {
    if !buf.is_null() && len > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let dst = unsafe { core::slice::from_raw_parts_mut(buf, len as usize) };
            if ctx.raw_flash_read(addr, dst).is_ok() {
                0
            } else {
                1
            }
        } else {
            1
        }
    } else {
        1
    }
}

pub static HW_BUS_VTABLE: CapHwBusVTable = CapHwBusVTable {
    hw_i2c_transfer: api_hw_i2c_transfer,
    hw_spi_transfer: api_hw_spi_transfer,
    hw_gpio_set: api_hw_gpio_set,
    hw_gpio_get: api_hw_gpio_get,
    hw_rf_read_reg: api_hw_rf_read_reg,
    hw_rf_write_reg: api_hw_rf_write_reg,
    hw_flash_read: api_hw_flash_read,
};
