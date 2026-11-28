//! CTCSS / DCS Subaudible Tone Synthesis and Tail Elimination for BK4829 / BK4819.
//!
//! Hardware Register Architecture:
//! - `REG_07` (`REG_CXCSS_FREQ`):
//!   - Bits `[15:13]`: Mode (`0 = CTC1` main CTCSS, `1 = CTC2` 55.0 Hz tail detector, `2 = CDCSS` 134.4 Hz baud)
//!   - Bits `[12:0]`: Frequency control word `round(freq_deci_hz * 2065 / 1000)`
//! - `REG_08` (`REG_CDCSS_CODE`):
//!   - Bit `15`: `0 = low 12 bits`, `1 = high 12 bits` of 23/24-bit Golay codeword
//! - `REG_51` (`REG_CXCSS_CTRL`):
//!   - Bit `15`: Enable CxCSS, Bit `13`: Invert CDCSS polarity, Bit `12`: `1 = CTCSS / 0 = CDCSS`,
//!     Bit `11`: 24-bit mode, Bits `[6:0]`: Subaudible TX deviation gain (`0x4A` CTCSS, `0x33` CDCSS)
//! - `REG_52` (`REG_CXCSS_TAIL`):
//!   - Bit `15`: Enable tail shift, Bits `[14:13]`: `0 = 134.4Hz CDCSS tail`, `1 = 120°`, `2 = 180°`, `3 = 240°`

/// Calculate BK4819/BK4829 `REG_07` CTC1 frequency control word from deci-Hz (`0.1 Hz` units).
/// Uses single-cycle 32-bit Q16 multiply `round(deci_hz * 2.065)` without u64/division.
#[inline(always)]
pub const fn ctcss_deci_hz_to_reg(deci_hz: u16) -> u16 {
    (((deci_hz as u32 * 135_332) + 32_768) >> 16) as u16 & 0x1FFF
}

/// Convert raw `REG_68` CTCSS scan word into deci-Hz (`0.1 Hz` units).
/// Uses BK4829 bench-verified `20.380 counts/Hz` Q16 reciprocal `32_157` (`1804 -> 885` = `88.5 Hz`).
#[inline(always)]
pub const fn reg_to_ctcss_deci_hz(reg68: u16) -> u16 {
    ((((reg68 & 0x1FFF) as u32) * 32_157 + 32_768) >> 16) as u16
}

/// 55.0 Hz CTC1 frequency control word (`1136`) for transmitting a 55 Hz STE tail burst.
pub const CSS_TAIL_55HZ_REG: u16 = ctcss_deci_hz_to_reg(550);

/// `REG_07` Mode 1 (`CTC2`) 55.0 Hz tail tone detector configuration (`(1 << 13) | 1136`).
pub const CTC2_55HZ_DETECT_REG: u16 = (1 << 13) | CSS_TAIL_55HZ_REG;

/// `REG_07` Mode 2 (`CDCSS`) 134.4 Hz baud rate generator word (`(2 << 13) | 2775`).
pub const CDCSS_134_4HZ_BAUD_REG: u16 = (2 << 13) | 2775;

/// `REG_51` configuration for active CTCSS (`0x904A`: enable CxCSS, CTCSS mode, gain 74).
pub const CXCSS_CTRL_CTCSS: u16 = 0x904A;

/// `REG_51` configuration for normal 23-bit Golay CDCSS (`0xA033`: Bit 11 = 0 23-bit mode,
/// Bit 13 = 1 silicon-verified positive on-air D023N polarity, gain 51).
pub const CXCSS_CTRL_CDCSS_NORM: u16 = 0xA033;

/// `REG_51` configuration for inverted 23-bit Golay CDCSS (`0x8033`: Bit 11 = 0 23-bit mode,
/// Bit 13 = 0 silicon-verified inverted on-air D023I polarity, gain 51).
pub const CXCSS_CTRL_CDCSS_INV: u16 = 0x8033;

/// `REG_52` default threshold configuration without active TX phase shift (`0x028F`).
pub const CXCSS_TAIL_NORMAL: u16 = 0x028F;

/// `REG_52` 134.4 Hz CDCSS tail shift (`0x828F`).
pub const CXCSS_TAIL_CDCSS_134HZ: u16 = 0x828F;

/// `REG_52` 120-degree CTCSS reverse-burst phase shift (`0xA28F`).
pub const CXCSS_TAIL_SHIFT_120: u16 = 0xA28F;

/// `REG_52` 180-degree CTCSS reverse-burst phase shift (`0xC28F`).
pub const CXCSS_TAIL_SHIFT_180: u16 = 0xC28F;

/// `REG_52` 240-degree CTCSS reverse-burst phase shift (`0xE28F`).
pub const CXCSS_TAIL_SHIFT_240: u16 = 0xE28F;
