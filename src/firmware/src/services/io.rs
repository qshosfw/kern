pub use kernel::service::UartService;

#[inline(always)]
pub fn mark_serial_activity() {
    py32_hal::uart::mark_activity();
}

#[inline(always)]
pub fn is_serial_active() -> bool {
    py32_hal::uart::is_active(800)
}
