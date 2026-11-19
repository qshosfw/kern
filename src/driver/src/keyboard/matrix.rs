//! Keypad Matrix Scanning Delegation (hardware GPIOB scan lives in `bsp::BspKeyMatrix`).

use super::keys::ALL_KEYS;
use kernel::event::KeyCode;

/// Scans the board keypad matrix and PTT pin into a 19-bit active-key bitmask.
#[inline(always)]
pub fn poll_raw_mask() -> u32 {
    bsp::BspKeyMatrix::scan_raw_mask()
}

/// Convenience helper returning the first detected active key from the hardware matrix.
pub fn poll_matrix() -> Option<KeyCode> {
    let mask = poll_raw_mask();
    if mask == 0 {
        return None;
    }
    for i in 0..19 {
        if (mask & (1 << i)) != 0 {
            return Some(ALL_KEYS[i]);
        }
    }
    None
}

/// Bare-metal fail-safe GPIO matrix reset helper for boot and panic recovery.
#[inline(always)]
pub fn reset_gpio_pins() {
    bsp::BspKeyMatrix::reset_gpio_pins();
}
