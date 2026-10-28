//! SPI bus abstraction.

/// An SPI master device.
pub trait SpiDevice {
    /// Write a single byte, blocking until complete.
    fn write_byte(&mut self, byte: u8);
    /// Write a byte and return the received byte (full-duplex transfer).
    fn transfer_byte(&mut self, byte: u8) -> u8;
    /// Wait until the SPI peripheral is idle.
    fn wait_idle(&mut self);
}
