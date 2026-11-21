//! Real-Time Clock & Calendar Trait.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtcTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub min: u8,
    pub sec: u8,
}

pub trait RtcDevice {
    fn get_time(&self) -> RtcTime;
    fn set_time(&mut self, time: &RtcTime);
    fn read_backup(&self, idx: usize) -> u32;
    fn write_backup(&mut self, idx: usize, val: u32);
}
