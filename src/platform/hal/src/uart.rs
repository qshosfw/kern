//! USART1 polled serial driver for PY32F071.
//!
//! Provides basic TX/RX for the debug console and Rigwire protocol.
//! Pin mapping: PA9=TX (AF1), PA10=RX (AF1).
//!
//! Default configuration: 115200 baud, 8N1 (matching the reference firmware).

use crate::gpio::Pin;
use crate::rcc;
use pac::gpio;
use pac::usart;

/// Initialize USART1 at 115200 baud, 8N1.
pub fn init() {
    rcc::enable_usart1();
    rcc::enable_syscfg();

    // Configure PA9 as AF1 (USART1_TX)
    let tx = Pin::new(gpio::GPIOA_BASE, 9);
    tx.set_speed(pac::gpio::SPEED_MEDIUM);
    tx.configure_af(pac::gpio::AF1, pac::gpio::SPEED_MEDIUM);

    // Configure PA10 as AF1 (USART1_RX)
    let rx = Pin::new(gpio::GPIOA_BASE, 10);
    rx.set_pull(pac::gpio::PULL_UP);
    rx.configure_af(pac::gpio::AF1, pac::gpio::SPEED_MEDIUM);

    let pclk = rcc::sysclk_hz();
    unsafe {
        // Disable USART before configuration
        usart::USART_CR1.write_volatile(0);

        // Baud rate: BRR = PCLK / baud (e.g. 48000000 / 115200 = 417, 48000000 / 38400 = 1250)
        let brr = (pclk + 115200 / 2) / 115200;
        usart::USART_BRR.write_volatile(brr);

        // CR2: 1 stop bit (default 0)
        usart::USART_CR2.write_volatile(0);
        // CR3: no flow control (default 0)
        usart::USART_CR3.write_volatile(0);

        // CR1: Enable USART, TX, RX (8-bit word, no parity)
        usart::USART_CR1.write_volatile(usart::CR1_UE | usart::CR1_TE | usart::CR1_RE);
    }
}

/// Dynamically update USART1 baud rate (e.g. 115200 or 38400)
pub fn set_baud(baud: u32) {
    let baud = if baud == 0 { 115200 } else { baud };
    let pclk = rcc::sysclk_hz();
    unsafe {
        let cr1 = usart::USART_CR1.read_volatile();
        usart::USART_CR1.write_volatile(0);
        let brr = (pclk + baud / 2) / baud;
        usart::USART_BRR.write_volatile(brr);
        usart::USART_CR1.write_volatile(cr1);
    }
}

use core::sync::atomic::{AtomicU32, Ordering};

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
    unsafe {
        let mut timeout = 100_000;
        // Wait until TX register is empty
        while usart::USART_SR.read_volatile() & usart::SR_TXE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(UartError::Timeout);
            }
        }
        usart::USART_DR.write_volatile(byte as u32);
        Ok(())
    }
}

/// Transmit a byte slice (blocking).
pub fn write_bytes(data: &[u8]) -> Result<(), UartError> {
    for &b in data {
        write_byte(b)?;
    }
    Ok(())
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

/// Clear pending UART error flags (overrun, noise, framing) and flush DR.
#[inline]
pub fn clear_errors() {
    unsafe {
        let _ = usart::USART_SR.read_volatile();
        let _ = usart::USART_DR.read_volatile();
    }
}

/// Check if a byte is available to read, clearing any pending error flags.
#[inline]
pub fn is_readable() -> bool {
    unsafe {
        let sr = usart::USART_SR.read_volatile();
        if sr & (usart::SR_ORE | usart::SR_NE | usart::SR_FE | usart::SR_PE) != 0 {
            let _ = usart::USART_DR.read_volatile();
        }
        sr & usart::SR_RXNE != 0
    }
}

/// Read a single byte (blocking).
#[inline]
pub fn read_byte() -> Result<u8, UartError> {
    unsafe {
        let mut timeout = 100_000;
        while usart::USART_SR.read_volatile() & usart::SR_RXNE == 0 {
            timeout -= 1;
            if timeout == 0 {
                return Err(UartError::Timeout);
            }
        }
        mark_activity();
        Ok(usart::USART_DR.read_volatile() as u8)
    }
}

/// Try to read a byte without blocking. Returns `None` if no data available.
#[inline]
pub fn try_read_byte() -> Option<u8> {
    if is_readable() {
        mark_activity();
        Some(unsafe { usart::USART_DR.read_volatile() as u8 })
    } else {
        None
    }
}
