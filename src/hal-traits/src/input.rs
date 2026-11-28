//! Input device abstraction.

/// Key codes for the Quansheng keypad.
/// Defined here so the kernel and drivers share a single enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyCode {
    Key0 = 0,
    Key1,
    Key2,
    Key3,
    Key4,
    Key5,
    Key6,
    Key7,
    Key8,
    Key9,
    Menu,
    Exit,
    Up,
    Down,
    Star,
    Hash,
    Ptt,
    Side1,
    Side2,
    FKey,
}

/// Key press states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
    Held,
    /// Long press (held beyond threshold).
    LongPress,
}

/// A keypad / input scanner.
pub trait InputBackend {
    /// Scan the input matrix and return an event if a key state changed.
    fn scan(&mut self) -> Option<(KeyCode, KeyState)>;
}

/// Low-level board keypad matrix scanner returning a 19-bit active-key bitmask.
pub trait KeyMatrix {
    fn poll_raw_mask(&self) -> u32;
    fn reset_pins(&self);
    fn now_ms(&self) -> u32;
}
