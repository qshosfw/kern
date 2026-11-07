//! BK4829 / BK4819 Complete Hardware Register Definitions & Bitfields.

// Control, Synthesizer & Power Registers
pub const REG_CHIP_ID: u8 = 0x00;
pub const REG_INT_STATUS: u8 = 0x02; // Interrupt Status / Vector (read status, write 0 to clear IRQ)
pub const REG_CXCSS_FREQ: u8 = 0x07; // CTC1 / CTC2 (55Hz) / CDCSS (134.4Hz) frequency word
pub const REG_CDCSS_CODE: u8 = 0x08; // CDCSS 23/24-bit Golay codeword (bit 15 selects high/low 12 bits)
pub const REG_DTMF_COEFF: u8 = 0x09; // DTMF / 5-Tone detection filter coefficient
pub const REG_GPIO_IN: u8 = 0x0A; // GPIO input indicator
pub const REG_STATUS_LIVE: u8 = 0x0B; // DTMF/FSK live status
pub const REG_STATUS: u8 = 0x0C; // Main receiver status (bit 0 IRQ, bit 1 SQL, bit 10 CTC1, bit 11 CTC2, bits 13:12 phase shift, bits 15:14 DCS)

// AGC Gain Table Registers
pub const REG_AGC_TABLE_0: u8 = 0x10;
pub const REG_AGC_TABLE_1: u8 = 0x11;
pub const REG_AGC_TABLE_2: u8 = 0x12;
pub const REG_AGC_TABLE_3: u8 = 0x13;
pub const REG_AGC_TABLE_MIN: u8 = 0x14; // RX AGC Gain Table[-1]

pub const REG_FILTERS: u8 = 0x2B; // AF Tx/Rx filter control (bit 0 preemp, bit 8 deemp)
pub const REG_SYS_POWER: u8 = 0x30; // System blocks enable (0xBFF1 RX, 0xC3FE TX, 0xFFF7 Internal TX-to-RX Loopback)
pub const REG_SCAN_CTRL: u8 = 0x32; // Hardware frequency/tone scan control
pub const REG_GPIO: u8 = 0x33; // GPIO output control
pub const REG_PA_CTRL: u8 = 0x36; // PA bias and gain stages
pub const REG_POWER_CTRL: u8 = 0x37; // Bandgap, XTAL enable, RF LDO voltage (bit 9: 1=2.7V RX, 0=2.4V TX)
pub const REG_PLL_LOW: u8 = 0x38; // Frequency low 16 bits (in 10 Hz units)
pub const REG_PLL_HIGH: u8 = 0x39; // Frequency high 16 bits (in 10 Hz units)
pub const REG_XTAL_FREQ_LOW: u8 = 0x3B; // Crystal reference frequency low 16 bits (baseline 22656 for 26 MHz)
pub const REG_IF_COEFF: u8 = 0x3D; // IF filter coefficient (0x2AAB normal, 0x0000 wide/SSB)
pub const REG_INT_ENABLE: u8 = 0x3F; // Interrupt Enable Mask Register

// Modulation & DSP Configuration
pub const REG_MODULATION: u8 = 0x40;
pub const REG_FILTER_BW: u8 = 0x43;
pub const REG_MIC_GAIN: u8 = 0x44;
pub const REG_AF_ROUTING: u8 = 0x47;
pub const REG_AF_LEVEL: u8 = 0x48;
pub const REG_AGC_TARGET: u8 = 0x49; // AGC target & timing parameters
pub const REG_ALC_CTRL: u8 = 0x4B; // Audio ALC control

// Squelch Threshold Registers
pub const REG_SQL_GLITCH_CLOSE: u8 = 0x4D;
pub const REG_SQL_GLITCH_OPEN: u8 = 0x4E;
pub const REG_SQL_NOISE: u8 = 0x4F;

// Audio Path & Subaudible Control Registers
pub const REG_TX_MUTE: u8 = 0x50; // 0x3B18 = unmute / ExitTxMute, 0xBB18 = mute / EnterTxMute
pub const REG_CXCSS_CTRL: u8 = 0x51; // CTCSS/CDCSS enable, polarity, 24-bit mode, TX gain
pub const REG_CXCSS_TAIL: u8 = 0x52; // CTCSS/CDCSS phase-shift tail generation (120/180/240 deg) & detection thresholds
pub const REG_ALC_TC: u8 = 0x53; // Audio ALC attack/decay time constants

// Digital Packet & FSK Modem Registers
pub const REG_FSK_CTRL: u8 = 0x58;
pub const REG_FSK_FIFO_CTRL: u8 = 0x59;
pub const REG_FSK_SYNC_0_1: u8 = 0x5A;
pub const REG_FSK_SYNC_2_3: u8 = 0x5B;
pub const REG_FSK_CRC: u8 = 0x5C;
pub const REG_FSK_PACKET_LEN: u8 = 0x5D;
pub const REG_FSK_FIFO_STATUS: u8 = 0x5E;
pub const REG_FSK_FIFO_DATA: u8 = 0x5F;

// Diagnostic & Signal Telemetry Registers
pub const REG_SNR_VAL: u8 = 0x61; // Signal-to-Noise Ratio indicator
pub const REG_LNA_PEAK_RSSI: u8 = 0x62; // [15:8] LNA peak RSSI, [7:0] AGC RSSI
pub const REG_GLITCH_VAL: u8 = 0x63; // Glitch indicator [7:0]
pub const REG_AUDIO_AMP: u8 = 0x64; // Voice / modulator amplitude output
pub const REG_NOISE_VAL: u8 = 0x65; // Ex-noise indicator [6:0]
pub const REG_ADJ_CH_PWR: u8 = 0x66; // [15:8] Upper adjacent channel power, [7:0] Lower adjacent channel power
pub const REG_RSSI_VAL: u8 = 0x67; // Calibrated RSSI count [8:0] (in 0.5 dB steps above -160 dBm)
pub const REG_CTCSS_SCAN: u8 = 0x68; // CTCSS scan indicator (bit 15 = 0 when locked)
pub const REG_CDCSS_SCAN_HI: u8 = 0x69; // CDCSS scan high 12 bits (bit 15 = 0 when locked)
pub const REG_CDCSS_SCAN_LO: u8 = 0x6A; // CDCSS scan low 12 bits
pub const REG_AFC_VAL: u8 = 0x6D; // Signed AFC frequency error indicator
pub const REG_AF_FREQ_OUT: u8 = 0x6E; // AF frequency output indicator
pub const REG_AF_AMP: u8 = 0x6F; // AF Tx/Rx input amplitude in dB [6:0]

// Tone Generator & AGC Config Registers
pub const REG_TONE_GAIN: u8 = 0x70; // Tone1 (bits 15:8) & Tone2 (bits 7:0) enable + gain
pub const REG_TONE1_FREQ: u8 = 0x71; // Tone1 DDS frequency word
pub const REG_TONE2_FREQ: u8 = 0x72; // Tone2 DDS frequency word / FSK baud rate divider
pub const REG_AFC: u8 = 0x73; // Automatic Frequency Correction (bit 4 afc_disable)
pub const REG_SQL_RSSI: u8 = 0x78; // RSSI open (bits 15:8) and close (bits 7:0) squelch thresholds
pub const REG_AGC_CONFIG: u8 = 0x7E; // AGC fix mode (bit 15), AGC index (bits 14:12), DC filter bandwidth

// Power & Loopback Mode Constants
pub const POWER_CTRL_RX: u16 = 0x9F1F; // 0x9D1F | (1 << 9): 2.7V RF LDO for full RX sensitivity
pub const POWER_CTRL_TX: u16 = 0x9D1F; // 2.4V RF LDO for TX / Beep
pub const POWER_CTRL_SLEEP: u16 = 0x1D00;
pub const SYS_POWER_RX: u16 = 0xBFF1;
pub const SYS_POWER_TX: u16 = 0xC3FE;
pub const SYS_POWER_LOOPBACK: u16 = 0xFFF7; // Internal TX-to-RX coupling enabled with PA_GAIN (bit 3) disabled

// Status & Interrupt Bits in Reg 0x02 and 0x3F
pub const INT_FSK_TX_FINISHED: u16 = 1 << 15;
pub const INT_FSK_FIFO_ALMOST_EMPTY: u16 = 1 << 14;
pub const INT_FSK_RX_FINISHED: u16 = 1 << 13;
pub const INT_FSK_FIFO_ALMOST_FULL: u16 = 1 << 12;
pub const INT_DTMF_5TONE_FOUND: u16 = 1 << 11;
pub const INT_CXCSS_TAIL: u16 = 1 << 10;
pub const INT_CDCSS_FOUND: u16 = 1 << 9;
pub const INT_CDCSS_LOST: u16 = 1 << 8;
pub const INT_CTCSS_FOUND: u16 = 1 << 7;
pub const INT_CTCSS_LOST: u16 = 1 << 6;
pub const INT_VOX_FOUND: u16 = 1 << 5;
pub const INT_VOX_LOST: u16 = 1 << 4;
pub const INT_SQUELCH_FOUND: u16 = 1 << 3;
pub const INT_SQUELCH_LOST: u16 = 1 << 2;
pub const INT_FSK_RX_SYNC: u16 = 1 << 1;
