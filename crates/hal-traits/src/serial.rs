//! Serial / UART & Mux Control Abstractions.

/// A byte-oriented serial port backend.
pub trait SerialBackend {
    /// Write a byte slice to the serial port (blocking).
    fn write_bytes(&mut self, data: &[u8]);
    /// Try to read one byte. Returns None if no data available.
    fn try_read_byte(&mut self) -> Option<u8>;
}

/// Optional runtime serial line configuration and TX drain synchronization.
pub trait SerialControl: SerialBackend {
    /// Update active baud rate dynamically (e.g. switching between 9600 and 115200 bps).
    fn set_baud(&mut self, baud: u32);
    /// Wait until all queued TX bits have shifted out onto the physical wire.
    fn flush(&mut self);
}

/// Power rail / analog switch gate for muxed or companion subsystems (e.g. `BL1551B`, LDO enable).
pub trait PowerGate {
    fn set_enabled(&mut self, enabled: bool);
    fn is_enabled(&self) -> bool;
}
