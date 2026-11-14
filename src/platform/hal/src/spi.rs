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

    // BR=/4 (12 MHz @ 48 MHz PCLK)
    set_spi_br(SPI1, 0b001);
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

    // BR=/2 = 24MHz
    set_spi_br(SPI2, 0b000);
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

/// Update SPI baud rate prescaler cleanly after draining active bus transfers.
#[inline(never)]
fn set_spi_br(spi_inst: pac::spi::SpiInstance, br: u32) {
    let _ = wait_idle_instance(spi_inst);
    unsafe {
        let cr1 = spi::CR1_MSTR
            | spi::CR1_CPOL
            | spi::CR1_CPHA
            | spi::CR1_SSM
            | spi::CR1_SSI
            | ((br & 0b111) << spi::CR1_BR_SHIFT);
        // Writing cr1 (without CR1_SPE) disables SPE while updating BR
        spi_inst.cr1().write_volatile(cr1);
        spi_inst.cr2().write_volatile(0);
        spi_inst.cr1().write_volatile(cr1 | spi::CR1_SPE);
    }
}

/// Dynamically adjust SPI1 baudrate prescaler without resetting peripheral state.
pub fn set_spi1_speed(speed: Spi1Speed) {
    set_spi_br(SPI1, speed as u32);
}

/// Resynchronize SPI2 (Flash) prescaler after APB clock changes so SCK stays <= 24 MHz.
pub fn resync_clock(pclk_hz: u32) {
    let apb1 = unsafe { pac::rcc::RCC_APB1ENR.read_volatile() };
    if (apb1 & pac::rcc::APB1ENR_SPI2) != 0 {
        let br = if pclk_hz > 48_000_000 { 0b001 } else { 0b000 };
        set_spi_br(SPI2, br);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiError {
    Timeout,
}

/// Transfer a byte over a specific SPI instance (returns received byte).
#[inline(never)]
pub fn transfer_byte_instance(spi: pac::spi::SpiInstance, data: u8) -> Result<u8, SpiError> {
    unsafe {
        let mut timeout = 100_000;
        while spi.sr().read_volatile() & spi::SR_TXE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        spi.dr_8().write_volatile(data);

        let mut timeout = 100_000;
        while spi.sr().read_volatile() & spi::SR_RXNE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        Ok(spi.dr_8().read_volatile())
    }
}

/// Wait until a specific SPI instance is no longer busy.
#[inline(never)]
pub fn wait_idle_instance(spi: pac::spi::SpiInstance) -> Result<(), SpiError> {
    unsafe {
        let mut timeout = 100_000;
        while spi.sr().read_volatile() & spi::SR_BSY != 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(SpiError::Timeout);
            }
        }
        Ok(())
    }
}

/// Transmit a single byte over SPI1 and read back 32-bit DR to clear RXNE.
#[inline]
pub fn write_byte(data: u8) -> Result<(), SpiError> {
    transfer_byte(data).map(|_| ())
}

/// Transfer a byte over SPI1 (returns received byte).
#[inline]
pub fn transfer_byte(data: u8) -> Result<u8, SpiError> {
    transfer_byte_instance(SPI1, data)
}

/// Wait until SPI1 is no longer busy (all transfers complete).
#[inline]
pub fn wait_idle() -> Result<(), SpiError> {
    wait_idle_instance(SPI1)
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
    transfer_byte_instance(SPI2, data)
}

/// Transmit a byte over SPI2.
#[inline]
pub fn spi2_write_byte(data: u8) -> Result<(), SpiError> {
    spi2_transfer(data).map(|_| ())
}

/// Transmit a slice of bytes over SPI2.
#[inline]
pub fn spi2_write_bytes(data: &[u8]) -> Result<(), SpiError> {
    for &b in data {
        spi2_write_byte(b)?;
    }
    Ok(())
}

/// Read multiple bytes over SPI2 by clocking out 0xFF dummy bytes.
#[inline]
pub fn spi2_read_bytes(buffer: &mut [u8]) -> Result<(), SpiError> {
    for b in buffer.iter_mut() {
        *b = spi2_transfer(0xFF)?;
    }
    Ok(())
}

/// Wait until SPI2 is no longer busy.
#[inline]
pub fn spi2_wait_idle() -> Result<(), SpiError> {
    wait_idle_instance(SPI2)
}
