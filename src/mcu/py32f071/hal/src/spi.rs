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

use crate::dma::{
    CH_SPI1_TX, CH_SPI2_RX, CH_SPI2_TX, DataSize, Direction, DmaChannel, DmaConfig, Priority, req,
};
use crate::gpio::Pin;
use crate::rcc;
use pac::gpio;
use pac::spi::{self, SPI1, SPI2};

static DUMMY_FF: u8 = 0xFF;

const DMA_SPI_TX_CFG: DmaConfig = DmaConfig {
    direction: Direction::MemToPeriph,
    priority: Priority::High,
    mem_size: DataSize::Byte8,
    periph_size: DataSize::Byte8,
    mem_inc: true,
    periph_inc: false,
    circular: false,
};

const DMA_SPI_DUMMY_TX_CFG: DmaConfig = DmaConfig {
    direction: Direction::MemToPeriph,
    priority: Priority::High,
    mem_size: DataSize::Byte8,
    periph_size: DataSize::Byte8,
    mem_inc: false,
    periph_inc: false,
    circular: false,
};

const DMA_SPI_RX_CFG: DmaConfig = DmaConfig {
    direction: Direction::PeriphToMem,
    priority: Priority::VeryHigh,
    mem_size: DataSize::Byte8,
    periph_size: DataSize::Byte8,
    mem_inc: true,
    periph_inc: false,
    circular: false,
};

/// Initialize SPI1 peripheral and configure PA5 (SCK) + PA7 (MOSI) as AF0 + DMA1 Ch2 (`SPI1_TX`).
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

    // Bind DMA1 Channel 2 -> SPI1_TX (0x04)
    DmaChannel::new(CH_SPI1_TX).remap(req::SPI1_TX);

    // BR=/4 (12 MHz @ 48 MHz PCLK)
    set_spi_br(SPI1, 0b001);
}

/// Initialize SPI2 peripheral for Flash + DMA1 Ch3 (`SPI2_RX`) and Ch4 (`SPI2_TX`).
/// PA0 (SCK, AF8), PA1 (MOSI, AF9), PA2 (MISO, AF9).
pub fn init_spi2() {
    unsafe {
        // Enable SPI2 Clock
        let apb1 = pac::rcc::RCC_APB1ENR.read_volatile();
        pac::rcc::RCC_APB1ENR.write_volatile(apb1 | pac::rcc::APB1ENR_SPI2);
    }

    let sck = Pin::new(gpio::GPIOA_BASE, 0);
    sck.set_pull(pac::gpio::PULL_UP);
    sck.configure_af(8, pac::gpio::SPEED_MEDIUM);

    let mosi = Pin::new(gpio::GPIOA_BASE, 1);
    mosi.set_pull(pac::gpio::PULL_NONE);
    mosi.configure_af(9, pac::gpio::SPEED_MEDIUM);

    let miso = Pin::new(gpio::GPIOA_BASE, 2);
    miso.set_pull(pac::gpio::PULL_NONE);
    miso.configure_af(9, pac::gpio::SPEED_MEDIUM);

    // Bind DMA1 Channel 3 -> SPI2_RX (0x05) and Channel 4 -> SPI2_TX (0x06)
    DmaChannel::new(CH_SPI2_RX).remap(req::SPI2_RX);
    DmaChannel::new(CH_SPI2_TX).remap(req::SPI2_TX);

    // BR=/2 = 24MHz (or /4 = 18MHz when PCLK > 48MHz)
    let br = if rcc::sysclk_hz() > 48_000_000 {
        0b001
    } else {
        0b000
    };
    set_spi_br(SPI2, br);
}

static SPI1_BASE_BR: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0b001);

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
    let base = speed as u8;
    SPI1_BASE_BR.store(base, core::sync::atomic::Ordering::Relaxed);
    let br = if rcc::sysclk_hz() > 48_000_000 {
        (base + 1).min(0b111)
    } else {
        base
    };
    set_spi_br(SPI1, br as u32);
}

/// Resynchronize SPI1 (LCD) and SPI2 (Flash) prescalers after APB clock changes
/// so LCD SCK stays <= 12 MHz and Flash SCK stays <= 24 MHz.
pub fn resync_clock(pclk_hz: u32) {
    let apb2 = unsafe { pac::rcc::RCC_APB2ENR.read_volatile() };
    if (apb2 & pac::rcc::APB2ENR_SPI1) != 0 {
        let base = SPI1_BASE_BR.load(core::sync::atomic::Ordering::Relaxed);
        let br = if pclk_hz > 48_000_000 {
            (base + 1).min(0b111)
        } else {
            base
        };
        set_spi_br(SPI1, br as u32);
    }
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
pub fn transfer_byte_instance(spi: pac::spi::SpiInstance, data: u8) -> Result<u8, SpiError> {
    unsafe {
        // Drain any stale RX byte / clear OVR so a prior timeout cannot shift subsequent reads.
        let sr = spi.sr().read_volatile();
        if (sr & (spi::SR_RXNE | spi::SR_OVR)) != 0 {
            let _ = spi.dr_8().read_volatile();
            let _ = spi.sr().read_volatile();
        }

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

#[inline(always)]
unsafe fn drain_rx_ovr(spi: pac::spi::SpiInstance) {
    unsafe {
        let _ = spi.dr_8().read_volatile();
        let _ = spi.sr().read_volatile();
    }
}

/// DMA-accelerated TX burst helper shared by `SPI1` (`CH_SPI1_TX`) and `SPI2` (`CH_SPI2_TX`).
fn dma_write_instance(spi: pac::spi::SpiInstance, dma_ch: u8, data: &[u8]) -> Result<(), SpiError> {
    if data.len() < 8 || data.len() > u16::MAX as usize {
        for &b in data {
            transfer_byte_instance(spi, b)?;
        }
        return Ok(());
    }
    wait_idle_instance(spi)?;
    unsafe { drain_rx_ovr(spi) };
    let mut ch = DmaChannel::new(dma_ch);
    let ok = {
        let xfer = ch.start_scoped(
            &DMA_SPI_TX_CFG,
            spi.dr_8() as u32,
            data.as_ptr() as u32,
            data.len() as u16,
        );
        unsafe { spi.cr2().write_volatile(spi::CR2_TXDMAEN) };
        xfer.wait(500_000)
    };
    unsafe { spi.cr2().write_volatile(0) };
    let idle = wait_idle_instance(spi);
    unsafe { drain_rx_ovr(spi) };
    if !ok {
        return Err(SpiError::Timeout);
    }
    idle
}

/// Transmit a slice of bytes over SPI1 (uses `DMA1_Ch2` for slices `>= 8 B`).
#[inline]
pub fn write_bytes(data: &[u8]) -> Result<(), SpiError> {
    dma_write_instance(SPI1, CH_SPI1_TX, data)
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

/// Transmit a slice of bytes over SPI2 (uses `DMA1_Ch4` for slices `>= 8 B`).
#[inline]
pub fn spi2_write_bytes(data: &[u8]) -> Result<(), SpiError> {
    dma_write_instance(SPI2, CH_SPI2_TX, data)
}

/// Read multiple bytes over SPI2 at 24 MHz using full-duplex `DMA1_Ch3` (`RX`) + `DMA1_Ch4` (`0xFF` dummy `TX`).
pub fn spi2_read_bytes(buffer: &mut [u8]) -> Result<(), SpiError> {
    if buffer.len() < 12 || buffer.len() > u16::MAX as usize {
        for b in buffer.iter_mut() {
            *b = spi2_transfer(0xFF)?;
        }
        return Ok(());
    }
    wait_idle_instance(SPI2)?;
    unsafe { drain_rx_ovr(SPI2) };
    let count = buffer.len() as u16;
    let dr_addr = SPI2.dr_8() as u32;
    let mut rx_ch = DmaChannel::new(CH_SPI2_RX);
    let mut tx_ch = DmaChannel::new(CH_SPI2_TX);
    let ok = {
        let rx_xfer =
            rx_ch.start_scoped(&DMA_SPI_RX_CFG, dr_addr, buffer.as_mut_ptr() as u32, count);
        let _tx_xfer = tx_ch.start_scoped(
            &DMA_SPI_DUMMY_TX_CFG,
            dr_addr,
            (&DUMMY_FF as *const u8) as u32,
            count,
        );
        unsafe {
            SPI2.cr2()
                .write_volatile(spi::CR2_RXDMAEN | spi::CR2_TXDMAEN);
        }
        rx_xfer.wait(500_000)
    };
    unsafe { SPI2.cr2().write_volatile(0) };
    let idle = wait_idle_instance(SPI2);
    unsafe { drain_rx_ovr(SPI2) };
    if !ok {
        return Err(SpiError::Timeout);
    }
    idle
}

/// Wait until SPI2 is no longer busy.
#[inline]
pub fn spi2_wait_idle() -> Result<(), SpiError> {
    wait_idle_instance(SPI2)
}
