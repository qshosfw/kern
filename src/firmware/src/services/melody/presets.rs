//! Built-in const tone sequences, audio presets, and roger beep definitions.
//! Stored in `.rodata` (flash) for zero stack allocation and zero RAM overhead.

use drivers::audio::ToneNote;

// ── Keypress Beep Tone Sequences ──────────────────────────────────────────

/// Warm musical keypress pip — Concert A (880 Hz) for 40ms.
pub const BEEP_TONE: &[ToneNote] = &[ToneNote::new(880, 40)];

/// Tactile mechanical switch impulse — sharp transient snap into resonant body.
/// 1200 Hz (15ms) → 880 Hz (25ms). Total: 40ms.
pub const BEEP_CLICK: &[ToneNote] = &[ToneNote::new(1200, 15), ToneNote::new(880, 25)];

/// Friendly ascending confirmation pair — snappy major third.
/// 880 Hz (25ms) → pause (15ms) → 1175 Hz (25ms). Total: 65ms.
pub const BEEP_DOUBLE: &[ToneNote] = &[
    ToneNote::new(880, 25),
    ToneNote::new(0, 15),
    ToneNote::new(1175, 25),
];

/// Classic assertive dual warning tone: 650 Hz (40ms) → pause (20ms) → 650 Hz (40ms). Total: 100ms.
pub const BEEP_WARNING: &[ToneNote] = &[
    ToneNote::new(650, 40),
    ToneNote::new(0, 20),
    ToneNote::new(650, 40),
];

/// Subtle rotary dial tick: clean crisp pip (1046 Hz / C6 for 35ms) for continuous scrolling.
pub const BEEP_SCROLL: &[ToneNote] = &[ToneNote::new(1046, 35)];

// ── UI Dialog / Notification Cues ─────────────────────────────────────────

/// Lock: descending double click — 1046 Hz (25ms) → pause (15ms) → 659 Hz (30ms)
pub const DIALOG_LOCK: &[ToneNote] = &[
    ToneNote::new(1046, 25),
    ToneNote::new(0, 15),
    ToneNote::new(659, 30),
];

/// Unlock: ascending double chime — 659 Hz (25ms) → pause (15ms) → 1046 Hz (30ms)
pub const DIALOG_UNLOCK: &[ToneNote] = &[
    ToneNote::new(659, 25),
    ToneNote::new(0, 15),
    ToneNote::new(1046, 30),
];

/// Prompt: subtle gentle reminder pip — 880 Hz (30ms)
pub const DIALOG_PROMPT: &[ToneNote] = &[ToneNote::new(880, 30)];

/// Confirm: inquiring double pip — 880 Hz (25ms) → pause (15ms) → 1175 Hz (30ms)
pub const DIALOG_CONFIRM: &[ToneNote] = &[
    ToneNote::new(880, 25),
    ToneNote::new(0, 15),
    ToneNote::new(1175, 30),
];

/// Success: bright ascending major triad — 880 Hz (25ms) → 1109 Hz (25ms) → 1318 Hz (35ms)
pub const DIALOG_SUCCESS: &[ToneNote] = &[
    ToneNote::new(880, 25),
    ToneNote::new(1109, 25),
    ToneNote::new(1318, 35),
];

/// Warning: classic dual warning tone
pub const DIALOG_WARNING: &[ToneNote] = BEEP_WARNING;

/// Error: heavy descending alert — 440 Hz (40ms) → pause (20ms) → 330 Hz (50ms)
pub const DIALOG_ERROR: &[ToneNote] = &[
    ToneNote::new(440, 40),
    ToneNote::new(0, 20),
    ToneNote::new(330, 50),
];

/// Charging: warm ascending major triad power chime
pub const CHIME_CHARGING: &[ToneNote] = DIALOG_SUCCESS;

/// USB / Serial connection: crisp rising digital chime — 880 Hz (30ms) → pause (15ms) → 1318 Hz (45ms)
pub const CHIME_USB_CONNECT: &[ToneNote] = &[
    ToneNote::new(880, 30),
    ToneNote::new(0, 15),
    ToneNote::new(1318, 45),
];

/// USB disconnection: gentle falling tone — 1318 Hz (30ms) → pause (15ms) → 880 Hz (45ms)
pub const CHIME_USB_DISCONNECT: &[ToneNote] = &[
    ToneNote::new(1318, 30),
    ToneNote::new(0, 15),
    ToneNote::new(880, 45),
];

// ── Power-On Boot Jingles ─────────────────────────────────────────────────

pub const JINGLE_CHIME: &[ToneNote] = &[
    ToneNote::new(523, 90),
    ToneNote::new(659, 90),
    ToneNote::new(784, 90),
    ToneNote::new(1046, 280),
];

pub const JINGLE_NOKIA: &[ToneNote] = &[
    ToneNote::new(1318, 140),
    ToneNote::new(1175, 140),
    ToneNote::new(740, 160),
    ToneNote::new(830, 180),
    ToneNote::new(1109, 140),
    ToneNote::new(988, 140),
    ToneNote::new(587, 160),
    ToneNote::new(659, 180),
    ToneNote::new(988, 140),
    ToneNote::new(880, 140),
    ToneNote::new(554, 160),
    ToneNote::new(659, 160),
    ToneNote::new(880, 360),
];

pub const JINGLE_CHIRP: &[ToneNote] = &[
    ToneNote::new(900, 45),
    ToneNote::new(0, 25),
    ToneNote::new(1500, 70),
];

pub const JINGLE_GAMEBOY: &[ToneNote] = &[
    ToneNote::new(1046, 25),
    ToneNote::new(0, 15),
    ToneNote::new(2093, 380),
];

pub struct JingleDef {
    pub file_name: &'static str,
    pub name: &'static str,
    pub notes: &'static [ToneNote],
}

pub static JINGLE_DEFS: &[JingleDef] = &[
    JingleDef {
        file_name: "01_chime.ton",
        name: "Chime",
        notes: JINGLE_CHIME,
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

// ── Built-in Roger Beep Presets ───────────────────────────────────────────

pub const ROGER_CLASSIC_FALLBACK: &[ToneNote] = &[ToneNote::new(500, 60), ToneNote::new(700, 60)];

pub struct RogerDef {
    pub file_name: &'static str,
    pub name: &'static str,
    pub notes: &'static [ToneNote],
}

pub static ROGER_DEFS: &[RogerDef] = &[
    RogerDef {
        file_name: "01_classic.ton",
        name: "Classic",
        notes: &[ToneNote::new(500, 60), ToneNote::new(700, 60)],
    },
    RogerDef {
        file_name: "02_tiny.ton",
        name: "Tiny",
        notes: &[
            ToneNote::new(1250, 20),
            ToneNote::new(0, 10),
            ToneNote::new(1500, 20),
        ],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "03_mototrbo.ton",
        name: "MotoTRBO",
        notes: &[ToneNote::new(1540, 60), ToneNote::new(1310, 60)],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "04_apx6000.ton",
        name: "APX6000",
        notes: &[
            ToneNote::new(910, 40),
            ToneNote::new(0, 20),
            ToneNote::new(910, 40),
            ToneNote::new(0, 20),
            ToneNote::new(910, 60),
        ],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "05_t40.ton",
        name: "T40",
        notes: &[
            ToneNote::new(2000, 60),
            ToneNote::new(2200, 60),
            ToneNote::new(2000, 60),
            ToneNote::new(2200, 60),
            ToneNote::new(2000, 60),
            ToneNote::new(2200, 60),
        ],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "06_tlkrt80.ton",
        name: "TLKR T80",
        notes: &[
            ToneNote::new(1190, 60),
            ToneNote::new(990, 60),
            ToneNote::new(510, 60),
            ToneNote::new(1190, 60),
            ToneNote::new(990, 60),
            ToneNote::new(510, 60),
            ToneNote::new(1190, 60),
            ToneNote::new(990, 60),
            ToneNote::new(510, 80),
        ],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "07_cobra.ton",
        name: "Cobra",
        notes: &[
            ToneNote::new(430, 60),
            ToneNote::new(870, 60),
            ToneNote::new(1740, 60),
        ],
    },
];

pub static ROGER_PRESET_OPTIONS: [&'static str; 8] = [
    "Off", "Classic", "Tiny", "MotoTRBO", "APX6000", "T40", "TLKR T80", "Cobra",
];
