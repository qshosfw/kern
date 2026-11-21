//! Keyboard KeyCode Mappings and 19-Bit Bitmask Primitives.

use kernel::event::KeyCode;

/// Complete 19-key bit mapping table.
/// Each index (0..18) corresponds directly to bit `(1 << index)` in the raw and debounced bitmasks.
pub const ALL_KEYS: [KeyCode; 19] = [
    KeyCode::Side1, // Bit 0:  Col 0, Row 0 (PB15)
    KeyCode::Side2, // Bit 1:  Col 0, Row 1 (PB14)
    KeyCode::Menu,  // Bit 2:  Col 1 (PB6), Row 0 (PB15)
    KeyCode::Key1,  // Bit 3:  Col 1 (PB6), Row 1 (PB14)
    KeyCode::Key4,  // Bit 4:  Col 1 (PB6), Row 2 (PB13)
    KeyCode::Key7,  // Bit 5:  Col 1 (PB6), Row 3 (PB12)
    KeyCode::Up,    // Bit 6:  Col 2 (PB5), Row 0 (PB15)
    KeyCode::Key2,  // Bit 7:  Col 2 (PB5), Row 1 (PB14)
    KeyCode::Key5,  // Bit 8:  Col 2 (PB5), Row 2 (PB13)
    KeyCode::Key8,  // Bit 9:  Col 2 (PB5), Row 3 (PB12)
    KeyCode::Down,  // Bit 10: Col 3 (PB4), Row 0 (PB15)
    KeyCode::Key3,  // Bit 11: Col 3 (PB4), Row 1 (PB14)
    KeyCode::Key6,  // Bit 12: Col 3 (PB4), Row 2 (PB13)
    KeyCode::Key9,  // Bit 13: Col 3 (PB4), Row 3 (PB12)
    KeyCode::Exit,  // Bit 14: Col 4 (PB3), Row 0 (PB15)
    KeyCode::Star,  // Bit 15: Col 4 (PB3), Row 1 (PB14)
    KeyCode::Key0,  // Bit 16: Col 4 (PB3), Row 2 (PB13)
    KeyCode::KeyF,  // Bit 17: Col 4 (PB3), Row 3 (PB12)
    KeyCode::Ptt,   // Bit 18: PB10 dedicated pin
];

/// Convert KeyCode to its 19-bit matrix bitmask
#[inline(always)]
pub const fn key_to_bit(key: KeyCode) -> u32 {
    const SHIFTS: [u8; 20] = [
        18, 0, 1, 6, 10, 2, 14, 17, 16, 3, 7, 11, 4, 8, 12, 5, 9, 13, 15, 31,
    ];
    let k = key as usize;
    if k < 20 {
        let s = SHIFTS[k];
        if s < 31 {
            return 1 << s;
        }
    }
    0
}
