//! USART1 polled serial driver for PY32F071.
//!
//! Provides basic TX/RX for the debug console and Rigwire protocol.
//! Pin mapping: PA9=TX (AF1), PA10=RX (AF1).
//!
//! Default configuration: 115200 baud, 8N1 (matching the reference firmware).

use crate::dma::{
    CH_USART1_RX, DataSize, Direction, DmaChannel, DmaConfig, Priority, remaining_count, req,
};
use crate::gpio::Pin;
use crate::rcc;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use pac::dma;
use pac::gpio;
use pac::usart;

static CURRENT_BAUD: AtomicU32 = AtomicU32::new(115_200);
#[cfg(feature = "target_k10pro")]
static USART3_BAUD: AtomicU32 = AtomicU32::new(115_200);

/// Generic register-level handle for any PY32F071 USART peripheral instance (`USART1..4`).
#[derive(Clone, Copy)]
pub struct UsartInstance {
    pub base: u32,
}

impl UsartInstance {
    pub const USART1: Self = Self {
        base: usart::USART1_BASE,
    };
    pub const USART3: Self = Self {
        base: usart::USART3_BASE,
    };

    #[inline(always)]
    const fn sr(self) -> *const u32 {
        self.base as *const u32
    }

    #[inline(always)]
    const fn dr(self) -> *mut u32 {
        (self.base + 0x04) as *mut u32
    }

    #[inline(always)]
    const fn brr(self) -> *mut u32 {
        (self.base + 0x08) as *mut u32
    }

    #[inline(always)]
    const fn cr1(self) -> *mut u32 {
        (self.base + 0x0C) as *mut u32
    }

    #[inline(always)]
    const fn cr2(self) -> *mut u32 {
        (self.base + 0x10) as *mut u32
    }

    #[inline(always)]
    const fn cr3(self) -> *mut u32 {
        (self.base + 0x14) as *mut u32
    }

    /// Enable TX/RX 8N1 and apply initial baud rate for this USART instance.
    pub fn enable_8n1(self, baud: u32) {
        unsafe {
            self.cr1().write_volatile(0);
            self.cr2().write_volatile(0);
            self.cr3().write_volatile(0);
            self.cr1()
                .write_volatile(usart::CR1_UE | usart::CR1_TE | usart::CR1_RE);
        }
        self.apply_baud(rcc::sysclk_hz(), baud);
    }

    /// Recompute `USART_BRR` for `pclk_hz` and `baud` after draining any in-flight TX byte.
    pub fn apply_baud(self, pclk_hz: u32, baud: u32) {
        let baud = if baud == 0 { 115_200 } else { baud };
        unsafe {
            let cr1 = self.cr1().read_volatile();
            if (cr1 & usart::CR1_UE) == 0 {
                return;
            }
            if (cr1 & usart::CR1_TE) != 0 {
                self.wait_tx_complete();
            }
            self.cr1().write_volatile(cr1 & !usart::CR1_UE);
            self.brr()
                .write_volatile(crate::div::udiv(pclk_hz + (baud >> 1), baud));
            self.cr1().write_volatile(cr1);
        }
    }

    /// Blocking single-byte write with timeout.
    pub fn write_byte(self, byte: u8) -> Result<(), UartError> {
        unsafe {
            let mut timeout = 100_000u32;
            while self.sr().read_volatile() & usart::SR_TXE == 0 {
                timeout -= 1;
                if timeout == 0 {
                    return Err(UartError::Timeout);
                }
            }
            self.dr().write_volatile(byte as u32);
            Ok(())
        }
    }

    /// Write a slice of bytes synchronously.
    pub fn write_bytes(self, data: &[u8]) -> Result<(), UartError> {
        for &b in data {
            self.write_byte(b)?;
        }
        Ok(())
    }

    /// Wait until all queued TX bits have shifted out onto the line (`SR.TC`).
    pub fn wait_tx_complete(self) {
        unsafe {
            let mut timeout = 100_000u32;
            while self.sr().read_volatile() & usart::SR_TC == 0 {
                timeout -= 1;
                if timeout == 0 {
                    break;
                }
            }
        }
    }

    /// Discard RF-corrupted (noise/framing/parity) bytes, clear stale overrun, and report RX readiness.
    #[inline]
    pub fn is_readable(self) -> bool {
        unsafe {
            let sr = self.sr().read_volatile();
            if (sr & (usart::SR_NE | usart::SR_FE | usart::SR_PE)) != 0 {
                let _ = self.dr().read_volatile();
                return false;
            }
            if (sr & usart::SR_RXNE) != 0 {
                return true;
            }
            if (sr & usart::SR_ORE) != 0 {
                let _ = self.dr().read_volatile();
            }
            false
        }
    }

    /// Non-blocking single-byte read.
    #[inline]
    pub fn try_read_byte(self) -> Option<u8> {
        if self.is_readable() {
            Some(unsafe { self.dr().read_volatile() as u8 })
        } else {
            None
        }
    }
}

/// Initialize USART3 on `PB10`/`PB11` (`AF4`) for the K10 Pro coprocessor link (`PHY6235SC`).
#[cfg(feature = "target_k10pro")]
pub fn init_usart3(baud: u32) {
    let baud = if baud == 0 { 115_200 } else { baud };
    USART3_BAUD.store(baud, Ordering::Relaxed);
    rcc::enable_usart3();
    rcc::enable_syscfg();

    let tx = Pin::new(gpio::GPIOB_BASE, 10);
    tx.configure_af(pac::gpio::AF4, pac::gpio::SPEED_MEDIUM);
    let rx = Pin::new(gpio::GPIOB_BASE, 11);
    rx.set_pull(pac::gpio::PULL_UP);
    rx.configure_af(pac::gpio::AF4, pac::gpio::SPEED_MEDIUM);

    UsartInstance::USART3.enable_8n1(baud);
}

/// Update `USART3` baud rate dynamically (for coprocessor link speed negotiation).
#[cfg(feature = "target_k10pro")]
pub fn set_usart3_baud(baud: u32) {
    let baud = if baud == 0 { 115_200 } else { baud };
    USART3_BAUD.store(baud, Ordering::Relaxed);
    resync_usart3_clock(rcc::sysclk_hz());
}

/// Recompute `USART3_BRR` after a system clock switch (`rcc::set_sysclk_*`).
#[cfg(feature = "target_k10pro")]
pub fn resync_usart3_clock(pclk_hz: u32) {
    if unsafe { (pac::rcc::RCC_APB1ENR.read_volatile() & pac::rcc::APB1ENR_USART3) == 0 } {
        return;
    }
    UsartInstance::USART3.apply_baud(pclk_hz, USART3_BAUD.load(Ordering::Relaxed));
}

const RX_DMA_CAP: usize = 256;

#[repr(align(4))]
struct RxDmaRing([u8; RX_DMA_CAP]);

static mut RX_DMA_RING: RxDmaRing = RxDmaRing([0u8; RX_DMA_CAP]);
static RX_DMA_TAIL: AtomicU8 = AtomicU8::new(0);

const DMA_UART_RX_CFG: DmaConfig = DmaConfig {
    direction: Direction::PeriphToMem,
    priority: Priority::High,
    mem_size: DataSize::Byte8,
    periph_size: DataSize::Byte8,
    mem_inc: true,
    periph_inc: false,
    circular: true,
};

#[inline(always)]
fn dma_rx_head() -> u8 {
    let rem = remaining_count(CH_USART1_RX);
    if rem == 0 || rem > RX_DMA_CAP as u16 {
        RX_DMA_TAIL.load(Ordering::Relaxed)
    } else {
        ((RX_DMA_CAP as u16) - rem) as u8
    }
}

/// Initialize USART1 at 115200 baud, 8N1 with hardware `DMA1_Ch6` circular `256 B` RX ring.
pub fn init() {
    rcc::enable_usart1();
    rcc::enable_syscfg();

    // Configure PA9 as AF1 (USART1_TX) and PA10 as AF1 (USART1_RX)
    let tx = Pin::new(gpio::GPIOA_BASE, 9);
    tx.configure_af(pac::gpio::AF1, pac::gpio::SPEED_MEDIUM);
    let rx = Pin::new(gpio::GPIOA_BASE, 10);
    rx.set_pull(pac::gpio::PULL_UP);
    rx.configure_af(pac::gpio::AF1, pac::gpio::SPEED_MEDIUM);

    unsafe {
        usart::USART_CR1.write_volatile(0);
        usart::USART_CR2.write_volatile(0);
        let _ = usart::USART_SR.read_volatile();
        let _ = usart::USART_DR.read_volatile();
    }

    // Arm DMA1 Channel 6 (`req::USART1_RX = 0x07`) in permanent 256-byte circular mode
    let mut ch = DmaChannel::new(CH_USART1_RX);
    ch.disable();
    ch.clear_flags();
    ch.remap(req::USART1_RX);
    RX_DMA_TAIL.store(0, Ordering::Release);
    ch.set_peripheral_address(usart::USART_DR as u32);
    ch.set_memory_address(core::ptr::addr_of_mut!(RX_DMA_RING) as u32);
    ch.set_transfer_count(RX_DMA_CAP as u16);
    unsafe {
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        dma::ccr(CH_USART1_RX)
            .write_volatile(DmaChannel::encode_ccr(&DMA_UART_RX_CFG) | dma::CCR_EN);
        usart::USART_CR3.write_volatile(usart::CR3_DMAR);
        usart::USART_CR1.write_volatile(usart::CR1_UE | usart::CR1_TE | usart::CR1_RE);
    }
    resync_clock(rcc::sysclk_hz());
}

/// Legacy `USART1` (`IRQ 27`) vector entry (clears any stray line error flags).
pub fn on_usart1_irq() {
    unsafe {
        let sr = usart::USART_SR.read_volatile();
        if (sr & (usart::SR_ORE | usart::SR_NE | usart::SR_FE | usart::SR_PE)) != 0 {
            let _ = usart::USART_DR.read_volatile();
        }
    }
}

/// Dynamically update USART1 baud rate (e.g. 115200..921600 or 38400).
pub fn set_baud(baud: u32) {
    let baud = if baud == 0 { 115_200 } else { baud };
    if CURRENT_BAUD.load(Ordering::Relaxed) == baud {
        return;
    }
    CURRENT_BAUD.store(baud, Ordering::Relaxed);
    resync_clock(rcc::sysclk_hz());
}

/// Recompute `USART_BRR` after a system clock switch (`rcc::set_sysclk_*`).
pub fn resync_clock(pclk_hz: u32) {
    if unsafe { (pac::rcc::RCC_APB2ENR.read_volatile() & pac::rcc::APB2ENR_USART1) == 0 } {
        return;
    }
    UsartInstance::USART1.apply_baud(pclk_hz, CURRENT_BAUD.load(Ordering::Relaxed));
}

pub static LAST_UART_ACTIVITY_MS: AtomicU32 = AtomicU32::new(0);

#[inline(always)]
pub fn mark_activity() {
    LAST_UART_ACTIVITY_MS.store(crate::delay::millis(), Ordering::Relaxed);
}

#[inline(always)]
pub fn is_active(timeout_ms: u32) -> bool {
    let last = LAST_UART_ACTIVITY_MS.load(Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    crate::delay::millis().wrapping_sub(last) < timeout_ms
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UartError {
    Timeout,
}

/// Transmit a single byte (blocking).
#[inline]
pub fn write_byte(byte: u8) -> Result<(), UartError> {
    mark_activity();
    UsartInstance::USART1.write_byte(byte)
}

/// Transmit a byte slice (blocking).
pub fn write_bytes(data: &[u8]) -> Result<(), UartError> {
    mark_activity();
    UsartInstance::USART1.write_bytes(data)
}

/// Wait until all transmitted bytes have completely shifted out (Transmission Complete).
#[inline]
pub fn wait_tx_complete() {
    UsartInstance::USART1.wait_tx_complete();
}

/// Transmit a string (blocking).
pub fn write_str(s: &str) -> Result<(), UartError> {
    write_bytes(s.as_bytes())
}

/// Transmit a string followed by CRLF.
pub fn write_line(s: &str) -> Result<(), UartError> {
    write_str(s)?;
    write_byte(b'\r')?;
    write_byte(b'\n')?;
    Ok(())
}

/// Clear pending UART error flags (overrun, noise, framing) and sync DMA RX tail to head.
#[inline]
pub fn clear_errors() {
    cortex_m::interrupt::free(|_| unsafe {
        let _ = usart::USART_SR.read_volatile();
        let _ = usart::USART_DR.read_volatile();
        RX_DMA_TAIL.store(dma_rx_head(), Ordering::Release);
    });
}

/// Check hardware `USART_SR` directly (discarding RF-corrupted bytes and clearing stale `ORE`).
#[inline]
fn hw_is_readable() -> bool {
    unsafe {
        let sr = usart::USART_SR.read_volatile();
        if (sr & (usart::SR_NE | usart::SR_FE | usart::SR_PE)) != 0 {
            let _ = usart::USART_DR.read_volatile();
            return false;
        }
        if (sr & usart::SR_RXNE) != 0 && (usart::USART_CR3.read_volatile() & usart::CR3_DMAR) == 0 {
            return true;
        }
        if (sr & usart::SR_ORE) != 0 {
            let _ = usart::USART_DR.read_volatile();
        }
        false
    }
}

/// Check if a valid byte is ready in the hardware circular DMA RX ring (or non-DMA DR).
#[inline]
pub fn is_readable() -> bool {
    if dma_rx_head() != RX_DMA_TAIL.load(Ordering::Relaxed) {
        return true;
    }
    hw_is_readable()
}

/// Read a single byte (blocking).
#[inline]
pub fn read_byte() -> Result<u8, UartError> {
    let mut timeout = 100_000u32;
    loop {
        if let Some(b) = try_read_byte() {
            return Ok(b);
        }
        timeout -= 1;
        if timeout == 0 {
            return Err(UartError::Timeout);
        }
    }
}

/// Try to read a byte from the hardware circular `DMA1_Ch6` RX ring without blocking.
#[inline]
pub fn try_read_byte() -> Option<u8> {
    let tail = RX_DMA_TAIL.load(Ordering::Relaxed);
    if tail != dma_rx_head() {
        let b = unsafe { (*core::ptr::addr_of!(RX_DMA_RING)).0[tail as usize] };
        RX_DMA_TAIL.store(tail.wrapping_add(1), Ordering::Release);
        mark_activity();
        return Some(b);
    }
    if hw_is_readable() {
        mark_activity();
        Some(unsafe { usart::USART_DR.read_volatile() as u8 })
    } else {
        None
    }
}
