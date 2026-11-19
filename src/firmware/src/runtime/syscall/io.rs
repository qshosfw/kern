//! I/O & Hardware Peripheral System Call Dispatcher (`Op::Io*`, domain `0x8`).

#[cfg(test)]
use super::active_ctx_opt;
use super::{ptr_slice, ptr_slice_mut, slice_or_empty};
use kernel::proc::abi::KrxBusReq;

#[inline(always)]
fn power_ref() -> &'static drivers::battery::SystemPower {
    unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) }
}

#[inline(never)]
fn dispatch_io_key(sub: u8, a1: usize) -> usize {
    match sub {
        0 => {
            bsp::Board::wdt_feed();
            drivers::keyboard::KeypadScanner::poll_matrix()
                .map(|k| k as usize)
                .unwrap_or(0)
        }
        1 => match a1 as u8 {
            1 => (-1i8 as u8) as usize,
            2 => 1,
            _ => 0,
        },
        2 => drivers::keyboard::KeypadScanner::poll_raw_mask() as usize,
        3 => {
            let mask = drivers::keyboard::KeypadScanner::poll_raw_mask();
            kernel::event::KeyCode::from_u8(a1 as u8)
                .is_some_and(|kc| (mask & drivers::keyboard::key_to_bit(kc)) != 0)
                as usize
        }
        4 => {
            #[cfg(not(test))]
            {
                crate::kern_hw().orchestrator.is_f_active() as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| ctx.is_f_active as usize)
            }
        }
        5 => {
            drivers::keyboard::KeypadScanner::reset_gpio_pins();
            0
        }
        6 => {
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

#[inline(never)]
fn dispatch_io_power(sub: u8, a1: usize, a2: usize) -> usize {
    use kernel::service::PowerService;
    let p = power_ref();
    match sub {
        0 => p.battery_voltage_mv() as usize,
        1 => p.battery_percentage() as usize,
        2 => p.battery_current_ma() as usize,
        3 => p.battery_type() as usize,
        4 => p.raw_battery_adc() as usize,
        5 => p.mcu_temperature_deci_c() as u16 as usize,
        6 => usize::from(p.is_charging),
        7 => usize::from(p.is_low()),
        8 => usize::from(p.is_critical()),
        9 => {
            let mode = a1 as u8;
            let timeout_ms = ((a2 as u16) * 1000).min(60000);
            #[cfg(not(test))]
            {
                let bmode = match mode {
                    0 => drivers::backlight::BacklightMode::Off,
                    1 => drivers::backlight::BacklightMode::Auto(timeout_ms),
                    _ => drivers::backlight::BacklightMode::On,
                };
                crate::kern_hw().backlight.set_mode(bmode);
            }
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                if let Some(ref mut d) = ctx.display {
                    let bmode = match mode {
                        0 => kernel::service::BacklightMode::Off,
                        1 => kernel::service::BacklightMode::Auto(timeout_ms),
                        _ => kernel::service::BacklightMode::On,
                    };
                    kernel::service::DisplayService::set_backlight_mode(*d, bmode);
                }
            }
            0
        }
        10 => {
            #[cfg(not(test))]
            crate::kern_hw().backlight.set_levels(a1 as u8, a2 as u8);
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                if let Some(ref mut d) = ctx.display {
                    kernel::service::DisplayService::set_backlight_brightness(
                        *d, a1 as u8, a2 as u8,
                    );
                }
            }
            0
        }
        11 => {
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
        12 => {
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

#[inline(never)]
unsafe fn dispatch_io_serial(sub: u8, a1: usize, a2: usize) -> usize {
    match sub {
        0 => {
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
        1 => {
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
        2 => {
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
        3 => {
            py32_hal::uart::wait_tx_complete();
            0
        }
        _ => 0,
    }
}

#[inline(never)]
unsafe fn dispatch_io_bus(sub: u8, a1: usize, a2: usize) -> usize {
    match sub {
        0 => {
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
        1 => {
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
        2 => {
            crate::services::primitives::hw_gpio_write(a1 as u8, a2 != 0);
            0
        }
        3 => crate::services::primitives::hw_gpio_read(a1 as u8) as usize,
        4 => {
            #[cfg(not(test))]
            {
                crate::kern_hw().radio.read_reg(a1 as u8) as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| ctx.radio.read_reg(a1 as u8) as usize)
            }
        }
        5 => {
            #[cfg(not(test))]
            crate::kern_hw().radio.write_reg(a1 as u8, a2 as u16);
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.radio.write_reg(a1 as u8, a2 as u16);
            }
            0
        }
        6 => {
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
    match idx {
        0 => dispatch_io_key(a0 as u8, a1),
        1 => dispatch_io_power(a0 as u8, a1, a2),
        2 => unsafe { dispatch_io_serial(a0 as u8, a1, a2) },
        3 => match a0 as u8 {
            0 => py32_hal::delay::millis() as usize,
            1 => utils::math::div1000_u32(py32_hal::delay::millis()) as usize,
            2 => {
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
            3 => {
                py32_hal::delay::delay_us(a1 as u32);
                0
            }
            4 => {
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
        4 => unsafe { dispatch_io_bus(a0 as u8, a1, a2) },
        _ => 0,
    }
}
