//! Radio transceiver bus abstraction.

/// Low-level register access to the BK4819/BK4829 radio IC.
pub trait RadioBus {
    /// Read a 16-bit register value.
    fn read_reg(&mut self, addr: u8) -> u16;
    /// Write a 16-bit register value.
    fn write_reg(&mut self, addr: u8, val: u16);
}

/// Pipelined Burst RSSI Sweep Hardware Trait.
pub trait RfBurstSweep {
    fn burst_sweep(&mut self, start_hz: u32, step_hz: u32, out_rssi: &mut [i16]) -> usize;
}

/// Cycle-accurate Symbol Modulation Hardware Trait.
pub trait RfSymbolModulator {
    fn emit_symbol_stream(&mut self, freq_words: &[u16], durations_us: &[u16]) -> Result<(), ()>;
}
