//! Internal & External Flash Controller Traits.

pub trait InternalFlashDevice {
    fn erase_page(&mut self, addr: u32) -> Result<(), ()>;
    fn program_page(&mut self, addr: u32, data: &[u8; 256]) -> Result<(), ()>;
    fn rdp_level(&self) -> u8;
}

/// SPI + CS + Delay + Watchdog bus for external SPI NOR flash chips (PY25Q16, W25Q16, etc.).
pub trait SpiFlashBus:
    crate::spi::SpiDevice + crate::time::Delay + crate::wdt::WatchdogDevice
{
    fn cs_low(&mut self);
    fn cs_high(&mut self);
}
