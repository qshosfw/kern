//! I/O & Hardware Peripheral System Call Handlers (`Op::IoKey`, `Op::IoSerial`, `Op::IoBus`, domain `0x8`).

use super::{ptr_ref, ptr_slice, ptr_slice_mut, slice_or_empty};
use kernel::proc::abi::KrxBusReq;
use kernel::service::PowerService;

const HW_GPIO_PINS: [py32_hal::gpio::Pin; 4] = [
    bsp::pins::FLASHLIGHT,
    bsp::pins::PTT,
    bsp::pins::AUDIO_PA,
    bsp::pins::BACKLIGHT,
];

fn hw_spi_transfer(cs_pin: u8, tx: &[u8], rx: &mut [u8]) -> Result<(), ()> {
    let is_flash = cs_pin == 0;
    let cs = if is_flash {
        bsp::pins::FLASH_CS
    } else {
        bsp::pins::LCD_CS
    };
    cs.set_low();
    let count = tx.len().max(rx.len());
    let mut res = Ok(());
    for i in 0..count {
        let out_b = tx.get(i).copied().unwrap_or(0);
        let xfer = if is_flash {
            py32_hal::spi::spi2_transfer(out_b)
        } else {
            py32_hal::spi::transfer_byte(out_b)
        };
        let Ok(in_b) = xfer else {
            res = Err(());
            break;
        };
        if let Some(slot) = rx.get_mut(i) {
            *slot = in_b;
        }
    }
    let _ = if is_flash {
        py32_hal::spi::spi2_wait_idle()
    } else {
        py32_hal::spi::wait_idle()
    };
    cs.set_high();
    res
}

fn hw_gpio_read(pin: u8) -> bool {
    if let Some(&p) = HW_GPIO_PINS.get(pin as usize) {
        p.is_high() ^ (pin == 1)
    } else if pin == 4 {
        crate::services::power().is_charging()
    } else {
        false
    }
}

fn hw_gpio_write(pin: u8, high: bool) {
    if matches!(pin, 0 | 2 | 3) {
        let p = HW_GPIO_PINS[pin as usize];
        if high {
            p.set_high();
        } else {
            p.set_low();
        }
    }
}

pub unsafe fn op_io_key(a0: usize, a1: usize, _a2: usize) -> usize {
    use ::abi::sub::io_key;
    if !crate::system::admits_physical_input() {
        return 0;
    }
    match a0 as u8 {
        io_key::POLL => {
            bsp::Board::wdt_feed();
            drivers::keyboard::matrix::poll_matrix(&bsp::BspKeyMatrix)
                .map(|k| k as usize)
                .unwrap_or(0)
        }
        io_key::MATRIX => bsp::BspKeyMatrix::scan_raw_mask() as usize,
        io_key::IS_PRESSED => {
            let mask = bsp::BspKeyMatrix::scan_raw_mask();
            kernel::event::KeyCode::from_u8(a1 as u8)
                .is_some_and(|kc| (mask & drivers::keyboard::key_to_bit(kc)) != 0)
                as usize
        }
        _ => 0,
    }
}

fn can_raw_serial(port_idx: usize) -> bool {
    let pid = super::service::active_caller_pid();
    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    let wire_active = crate::kern_hw().wire.session.is_active();
    #[cfg(not(any(feature = "uart", feature = "usb_cdc")))]
    let wire_active = false;
    crate::hardware::serial_router_mut().can_raw_io(port_idx, pid, wire_active)
}

unsafe fn serial_xfer(is_write: bool, ptr: usize, len: usize) -> usize {
    if !super::guest_range_valid(ptr, len, !is_write) {
        return 0;
    }
    let hw = crate::kern_hw();
    #[cfg(feature = "usb")]
    if hw.usb.is_connected() {
        if !can_raw_serial(kernel::service::PortId::UsbCdc as usize) {
            return 0;
        }
        return if is_write {
            let _ = hw
                .usb
                .cdc_write(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) });
            len
        } else {
            hw.usb
                .cdc_read(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) })
        };
    }
    #[cfg(feature = "uart")]
    {
        if !can_raw_serial(kernel::service::PortId::Uart0 as usize) {
            return 0;
        }
        return if is_write {
            let _ = py32_hal::uart::write_bytes(unsafe {
                core::slice::from_raw_parts(ptr as *const u8, len)
            });
            len
        } else {
            let dst = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) };
            let mut n = 0;
            for b in dst {
                let Some(rx) = bsp::Board::uart_try_read_byte() else {
                    break;
                };
                *b = rx;
                n += 1;
            }
            n
        };
    }
    #[cfg(not(feature = "uart"))]
    0
}

pub unsafe fn op_io_serial(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::io_serial;
    match a0 as u8 {
        io_serial::WRITE | io_serial::READ => unsafe {
            serial_xfer((a0 as u8) == io_serial::WRITE, a1, (a2 as u16) as usize)
        },
        io_serial::FLUSH => {
            py32_hal::uart::wait_tx_complete();
            0
        }
        _ => 0,
    }
}

pub unsafe fn op_io_bus(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::io_bus;
    let sub = a0 as u8;
    match sub {
        io_bus::GPIO_SET => {
            hw_gpio_write(a1 as u8, a2 != 0);
            return 0;
        }
        io_bus::GPIO_GET => return hw_gpio_read(a1 as u8) as usize,
        io_bus::RF_RD => {
            let (_, radio) = crate::services::state_and_radio();
            return radio.read_reg(a1 as u8) as usize;
        }
        io_bus::RF_WR => {
            let (_, radio) = crate::services::state_and_radio();
            radio.write_reg(a1 as u8, a2 as u16);
            return 0;
        }
        _ => {}
    }
    let Some(r) = (unsafe { ptr_ref(a1 as *const KrxBusReq) }) else {
        return 1;
    };
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
            usize::from(hw_spi_transfer(r.addr as u8, tx_slice, rx_slice).is_err())
        }
        io_bus::FLASH_RD => {
            if let Some(dst) = unsafe { ptr_slice_mut(r.rx, r.rx_len as usize) } {
                return usize::from(
                    crate::services::storage_mut()
                        .dev
                        .read_data(r.addr, dst)
                        .is_err(),
                );
            }
            1
        }
        _ => 0,
    }
}
