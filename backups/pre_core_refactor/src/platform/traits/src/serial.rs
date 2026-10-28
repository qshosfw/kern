//! Serial / UART abstraction.

/// A serial port backend.
pub trait SerialBackend {
    /// Write a byte slice to the serial port (blocking).
    fn write_bytes(&mut self, data: &[u8]);
    /// Try to read one byte. Returns None if no data available.
    fn try_read_byte(&mut self) -> Option<u8>;
}
