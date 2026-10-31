//! BK4829 / BK4819 Complete Hardware Register Definitions & Bitfields.

// Control & Power Registers
pub const REG_CHIP_ID: u8 = 0x00;
pub const REG_STATUS_LIVE: u8 = 0x0B;
pub const REG_STATUS: u8 = 0x0C;
pub const REG_SYS_POWER: u8 = 0x30;
pub const REG_GPIO: u8 = 0x33;
pub const REG_XTAL_TRIM: u8 = 0x37;
pub const REG_PLL_LOW: u8 = 0x38;
pub const REG_PLL_HIGH: u8 = 0x39;
pub const REG_INT_ENABLE: u8 = 0x3E;
pub const REG_INT_CLEAR: u8 = 0x3F;

pub const REG_AGC_TABLE_MIN: u8 = 0x14; // RX AGC Gain Table[-1]
pub const REG_FILTERS: u8 = 0x2B; // AF Tx/Rx filter control (bit 0 preemp, bit 8 deemp)
pub const REG_IF_COEFF: u8 = 0x3D; // IF filter coefficient (0x2AAB normal, 0x0000 SSB)

// Modulation & DSP Configuration
pub const REG_MODULATION: u8 = 0x40;
pub const REG_FILTER_BW: u8 = 0x43;
pub const REG_MIC_GAIN: u8 = 0x44;
pub const REG_AF_ROUTING: u8 = 0x47;
pub const REG_AF_LEVEL: u8 = 0x48;
pub const REG_AF_RESPONSE: u8 = 0x48; // backward compat alias
pub const REG_AGC_TARGET: u8 = 0x49; // AGC target & timing parameters

// Subaudible CTCSS / DCS Registers
pub const REG_CTCSS_ENABLE: u8 = 0x4B;
pub const REG_CTCSS_RX_FREQ: u8 = 0x4C;
pub const REG_CTCSS_TX_FREQ: u8 = 0x4D;
pub const REG_DCS_ENABLE: u8 = 0x51;
pub const REG_DCS_RX_CODE: u8 = 0x52;
pub const REG_DCS_TX_CODE: u8 = 0x53;

// Audio Path & TX Mute Registers
pub const REG_TX_MUTE: u8 = 0x50; // 0x3B18 = unmute / ExitTxMute, 0xBB18 = mute / EnterTxMute

// Digital Packet & FSK Modem Registers
pub const REG_FSK_CTRL: u8 = 0x58;
pub const REG_FSK_FIFO_CTRL: u8 = 0x59;
pub const REG_FSK_SYNC_0_1: u8 = 0x5A;
pub const REG_FSK_SYNC_2_3: u8 = 0x5B;
pub const REG_FSK_CRC: u8 = 0x5C;
pub const REG_FSK_PACKET_LEN: u8 = 0x5D;
pub const REG_FSK_FIFO_STATUS: u8 = 0x5E;
pub const REG_FSK_FIFO_DATA: u8 = 0x5F;

// Squelch Threshold & Telemetry Registers
pub const REG_SQL_GLITCH_CLOSE: u8 = 0x4D;
pub const REG_SQL_GLITCH_OPEN: u8 = 0x4E;
pub const REG_SQL_NOISE: u8 = 0x4F;
pub const REG_GLITCH_VAL: u8 = 0x63;
pub const REG_AUDIO_AMP: u8 = 0x64; // TX audio amplitude indicator
pub const REG_NOISE_VAL: u8 = 0x65; // Ex-noise indicator
pub const REG_SNR_VAL: u8 = 0x66;
pub const REG_RSSI_VAL: u8 = 0x67;
pub const REG_SQL_RSSI: u8 = 0x78;

// Tone Generator Registers
pub const REG_TONE_GAIN: u8 = 0x70;
pub const REG_TONE1_GAIN: u8 = 0x70; // alias
pub const REG_TONE1_FREQ: u8 = 0x71;
pub const REG_TONE2_FREQ: u8 = 0x72;
pub const REG_FSK_BAUD_DIV: u8 = 0x72; // alias
pub const REG_AFC: u8 = 0x73; // Automatic Frequency Correction (bit 4 afc_disable)
pub const REG_AGC_CONFIG: u8 = 0x7E; // AGC and DC filter bandwidth (bit 15 fix mode)

// Status & Interrupt Bits in Reg 0x0C and 0x3F
pub const INT_FSK_TX_FINISHED: u16 = 1 << 15;
pub const INT_FSK_RX_FIFO_EMPTY: u16 = 1 << 14;
pub const INT_FSK_RX_FINISHED: u16 = 1 << 13;
pub const INT_FSK_FIFO_ALMOST_FULL: u16 = 1 << 12;
pub const INT_FSK_RX_SYNC: u16 = 1 << 11;
pub const INT_SQUELCH_LOST: u16 = 1 << 1;
pub const INT_SQUELCH_FOUND: u16 = 1 << 0;
