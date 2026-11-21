//! I/O & Hardware Peripheral System Call Dispatcher (`Op::Io*`, domain `0x8`).

#[cfg(test)]
use super::active_ctx_opt;
use super::{ptr_slice, ptr_slice_mut, slice_or_empty};
use kernel::proc::abi::KrxBusReq;

#[inline(always)]
fn power_ref() -> &'static drivers::battery::SystemPower {
    unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) }
}

fn dispatch_io_key(sub: u8, a1: usize) -> usize {
    use ::abi::sub::io_key;
    // Headless: apps see an idle matrix; raw keypad probes cannot bypass the wire-only lock.
    if matches!(sub, io_key::POLL | io_key::MATRIX | io_key::IS_PRESSED)
        && !crate::system::admits_physical_input()
    {
        return 0;
    }
    match sub {
        io_key::POLL => {
            bsp::Board::wdt_feed();
            drivers::keyboard::KeypadScanner::poll_matrix()
                .map(|k| k as usize)
                .unwrap_or(0)
        }
        io_key::MATRIX => drivers::keyboard::KeypadScanner::poll_raw_mask() as usize,
        io_key::IS_PRESSED => {
            let mask = drivers::keyboard::KeypadScanner::poll_raw_mask();
            kernel::event::KeyCode::from_u8(a1 as u8)
                .is_some_and(|kc| (mask & drivers::keyboard::key_to_bit(kc)) != 0)
                as usize
        }
        io_key::IS_F => {
            #[cfg(not(test))]
            {
                crate::kern_hw().orchestrator.is_f_active() as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| ctx.is_f_active as usize)
            }
        }
        io_key::IS_LOCKED => {
            #[cfg(not(test))]
            {
                crate::kern_hw().orchestrator.is_keypad_locked() as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| ctx.is_keypad_locked as usize)
            }
        }
        _ => 0,
    }
}

fn dispatch_io_power(sub: u8, a1: usize) -> usize {
    use ::abi::sub::io_power;
    use kernel::service::PowerService;
    let p = power_ref();
    match sub {
        io_power::BATT_MV => p.battery_voltage_mv() as usize,
        io_power::BATT_PCT => p.battery_percentage() as usize,
        io_power::BATT_MA => p.battery_current_ma() as usize,
        io_power::BATT_TYPE => p.battery_type() as usize,
        io_power::BATT_ADC => p.raw_battery_adc() as usize,
        io_power::MCU_TEMP => p.mcu_temperature_deci_c() as u16 as usize,
        io_power::CHARGING => usize::from(p.is_charging),
        io_power::LOW => usize::from(p.is_low()),
        io_power::CRITICAL => usize::from(p.is_critical()),
        io_power::BL_WAKE => {
            #[cfg(not(test))]
            crate::kern_hw().backlight.wake();
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                if let Some(ref mut d) = ctx.display {
                    kernel::service::DisplayService::wake(*d);
                }
            }
            0
        }
        io_power::FLASHLIGHT => {
            if a1 != 0 {
                bsp::pins::FLASHLIGHT.set_high();
            } else {
                bsp::pins::FLASHLIGHT.set_low();
            }
            0
        }
        _ => 0,
    }
}

unsafe fn dispatch_io_serial(sub: u8, a1: usize, a2: usize) -> usize {
    use ::abi::sub::io_serial;
    match sub {
        io_serial::WRITE => {
            let len = a2 as u16;
            let Some(slice) = (unsafe { ptr_slice(a1 as *const u8, len as usize) }) else {
                return 0;
            };
            #[cfg(not(test))]
            {
                #[cfg(feature = "usb")]
                if crate::kern_hw().usb.is_connected() {
                    let _ = crate::kern_hw().usb.cdc_write(slice);
                    return len as usize;
                }
                #[cfg(feature = "uart")]
                {
                    let _ = py32_hal::uart::write_bytes(slice);
                    len as usize
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
                            return len as usize;
                        }
                    }
                    #[cfg(feature = "uart")]
                    ctx.uart.write(slice);
                    len as usize
                } else {
                    0
                }
            }
        }
        io_serial::READ => {
            let Some(slice) = (unsafe { ptr_slice_mut(a1 as *mut u8, (a2 as u16) as usize) })
            else {
                return 0;
            };
            #[cfg(not(test))]
            {
                #[cfg(feature = "usb")]
                if crate::kern_hw().usb.is_connected() {
                    return crate::kern_hw().usb.cdc_read(slice);
                }
                #[cfg(feature = "uart")]
                {
                    crate::kern_hw().uart.read(slice)
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
                            return usb.cdc_read(slice);
                        }
                    }
                    #[cfg(feature = "uart")]
                    {
                        return ctx.uart.read(slice);
                    }
                    #[cfg(not(feature = "uart"))]
                    0
                } else {
                    0
                }
            }
        }
        io_serial::CONNECTED => {
            #[cfg(all(not(test), feature = "usb"))]
            {
                crate::kern_hw().usb.is_connected() as usize
            }
            #[cfg(all(test, feature = "usb"))]
            {
                active_ctx_opt().map_or(0, |ctx| {
                    ctx.usb.as_ref().is_some_and(|u| u.is_connected()) as usize
                })
            }
            #[cfg(not(feature = "usb"))]
            {
                0
            }
        }
        io_serial::FLUSH => {
            py32_hal::uart::wait_tx_complete();
            0
        }
        _ => 0,
    }
}

unsafe fn dispatch_io_bus(sub: u8, a1: usize, a2: usize) -> usize {
    use ::abi::sub::io_bus;
    match sub {
        io_bus::I2C => {
            let req = a1 as *const KrxBusReq;
            if req.is_null() {
                return 1;
            }
            let r = unsafe { &*req };
            let mut i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let tx_opt = unsafe { ptr_slice(r.tx, r.tx_len as usize) };
            let rx_opt = unsafe { ptr_slice_mut(r.rx, r.rx_len as usize) };
            usize::from(
                i2c.xfer_reg(r.addr as u8, (r.addr >> 8) as u8, tx_opt, rx_opt)
                    .is_err(),
            )
        }
        io_bus::SPI => {
            let req = a1 as *const KrxBusReq;
            if req.is_null() {
                return 1;
            }
            let r = unsafe { &*req };
            let len = r.rx_len.max(r.tx_len) as usize;
            if len == 0 {
                return 1;
            }
            let tx_slice = unsafe { slice_or_empty(r.tx, len) };
            let rx_slice = unsafe { ptr_slice_mut(r.rx, len) }.unwrap_or(&mut []);
            usize::from(
                crate::services::primitives::hw_spi_transfer(r.addr as u8, tx_slice, rx_slice)
                    .is_err(),
            )
        }
        io_bus::GPIO_SET => {
            crate::services::primitives::hw_gpio_write(a1 as u8, a2 != 0);
            0
        }
        io_bus::GPIO_GET => crate::services::primitives::hw_gpio_read(a1 as u8) as usize,
        io_bus::RF_RD => {
            #[cfg(not(test))]
            {
                crate::kern_hw().radio.read_reg(a1 as u8) as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| ctx.radio.read_reg(a1 as u8) as usize)
            }
        }
        io_bus::RF_WR => {
            #[cfg(not(test))]
            crate::kern_hw().radio.write_reg(a1 as u8, a2 as u16);
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.radio.write_reg(a1 as u8, a2 as u16);
            }
            0
        }
        io_bus::FLASH_RD => {
            let req = a1 as *const KrxBusReq;
            if !req.is_null() {
                let r = unsafe { &*req };
                if let Some(dst) = unsafe { ptr_slice_mut(r.rx, r.rx_len as usize) } {
                    #[cfg(not(test))]
                    {
                        return usize::from(
                            crate::kern_fs().fs.dev.read_data(r.addr, dst).is_err(),
                        );
                    }
                    #[cfg(test)]
                    if let Some(ctx) = active_ctx_opt() {
                        return usize::from(ctx.storage.fs.dev.read_data(r.addr, dst).is_err());
                    }
                }
            }
            1
        }
        _ => 0,
    }
}

#[inline(never)]
pub unsafe fn dispatch_io(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::{idx as op, sub::io_time};
    match idx {
        op::IoKey => dispatch_io_key(a0 as u8, a1),
        op::IoPower => dispatch_io_power(a0 as u8, a1),
        op::IoSerial => unsafe { dispatch_io_serial(a0 as u8, a1, a2) },
        op::IoTime => match a0 as u8 {
            io_time::TICKS_MS => py32_hal::delay::millis() as usize,
            io_time::DELAY_MS => {
                let mut rem = a1 as u32;
                while rem > 50 {
                    bsp::Board::wdt_feed();
                    py32_hal::delay::delay_ms(50);
                    rem -= 50;
                }
                if rem > 0 {
                    bsp::Board::wdt_feed();
                    py32_hal::delay::delay_ms(rem);
                }
                0
            }
            io_time::DELAY_US => {
                py32_hal::delay::delay_us(a1 as u32);
                0
            }
            io_time::RTC_GET => {
                let out = a1 as *mut u8;
                if !out.is_null() {
                    let dst = unsafe { core::slice::from_raw_parts_mut(out, 7) };
                    dst.copy_from_slice(&[0xEA, 0x07, 1, 1, 0, 0, 0]);
                    0
                } else {
                    1
                }
            }
            _ => 0,
        },
        op::IoBus => unsafe { dispatch_io_bus(a0 as u8, a1, a2) },
        _ => 0,
    }
}
