//! Digital Mode Audio / Frequency Shift Tone Generators (FT8, WSPR).
//!
//! Tone profiles and symbol timing for narrow-band weak signal digital modes.

/// FT8 Digital Mode Tone Parameters (8-GFSK, 160ms symbol time, 6.25 Hz tone spacing).
pub struct Ft8Mod;

impl Ft8Mod {
    pub const SYMBOL_DURATION_MS: u16 = 160;
    pub const TONE_SPACING_MHZ: u32 = 6250; // 6.25 Hz = 6250 mHz

    /// Returns the tone frequency offset from base carrier in milli-Hertz for tone index (0..7).
    #[inline(always)]
    pub const fn tone_offset_mhz(tone: u8) -> i32 {
        (tone as i32) * 6250
    }
}

/// WSPR Digital Mode Tone Parameters (4-FSK, 683ms symbol time, 1.4648 Hz tone spacing).
pub struct WsprMod;

impl WsprMod {
    pub const SYMBOL_DURATION_MS: u16 = 683;
    pub const TONE_SPACING_MHZ: u32 = 1465; // ~1.4648 Hz = 1465 mHz

    /// Returns the tone frequency offset from base carrier in milli-Hertz for tone index (0..3).
    #[inline(always)]
    pub const fn tone_offset_mhz(tone: u8) -> i32 {
        (tone as i32) * 1465
    }
}
