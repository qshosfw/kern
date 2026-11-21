//! Sunk-key latch: after a modal or app transition consumes a press, its trailing
//! `Held`/`Released` events must not leak into the newly focused view.

use kernel::event::{Event, KeyCode, KeyState};

pub static mut SUNK_KEYS: u32 = 0;

pub const BIT_MENU: u32 = 1 << 2;
pub const BIT_KEY5: u32 = 1 << 8;
pub const BIT_EXIT: u32 = 1 << 14;

pub const TAG_FAULT_ALERT: u8 = 0xFE;
pub const TAG_FAULT_CONFIRM: u8 = 0xFF;

pub use drivers::keyboard::key_to_bit;

pub fn sink_key(key: KeyCode) {
    let bit = key_to_bit(key);
    unsafe {
        SUNK_KEYS |= bit;
    }
}

pub fn sink_modal_keys() {
    unsafe {
        SUNK_KEYS |= BIT_MENU | BIT_KEY5 | BIT_EXIT;
    }
}

pub fn sink_enter_keys() {
    unsafe {
        SUNK_KEYS |= BIT_MENU | BIT_KEY5;
    }
}

pub fn sink_exit_key() {
    unsafe {
        SUNK_KEYS |= BIT_EXIT;
    }
}

pub fn clear_sunk_keys() {
    unsafe {
        SUNK_KEYS = 0;
    }
}

pub(super) fn filter_sunk_keys(event: &Event) -> bool {
    if let Event::Key { code, state } = *event {
        let bit = key_to_bit(code);
        if (unsafe { SUNK_KEYS } & bit) != 0 {
            if state == KeyState::Released {
                unsafe {
                    SUNK_KEYS &= !bit;
                }
                return true;
            } else if state == KeyState::Held {
                return true;
            } else {
                // KeyState::Pressed: A new press arrives, clear the stale sunk bit and allow it through!
                unsafe {
                    SUNK_KEYS &= !bit;
                }
                return false;
            }
        }
    }
    false
}
