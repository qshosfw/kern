//! Firmware Radio Subsystem Integration, VFO Channel State, and Audio Feedback.

pub mod coordinator;
pub mod dwell;
pub mod fsk_stream;

pub use coordinator::*;
pub use dwell::DwellController;
pub use fsk_stream::*;

/// Lock-free 32-byte live RF telemetry snapshot published every 10 ms tick.
pub static RADIO_VIEW: kernel::service::radio::SeqLock<kernel::service::radio::RadioView> =
    kernel::service::radio::SeqLock::new(kernel::service::radio::RadioView::empty());

#[inline(always)]
pub fn radio_view() -> kernel::service::radio::RadioView {
    RADIO_VIEW.read()
}

/// System RF state holding live RSSI telemetry, the Multi-VFO `RadioCoordinator`,
/// and the non-blocking `ToneSequencer`.
#[derive(Debug, Clone)]
pub struct RadioState {
    pub rssi_dbm: i16,
    pub coord: RadioCoordinator,
    pub tone_seq: crate::services::melody::ToneSequencer,
}

impl RadioState {
    pub const fn new() -> Self {
        Self {
            rssi_dbm: -120,
            coord: RadioCoordinator::new(),
            tone_seq: crate::services::melody::ToneSequencer::new(),
        }
    }

    pub unsafe fn init_in_place(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
            let s = &mut *ptr;
            s.rssi_dbm = -120;
            s.coord.init_zeroed();
        }
    }
}
