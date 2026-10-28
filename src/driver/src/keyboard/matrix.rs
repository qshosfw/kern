//! Low-Level GPIO Keypad Matrix Scanning & Hardware Settling Primitives.

use super::keys::ALL_KEYS;
use kernel::event::KeyCode;

// GPIOB memory-mapped register offsets for atomic single-cycle register access
const GPIOB_BASE: u32 = 0x5000_0400;
const BSRR_OFFSET: u32 = 0x18;
const IDR_OFFSET: u32 = 0x10;

// PB6, PB5, PB4, PB3
const PIN_MASK_COLS: u32 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3);
// PB15, PB14, PB13, PB12
const PIN_MASK_ROWS: u32 = (1 << 15) | (1 << 14) | (1 << 13) | (1 << 12);

#[inline(always)]
fn set_cols_high() {
    let bsrr = (GPIOB_BASE + BSRR_OFFSET) as *mut u32;
    unsafe {
        bsrr.write_volatile(PIN_MASK_COLS);
    }
}

#[inline(always)]
fn clear_col(col_idx: usize) {
    let pin = 6 - col_idx;
    let bsrr = (GPIOB_BASE + BSRR_OFFSET) as *mut u32;
    unsafe {
        bsrr.write_volatile(1 << (pin + 16));
    }
}

#[inline(always)]
fn read_rows_raw() -> u32 {
    let idr = (GPIOB_BASE + IDR_OFFSET) as *const u32;
    unsafe { idr.read_volatile() & PIN_MASK_ROWS }
}

#[inline(always)]
fn read_ptt_raw() -> bool {
    let idr = (GPIOB_BASE + IDR_OFFSET) as *const u32;
    unsafe { (idr.read_volatile() & (1 << 10)) == 0 }
}

/// Scans the complete 5-column matrix and dedicated PTT pin into a 19-bit bitmask.
pub fn poll_raw_mask() -> u32 {
    let mut mask: u32 = 0;

    let mut side_keys = 0u32;
    for j in 0..5 {
        set_cols_high();
        if j > 0 {
            clear_col(j - 1);
        }

        // Line settling delay for GPIO output driver and trace capacitance (2µs @ 48MHz)
        cortex_m::asm::delay(96);

        let mut reg = 0u32;
        let mut i = 0usize;
        let mut k = 0usize;
        while i < 3 && k < 12 {
            cortex_m::asm::delay(48); // 1 µs intra-sample delay @ 48MHz
            let reg2 = read_rows_raw();
            if reg == reg2 {
                i += 1;
            } else {
                i = 0;
                reg = reg2;
            }
            k += 1;
        }

        if i >= 2 {
            let row_bits = (!reg >> 12) & 0x0F;
            let rev = ((row_bits & 1) << 3)
                | ((row_bits & 2) << 1)
                | ((row_bits & 4) >> 1)
                | ((row_bits & 8) >> 3);
            if j == 0 {
                side_keys = (rev & 0x03) as u32;
                mask |= side_keys;
            } else {
                // If Side1 is pressed (PB15 grounded), mask out Row 0 (bit 0 of rev)
                // If Side2 is pressed (PB14 grounded), mask out Row 1 (bit 1 of rev)
                let mut col_rev = rev as u32;
                if (side_keys & 1) != 0 {
                    col_rev &= !1;
                }
                if (side_keys & 2) != 0 {
                    col_rev &= !2;
                }
                mask |= col_rev << (4 * j - 2);
            }
        }
    }

    set_cols_high();

    if read_ptt_raw() {
        mask |= 1 << 18; // PTT
    }

    mask
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
pub fn reset_gpio_pins() {
    use bsp::pins::{KB_COLS, KB_ROWS, PTT};
    use py32_hal::pac::gpio;

    for col in &KB_COLS {
        col.configure_output(gpio::SPEED_HIGH);
        col.set_high();
    }
    for row in &KB_ROWS {
        row.configure_input(gpio::PULL_UP);
    }
    PTT.configure_input(gpio::PULL_UP);
}
