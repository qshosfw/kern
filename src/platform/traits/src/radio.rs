//! Radio transceiver bus abstraction.

/// Low-level register access to the BK4819/BK4829 radio IC.
pub trait RadioBus {
    /// Read a 16-bit register value.
    fn read_reg(&mut self, addr: u8) -> u16;
    /// Write a 16-bit register value.
    fn write_reg(&mut self, addr: u8, val: u16);
}
