//! Hardware CRC Calculation Trait.

pub trait CrcDevice {
    fn reset(&mut self);
    fn feed_bytes(&mut self, bytes: &[u8]) -> u32;
    fn compute(&mut self, bytes: &[u8]) -> u32 {
        self.reset();
        self.feed_bytes(bytes)
    }
}
