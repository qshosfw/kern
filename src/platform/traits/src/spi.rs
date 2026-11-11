//! SPI bus abstraction.

/// An SPI master device.
pub trait SpiDevice {
    /// Write a single byte, blocking until complete.
    fn write_byte(&mut self, byte: u8);
    /// Write a byte and return the received byte (full-duplex transfer).
    fn transfer_byte(&mut self, byte: u8) -> u8;
    /// Read multiple bytes into `buf` by transmitting 0xFF dummy bytes.
    #[inline]
    fn read_bytes(&mut self, buf: &mut [u8]) {
        for b in buf.iter_mut() {
            *b = self.transfer_byte(0xFF);
        }
    }
    /// Write multiple bytes sequentially.
    #[inline]
    fn write_bytes(&mut self, buf: &[u8]) {
        for &b in buf {
            self.write_byte(b);
        }
    }
    /// Wait until the SPI peripheral is idle.
    fn wait_idle(&mut self);
}
