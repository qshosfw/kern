//! Hardware SPI1 master driver for PY32F071.
//!
//! Used exclusively for the ST7565 LCD. The BK4819 uses separate
//! bit-bang SPI on different pins (PB8/PB9/PF9).
//!
//! Hardware SPI1 pin mapping:
//! - PA5 = SCK  (AF0)
//! - PA7 = MOSI (AF0)
//! - PA6 = LCD A0 (GPIO, directly controlled by LCD driver)
//! - PB2 = LCD CS (GPIO, directly controlled by LCD driver)
//!
//! Configuration matches the reference firmware:
//! - Master mode, 8-bit, MSB first
//! - CPOL=1 (idle high), CPHA=1 (second edge)
//! - Software NSS management
//! - Baud rate: fPCLK/16

use crate::gpio::Pin;
use crate::rcc;
use pac::gpio;
use pac::spi::{self, SPI1, SPI2};

/// Initialize SPI1 peripheral and configure PA5 (SCK) + PA7 (MOSI) as AF0.
pub fn init() {
    // Enable SPI1 clock
    rcc::enable_spi1();

    // Configure PA5 as AF0 (SPI1_SCK) with Medium slew-rate to prevent RF harmonics
    let sck = Pin::new(gpio::GPIOA_BASE, 5);
    sck.set_pull(pac::gpio::PULL_UP);
    sck.configure_af(pac::gpio::AF0, pac::gpio::SPEED_MEDIUM);

    // Configure PA7 as AF0 (SPI1_MOSI) with Medium slew-rate
    let mosi = Pin::new(gpio::GPIOA_BASE, 7);
    mosi.set_pull(pac::gpio::PULL_NONE);
    mosi.configure_af(pac::gpio::AF0, pac::gpio::SPEED_MEDIUM);

    // Configure SPI1 registers
    unsafe {
        // CR1: Master, CPOL=1, CPHA=1, 8-bit MSB, SSM+SSI, BR=/4 (12 MHz @ 48 MHz PCLK)
        // At 12 MHz, full 1024-byte screen transfer takes 0.68ms (4x faster blits, lower active power)
        let cr1 = spi::CR1_MSTR
            | spi::CR1_CPOL
            | spi::CR1_CPHA
            | spi::CR1_SSM
            | spi::CR1_SSI
            | (0b001 << spi::CR1_BR_SHIFT); // /4 = 48MHz/4 = 12MHz
        SPI1.cr1().write_volatile(cr1);

        // CR2: Clear to 0 (no DMA / SSOE / interrupts)
        SPI1.cr2().write_volatile(0);

        // Enable SPI
        let val = SPI1.cr1().read_volatile();
        SPI1.cr1().write_volatile(val | spi::CR1_SPE);
    }
}

/// Initialize SPI2 peripheral for Flash.
/// PA0 (SCK, AF8), PA1 (MISO, AF9), PA2 (MOSI, AF9).
pub fn init_spi2() {
    unsafe {
        // Enable SPI2 Clock
        let apb1 = pac::rcc::RCC_APB1ENR.read_volatile();
        pac::rcc::RCC_APB1ENR.write_volatile(apb1 | pac::rcc::APB1ENR_SPI2);
    }

    let sck = Pin::new(gpio::GPIOA_BASE, 0);
    sck.set_pull(pac::gpio::PULL_UP);
    sck.configure_af(8, pac::gpio::SPEED_HIGH);

    let mosi = Pin::new(gpio::GPIOA_BASE, 1);
    mosi.set_pull(pac::gpio::PULL_NONE);
    mosi.configure_af(9, pac::gpio::SPEED_HIGH);

    let miso = Pin::new(gpio::GPIOA_BASE, 2);
    miso.set_pull(pac::gpio::PULL_NONE);
    miso.configure_af(9, pac::gpio::SPEED_HIGH);

    unsafe {
        let cr1 = spi::CR1_MSTR
            | spi::CR1_CPOL
            | spi::CR1_CPHA
            | spi::CR1_SSM
            | spi::CR1_SSI
            | (0b000 << spi::CR1_BR_SHIFT); // BR=/2 = 24MHz
        SPI2.cr1().write_volatile(cr1);
        SPI2.cr2().write_volatile(0);
        let val = SPI2.cr1().read_volatile();
        SPI2.cr1().write_volatile(val | spi::CR1_SPE);
    }
}

/// Dynamic baud rate prescalers for SPI1 (LCD interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spi1Speed {
    /// 750 kHz (/64): Whisper profile (deltafw standard), 144MHz is 192nd harmonic (>80dB down, zero RF desense)
    Whisper = 0b101,
    /// 1.5 MHz (/32): Low profile for receiver protection
    Low = 0b100,
    /// 3 MHz (/16): Ultra-low EMI, quiet RF operation, lowest active power
    Quiet = 0b011,
    /// 6 MHz (/8): Balanced standard profile
    Standard = 0b010,
    /// 12 MHz (/4): Turbo profile for high-speed animations & 180 Hz temporal FRC
    Turbo = 0b001,
}

/// Dynamically adjust SPI1 baudrate prescaler without resetting peripheral state.
pub fn set_spi1_speed(speed: Spi1Speed) {
    unsafe {
        // Wait for current transmission to finish
        let mut timeout = 100_000;
        while SPI1.sr().read_volatile() & spi::SR_BSY != 0 {
            timeout -= 1;
            if timeout == 0 {
                break;
            }
        }
        let mut cr1 = SPI1.cr1().read_volatile();
        // SPE must be cleared before modifying BR bits
        SPI1.cr1().write_volatile(cr1 & !spi::CR1_SPE);
        cr1 &= !(0b111 << spi::CR1_BR_SHIFT);
        cr1 |= (speed as u32) << spi::CR1_BR_SHIFT;
        SPI1.cr1().write_volatile(cr1 | spi::CR1_SPE);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiError {
    Timeout,
}

/// Transmit a single byte over SPI1 and read back 32-bit DR to clear RXNE.
#[inline]
pub fn write_byte(data: u8) -> Result<(), SpiError> {
    transfer_byte(data).map(|_| ())
}

/// Transfer a byte over SPI1 (returns received byte).
#[inline]
pub fn transfer_byte(data: u8) -> Result<u8, SpiError> {
    unsafe {
        let mut timeout = 100_000;
        while SPI1.sr().read_volatile() & spi::SR_TXE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        SPI1.dr_8().write_volatile(data);

        let mut timeout = 100_000;
        while SPI1.sr().read_volatile() & spi::SR_RXNE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        Ok(SPI1.dr_8().read_volatile())
    }
}

/// Wait until SPI1 is no longer busy (all transfers complete).
#[inline]
pub fn wait_idle() -> Result<(), SpiError> {
    unsafe {
        let mut timeout = 100_000;
        while SPI1.sr().read_volatile() & spi::SR_BSY != 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        Ok(())
    }
}

/// Transmit a slice of bytes over SPI1.
#[inline]
pub fn write_bytes(data: &[u8]) -> Result<(), SpiError> {
    for &b in data {
        write_byte(b)?;
    }
    Ok(())
}

/// Transfer a byte over SPI2 (returns received byte).
#[inline]
pub fn spi2_transfer(data: u8) -> Result<u8, SpiError> {
    unsafe {
        let mut timeout = 100_000;
        while SPI2.sr().read_volatile() & spi::SR_TXE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        SPI2.dr_8().write_volatile(data);

        let mut timeout = 100_000;
        while SPI2.sr().read_volatile() & spi::SR_RXNE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        Ok(SPI2.dr_8().read_volatile())
    }
}

/// Transmit a byte over SPI2.
#[inline]
pub fn spi2_write_byte(data: u8) -> Result<(), SpiError> {
    spi2_transfer(data).map(|_| ())
}

/// Wait until SPI2 is no longer busy.
#[inline]
pub fn spi2_wait_idle() -> Result<(), SpiError> {
    unsafe {
        let mut timeout = 100_000;
        while SPI2.sr().read_volatile() & spi::SR_BSY != 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        Ok(())
    }
}
