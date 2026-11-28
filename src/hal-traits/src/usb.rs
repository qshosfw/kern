//! Portable USB 2.0 Full-Speed Device Controller hardware bus trait.
//!
//! Decouples `drivers::usb::UsbDriver` from MCU-specific PAC/HAL registers and BSP identity.

pub trait UsbBus {
    fn reg_read(&self, off: usize) -> u8;
    fn reg_write(&self, off: usize, val: u8);
    fn reg_read16(&self, off: usize) -> u16;
    fn fifo_read_byte(&self, ep: u8) -> u8;
    fn fifo_write_byte(&self, ep: u8, b: u8);
    fn millis(&self) -> u32;
    fn enable_usbd(&self);
    fn disable_usbd(&self);
    fn ensure_48mhz(&self);
    fn set_nvic(&self, enable: bool);
    fn mark_serial_activity(&self);
    fn fill_device_identity(&self, mac: &mut [u8; 6], crock: &mut [u8; 15]);
    fn default_product_name(&self) -> &'static str;
}
