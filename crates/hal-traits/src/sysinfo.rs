//! System Hardware Identification Trait.

pub trait SystemInfoDevice {
    fn uid_12(&self) -> [u8; 12];
    fn uid_16(&self) -> [u8; 16];
    fn uid_256(&self) -> [u8; 256];
    fn flash_size_kb(&self) -> u16;
    fn vrefint_calibration(&self) -> u16;
    fn tsensor_calibration(&self) -> u16;
}
