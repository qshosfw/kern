//! Internal Flash Controller Trait.

pub trait InternalFlashDevice {
    fn erase_page(&mut self, addr: u32) -> Result<(), ()>;
    fn program_page(&mut self, addr: u32, data: &[u8; 256]) -> Result<(), ()>;
    fn rdp_level(&self) -> u8;
}
