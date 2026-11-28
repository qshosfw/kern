//! Portable Keypad Matrix Helpers over `kern_platform_traits::input::KeyMatrix`.

use super::keys::ALL_KEYS;
use kern_platform_traits::input::KeyMatrix;
use kernel::event::KeyCode;

/// Decode the first detected active key from a 19-bit raw matrix bitmask.
pub fn first_key_from_mask(mask: u32) -> Option<KeyCode> {
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

/// Convenience helper returning the first detected active key from a `KeyMatrix`.
#[inline(always)]
pub fn poll_matrix<M: KeyMatrix>(matrix: &M) -> Option<KeyCode> {
    first_key_from_mask(matrix.poll_raw_mask())
}
