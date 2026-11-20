//! Compact flat `.rodata` tone pool and preset range LUTs.
//!
//! All built-in beep, dialog, roger, and boot jingle sequences share one contiguous
//! `NOTE_POOL` array indexed by 2-byte `(offset, len)` pairs, eliminating per-preset
//! fat-pointer slice tables and duplicate fallback arrays.

use drivers::audio::ToneNote;

// ── Contiguous Tone Pool ────────────────────────────────────────────────────

const fn n(freq_hz: u16, duration_ms: u16) -> ToneNote {
    ToneNote::new(freq_hz, duration_ms)
}

#[rustfmt::skip]
static NOTE_POOL: &[ToneNote] = &[
    // 0..1: BEEP_TONE (880 Hz, 40ms)
    n(880, 40),
    // 1..3: BEEP_CLICK (1200/15 -> 880/25)
    n(1200, 15), n(880, 25),
    // 3..6: BEEP_DOUBLE (880/25 -> 0/15 -> 1175/25)
    n(880, 25), n(0, 15), n(1175, 25),
    // 6..9: BEEP_WARNING / DIALOG_WARNING (650/40 -> 0/20 -> 650/40)
    n(650, 40), n(0, 20), n(650, 40),
    // 9..10: BEEP_SCROLL (1046/20)
    n(1046, 20),
    // 10..13: DIALOG_LOCK (1046/25 -> 0/15 -> 659/30)
    n(1046, 25), n(0, 15), n(659, 30),
    // 13..16: DIALOG_UNLOCK (659/25 -> 0/15 -> 1046/30)
    n(659, 25), n(0, 15), n(1046, 30),
    // 16..17: DIALOG_PROMPT (880/30)
    n(880, 30),
    // 17..20: DIALOG_CONFIRM (880/25 -> 0/15 -> 1175/30)
    n(880, 25), n(0, 15), n(1175, 30),
    // 20..23: DIALOG_SUCCESS / CHIME_CHARGING (880/25 -> 1109/25 -> 1318/35)
    n(880, 25), n(1109, 25), n(1318, 35),
    // 23..26: DIALOG_ERROR (440/40 -> 0/20 -> 330/50)
    n(440, 40), n(0, 20), n(330, 50),
    // 26..29: CHIME_USB_CONNECT (880/30 -> 0/15 -> 1318/45)
    n(880, 30), n(0, 15), n(1318, 45),
    // 29..32: CHIME_USB_DISCONNECT (1318/30 -> 0/15 -> 880/45)
    n(1318, 30), n(0, 15), n(880, 45),
    // 32..36: JINGLE_CHIME
    n(523, 90), n(659, 90), n(784, 90), n(1046, 280),
    // 36..38: ROGER_CLASSIC (also fallback)
    n(500, 60), n(700, 60),
    // 38..41: ROGER_TINY
    n(1250, 20), n(0, 10), n(1500, 20),
];

/// Return a slice from the core tone pool for a `(offset, len)` descriptor.
#[inline(always)]
pub fn pool_slice(range: (u8, u8)) -> &'static [ToneNote] {
    let start = range.0 as usize;
    let end = start + range.1 as usize;
    // SAFETY: All `(offset, len)` constants in this module are strictly within `NOTE_POOL`.
    unsafe { NOTE_POOL.get_unchecked(start..end) }
}

/// Compile-time `NOTE_POOL[start..end]` for static tables (range `Index` is not `const`).
#[cfg(feature = "boot_jingle")]
const fn pool_const(start: usize, end: usize) -> &'static [ToneNote] {
    NOTE_POOL.split_at(end).0.split_at(start).1
}

// ── Compact 2-Byte (offset, len) Lookup Tables ──────────────────────────────

/// Indexed by `BeepType as usize` (`0..=6`).
pub const BEEP_RANGES: [(u8, u8); 7] = [
    (0, 0), // Off
    (0, 1), // Tone
    (1, 2), // Click
    (3, 3), // Double
    (6, 3), // Warning
    (9, 1), // Scroll
    (0, 1), // Custom fallback -> Tone
];

/// Indexed by `DialogSoundType as usize` (`0..=9`).
pub const DIALOG_RANGES: [(u8, u8); 10] = [
    (10, 3), // Lock
    (13, 3), // Unlock
    (16, 1), // Prompt
    (17, 3), // Confirm
    (20, 3), // Success
    (6, 3),  // Warning (shared with BEEP_WARNING)
    (23, 3), // Error
    (20, 3), // Charging (shared with DIALOG_SUCCESS)
    (26, 3), // UsbConnect
    (29, 3), // UsbDisconnect
];

pub const RANGE_JINGLE_CHIME: (u8, u8) = (32, 4);
pub const RANGE_ROGER_CLASSIC: (u8, u8) = (36, 2);
pub const RANGE_ROGER_TINY: (u8, u8) = (38, 3);

// ── Optional Feature-Gated Boot Jingles ─────────────────────────────────────

#[cfg(feature = "boot_jingle")]
pub const JINGLE_NOKIA: &[ToneNote] = &[
    n(1318, 140),
    n(1175, 140),
    n(740, 160),
    n(830, 180),
    n(1109, 140),
    n(988, 140),
    n(587, 160),
    n(659, 180),
    n(988, 140),
    n(880, 140),
    n(554, 160),
    n(659, 160),
    n(880, 360),
];

#[cfg(feature = "boot_jingle")]
pub const JINGLE_CHIRP: &[ToneNote] = &[n(900, 45), n(0, 25), n(1500, 70)];

#[cfg(feature = "boot_jingle")]
pub const JINGLE_GAMEBOY: &[ToneNote] = &[n(1046, 25), n(0, 15), n(2093, 380)];

#[cfg(feature = "boot_jingle")]
pub struct JingleDef {
    pub file_name: &'static str,
    pub name: &'static str,
    pub notes: &'static [ToneNote],
}

#[cfg(feature = "boot_jingle")]
pub static JINGLE_DEFS: &[JingleDef] = &[
    JingleDef {
        file_name: "01_chime.ton",
        name: "Chime",
        notes: pool_const(32, 36),
    },
    JingleDef {
        file_name: "02_nokia.ton",
        name: "Nokia",
        notes: JINGLE_NOKIA,
    },
    JingleDef {
        file_name: "03_chirp.ton",
        name: "Chirp",
        notes: JINGLE_CHIRP,
    },
    JingleDef {
        file_name: "04_gameboy.ton",
        name: "GameBoy",
        notes: JINGLE_GAMEBOY,
    },
];

// ── Optional Feature-Gated Extra Rogers & Provisioning Metadata ─────────────

#[cfg(feature = "extra_rogers")]
pub const EXTRA_ROGER_NOTES: &[&[ToneNote]] = &[
    &[n(1540, 60), n(1310, 60)],
    &[n(910, 40), n(0, 20), n(910, 40), n(0, 20), n(910, 60)],
    &[
        n(2000, 60),
        n(2200, 60),
        n(2000, 60),
        n(2200, 60),
        n(2000, 60),
        n(2200, 60),
    ],
    &[
        n(1190, 60),
        n(990, 60),
        n(510, 60),
        n(1190, 60),
        n(990, 60),
        n(510, 60),
        n(1190, 60),
        n(990, 60),
        n(510, 80),
    ],
    &[n(430, 60), n(870, 60), n(1740, 60)],
];

#[cfg(feature = "provision_rogers")]
pub struct RogerDef {
    pub file_name: &'static str,
    pub name: &'static str,
    pub notes: &'static [ToneNote],
}

#[cfg(feature = "provision_rogers")]
pub static ROGER_DEFS: &[RogerDef] = &[
    RogerDef {
        file_name: "01_classic.ton",
        name: "Classic",
        notes: pool_const(36, 38),
    },
    RogerDef {
        file_name: "02_tiny.ton",
        name: "Tiny",
        notes: pool_const(38, 41),
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "03_mototrbo.ton",
        name: "MotoTRBO",
        notes: EXTRA_ROGER_NOTES[0],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "04_apx6000.ton",
        name: "APX6000",
        notes: EXTRA_ROGER_NOTES[1],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "05_t40.ton",
        name: "T40",
        notes: EXTRA_ROGER_NOTES[2],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "06_tlkrt80.ton",
        name: "TLKR T80",
        notes: EXTRA_ROGER_NOTES[3],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "07_cobra.ton",
        name: "Cobra",
        notes: EXTRA_ROGER_NOTES[4],
    },
];
