use crate::ipc::SysEvent;
use utils::mbuf::MBufToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyCode {
    Ptt,
    Side1,
    Side2,
    Up,
    Down,
    Menu,
    Exit,
    KeyF,
    Key0,
    Key1,
    Key2,
    Key3,
    Key4,
    Key5,
    Key6,
    Key7,
    Key8,
    Key9,
    Star,
    Hash,
}

impl KeyCode {
    #[inline(always)]
    pub fn to_digit(self) -> Option<u8> {
        let c = self as u8;
        if c >= Self::Key0 as u8 && c <= Self::Key9 as u8 {
            Some(c - Self::Key0 as u8)
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if val <= Self::Hash as u8 {
            Some(unsafe { core::mem::transmute(val) })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyState {
    Pressed,
    Released,
    Held,
}

impl KeyState {
    #[inline(always)]
    pub fn is_pressed(&self) -> bool {
        matches!(self, KeyState::Pressed)
    }

    #[inline(always)]
    pub fn is_released(&self) -> bool {
        matches!(self, KeyState::Released)
    }

    #[inline(always)]
    pub fn is_held(&self) -> bool {
        matches!(self, KeyState::Held)
    }
}

/// Typed USB hardware connection and bus lifecycle events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UsbEvent {
    Connected,
    Disconnected,
    Suspended,
    Resumed,
    DataAvailable,
}

/// System-level event enum, strictly typed for zero-copy routing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    // ── High-Priority Hardware Payloads ──
    PacketReceived(MBufToken),

    // ── UI & Input ──
    Key {
        code: KeyCode,
        state: KeyState,
    },
    /// Direct character / control-key input (e.g. from RigWire keyboard bridge or external HID).
    Char(char),
    Encoder(i8),

    // ── Inter-Process Communication ──
    Ipc {
        sender: u8,
        target: u8,
        payload: MBufToken,
    },
    Bus(crate::ipc::BusEvent),

    // ── System ──
    Sys(SysEvent),

    // ── Connectivity & Power ──
    Usb(UsbEvent),
    Charging(bool),

    // ── OS Scheduling ──
    Tick,
}
