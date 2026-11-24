//! I/O & Hardware Peripheral System Call Dispatcher (`Op::Io*`, domain `0x8`).

#[cfg(test)]
use super::active_ctx_opt;
use super::{ptr_slice, ptr_slice_mut, slice_or_empty};
use kernel::proc::abi::KrxBusReq;

#[inline(always)]
fn power_ref() -> &'static drivers::battery::SystemPower {
    crate::power_service()
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
            crate::services::primitives::hw_gpio_write(0, a1 != 0);
            0
        }
        _ => 0,
    }
}

#[cfg(not(test))]
#[inline(never)]
fn can_raw_serial(port_idx: usize) -> bool {
    let pid = super::service::active_caller_pid();
    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    let wire_active = crate::kern_hw().wire.session.is_active();
    #[cfg(not(any(feature = "uart", feature = "usb_cdc")))]
    let wire_active = false;
    crate::hardware::serial_router_mut().can_raw_io(port_idx, pid, wire_active)
}

#[inline(never)]
unsafe fn serial_xfer(is_write: bool, ptr: usize, len: usize) -> usize {
    if ptr == 0 || len == 0 {
        return 0;
    }
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) };
    #[cfg(not(test))]
    {
        #[cfg(feature = "usb")]
        if crate::kern_hw().usb.is_connected() {
            if !can_raw_serial(kernel::service::PortId::UsbCdc as usize) {
                return 0;
            }
            return if is_write {
                let _ = crate::kern_hw().usb.cdc_write(slice);
                len
            } else {
                crate::kern_hw().usb.cdc_read(slice)
            };
        }
        #[cfg(feature = "uart")]
        {
            if !can_raw_serial(kernel::service::PortId::Uart0 as usize) {
                return 0;
            }
            if is_write {
                let _ = py32_hal::uart::write_bytes(slice);
                len
            } else {
                crate::kern_hw().uart.read(slice)
            }
        }
        #[cfg(not(feature = "uart"))]
        {
            let _ = (is_write, slice);
            0
        }
    }
    #[cfg(test)]
    {
        let Some(ctx) = active_ctx_opt() else {
            return 0;
        };
        #[cfg(feature = "usb")]
        if let Some(ref mut usb) = ctx.usb
            && usb.is_connected()
        {
            return if is_write {
                let _ = usb.cdc_write(slice);
                len
            } else {
                usb.cdc_read(slice)
            };
        }
        #[cfg(feature = "uart")]
        {
            return if is_write {
                ctx.uart.write(slice);
                len
            } else {
                ctx.uart.read(slice)
            };
        }
        #[cfg(not(feature = "uart"))]
        {
            let _ = (is_write, slice);
            0
        }
    }
}

unsafe fn dispatch_io_serial(sub: u8, a1: usize, a2: usize) -> usize {
    use ::abi::sub::io_serial;
    match sub {
        io_serial::WRITE | io_serial::READ => {
            unsafe { serial_xfer(sub == io_serial::WRITE, a1, (a2 as u16) as usize) }
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
        io_bus::GPIO_SET => {
            crate::services::primitives::hw_gpio_write(a1 as u8, a2 != 0);
            return 0;
        }
        io_bus::GPIO_GET => return crate::services::primitives::hw_gpio_read(a1 as u8) as usize,
        io_bus::RF_RD => {
            #[cfg(not(test))]
            {
                return crate::kern_hw().radio.read_reg(a1 as u8) as usize;
            }
            #[cfg(test)]
            {
                return active_ctx_opt().map_or(0, |ctx| ctx.radio.read_reg(a1 as u8) as usize);
            }
        }
        io_bus::RF_WR => {
            #[cfg(not(test))]
            crate::kern_hw().radio.write_reg(a1 as u8, a2 as u16);
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.radio.write_reg(a1 as u8, a2 as u16);
            }
            return 0;
        }
        _ => {}
    }
    let req = a1 as *const KrxBusReq;
    if req.is_null() {
        return 1;
    }
    let r = unsafe { &*req };
    match sub {
        io_bus::I2C => {
            let mut i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let tx_opt = unsafe { ptr_slice(r.tx, r.tx_len as usize) };
            let rx_opt = unsafe { ptr_slice_mut(r.rx, r.rx_len as usize) };
            usize::from(
                i2c.xfer_reg(r.addr as u8, (r.addr >> 8) as u8, tx_opt, rx_opt)
                    .is_err(),
            )
        }
        io_bus::SPI => {
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
        io_bus::FLASH_RD => {
            if let Some(dst) = unsafe { ptr_slice_mut(r.rx, r.rx_len as usize) } {
                #[cfg(not(test))]
                {
                    return usize::from(crate::kern_fs().fs.dev.read_data(r.addr, dst).is_err());
                }
                #[cfg(test)]
                if let Some(ctx) = active_ctx_opt() {
                    return usize::from(ctx.storage.fs.dev.read_data(r.addr, dst).is_err());
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
                while rem > 0 {
                    let step = rem.min(50);
                    bsp::Board::wdt_feed();
                    py32_hal::delay::delay_ms(step);
                    rem -= step;
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
                    let mut rem = power_ref().uptime_secs;
                    let mut d = 1u8;
                    let mut h = 0u8;
                    while rem >= 3600 {
                        rem -= 3600;
                        h += 1;
                        if h == 24 {
                            h = 0;
                            d = if d == 28 { 1 } else { d + 1 };
                        }
                    }
                    let (m, s) = utils::math::div60_u16(rem as u16);
                    unsafe {
                        *out = 0xEA;
                        *out.add(1) = 0x07;
                        *out.add(2) = 1;
                        *out.add(3) = d;
                        *out.add(4) = h;
                        *out.add(5) = m as u8;
                        *out.add(6) = s;
                    }
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
