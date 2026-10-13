//! Built-in const tone sequences, audio presets, and roger beep definitions.
//! Stored in `.rodata` (flash) for zero stack allocation and zero RAM overhead.

use drivers::audio::ToneNote;

// ── Keypress Beep Tone Sequences ──────────────────────────────────────────

/// Warm musical keypress pip — 700 Hz for 45ms.
pub const BEEP_TONE: &[ToneNote] = &[ToneNote::new(700, 45)];

/// Tactile mechanical switch impulse — sharp leading snap into warm body.
/// 750 Hz (15ms) → 450 Hz (15ms). Total: 30ms.
pub const BEEP_CLICK: &[ToneNote] = &[ToneNote::new(750, 15), ToneNote::new(450, 15)];

/// Friendly ascending confirmation pair.
/// 650 Hz (20ms) → pause (10ms) → 950 Hz (25ms). Total: 55ms.
pub const BEEP_DOUBLE: &[ToneNote] = &[
    ToneNote::new(650, 20),
    ToneNote::new(0, 10),
    ToneNote::new(950, 25),
];

/// DeltaFW classic warning beep: 500 Hz (60ms) → pause (20ms) → 500 Hz (60ms). Total: 140ms.
pub const BEEP_WARNING: &[ToneNote] = &[
    ToneNote::new(500, 60),
    ToneNote::new(0, 20),
    ToneNote::new(500, 60),
];

/// DeltaFW scrolling key tone: fast subtle pip for continuous scrolling / rotary navigation (400 Hz for 25ms).
pub const BEEP_SCROLL: &[ToneNote] = &[ToneNote::new(400, 25)];

// ── UI Dialog / Notification Cues ─────────────────────────────────────────

/// Lock: Nokia descending double click — 850 Hz (20ms) → pause → 500 Hz (25ms)
pub const DIALOG_LOCK: &[ToneNote] = &[
    ToneNote::new(850, 20),
    ToneNote::new(0, 10),
    ToneNote::new(500, 25),
];

/// Unlock: Nokia ascending double chime — 500 Hz (20ms) → pause → 850 Hz (25ms)
pub const DIALOG_UNLOCK: &[ToneNote] = &[
    ToneNote::new(500, 20),
    ToneNote::new(0, 10),
    ToneNote::new(850, 25),
];

/// Prompt: subtle gentle pip — single 700 Hz (20ms)
pub const DIALOG_PROMPT: &[ToneNote] = &[ToneNote::new(700, 20)];

/// Confirm: inquiring double pip — 750 Hz (30ms) → pause → 950 Hz (35ms)
pub const DIALOG_CONFIRM: &[ToneNote] = &[
    ToneNote::new(750, 30),
    ToneNote::new(0, 15),
    ToneNote::new(950, 35),
];

/// Success: bright ascending major triad — 650 → 820 → 1050 Hz
pub const DIALOG_SUCCESS: &[ToneNote] = &[
    ToneNote::new(650, 25),
    ToneNote::new(820, 25),
    ToneNote::new(1050, 40),
];

/// Warning: DeltaFW warning beep
pub const DIALOG_WARNING: &[ToneNote] = BEEP_WARNING;

/// Error: heavy descending alert — 320 Hz (45ms) → pause → 240 Hz (55ms)
pub const DIALOG_ERROR: &[ToneNote] = &[
    ToneNote::new(320, 45),
    ToneNote::new(0, 15),
    ToneNote::new(240, 55),
];

/// Charging: warm ascending major triad power chime — 650 → 820 → 1050 Hz (total 90ms)
pub const CHIME_CHARGING: &[ToneNote] = DIALOG_SUCCESS;

/// USB / Serial connection: crisp rising digital handshake — 750 Hz (25ms) → pause (10ms) → 1150 Hz (35ms)
pub const CHIME_USB_CONNECT: &[ToneNote] = &[
    ToneNote::new(750, 25),
    ToneNote::new(0, 10),
    ToneNote::new(1150, 35),
];

/// USB disconnection: soft descending tone — 1050 Hz (20ms) → pause (10ms) → 700 Hz (25ms)
pub const CHIME_USB_DISCONNECT: &[ToneNote] = &[
    ToneNote::new(1050, 20),
    ToneNote::new(0, 10),
    ToneNote::new(700, 25),
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

// ── Built-in Roger Beep Presets ───────────────────────────────────────────

pub const ROGER_CLASSIC_FALLBACK: &[ToneNote] = &[ToneNote::new(500, 60), ToneNote::new(700, 60)];

#[cfg(feature = "filesystem")]
pub struct RogerDef {
    pub file_name: &'static str,
    pub name: &'static str,
    pub notes: &'static [(u16, u16)],
}

#[cfg(feature = "filesystem")]
pub static ROGER_DEFS: &[RogerDef] = &[
    RogerDef {
        file_name: "01_classic.ton",
        name: "Classic",
        notes: &[(500, 60), (700, 60)],
    },
    RogerDef {
        file_name: "02_tiny.ton",
        name: "Tiny",
        notes: &[(1250, 20), (0, 10), (1500, 20)],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "03_mototrbo.ton",
        name: "MotoTRBO",
        notes: &[(1540, 60), (1310, 60)],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "04_apx6000.ton",
        name: "APX6000",
        notes: &[(910, 40), (0, 20), (910, 40), (0, 20), (910, 60)],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "05_t40.ton",
        name: "T40",
        notes: &[
            (2000, 60),
            (2200, 60),
            (2000, 60),
            (2200, 60),
            (2000, 60),
            (2200, 60),
        ],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "06_tlkrt80.ton",
        name: "TLKR T80",
        notes: &[
            (1190, 60),
            (990, 60),
            (510, 60),
            (1190, 60),
            (990, 60),
            (510, 60),
            (1190, 60),
            (990, 60),
            (510, 80),
        ],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "07_cobra.ton",
        name: "Cobra",
        notes: &[(430, 60), (870, 60), (1740, 60)],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "08_police.ton",
        name: "Police",
        notes: &[
            (800, 60),
            (2200, 60),
            (890, 60),
            (1540, 60),
            (1300, 60),
            (970, 60),
            (1180, 60),
            (1070, 60),
        ],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "09_uv5rc.ton",
        name: "UV-5RC",
        notes: &[(1120, 140), (860, 200)],
    },
    #[cfg(feature = "extra_rogers")]
    RogerDef {
        file_name: "10_quindar.ton",
        name: "Quindar",
        notes: &[(2475, 250)],
    },
    #[cfg(all(feature = "extra_rogers", feature = "mdc1200"))]
    RogerDef {
        file_name: "11_mdc1200.ton",
        name: "MDC1200",
        notes: &[
            (1800, 10),
            (1200, 10),
            (1800, 10),
            (1200, 10),
            (1800, 10),
            (1800, 10),
            (1200, 10),
            (1800, 10),
            (1200, 10),
            (1800, 10),
            (1800, 10),
            (1200, 10),
            (1800, 10),
            (1200, 10),
            (1800, 10),
            (1800, 10),
            (1200, 10),
            (1800, 10),
        ],
    },
];
