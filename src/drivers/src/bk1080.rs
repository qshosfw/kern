//! BK1080 Broadcast FM Receiver Driver (64 MHz - 108 MHz).
//!
//! Provides hardware tuning, volume/mute control, RSSI/SNR signal metrics,
//! and stereo status for the optional BK1080 FM radio IC on the Quansheng K1 board.
//! Communicates over I2C at 7-bit address 0x40 (8-bit 0x80 write / 0x81 read).

use embedded_hal::i2c::I2c;
use kern_platform_traits::time::Delay;

pub const I2C_ADDR_BK1080: u8 = 0x40;

// ── Register Addresses ──────────────────────────────────────────────────────

pub const REG_CHIP_ID: u8 = 0x00;
pub const REG_POWER_CONFIG: u8 = 0x02;
pub const REG_CHANNEL: u8 = 0x03;
pub const REG_SYSTEM_CONFIG1: u8 = 0x04;
pub const REG_SYSTEM_CONFIG2: u8 = 0x05;
pub const REG_STATUS1: u8 = 0x07;
pub const REG_STATUS2: u8 = 0x0A;
pub const REG_INTERNAL_25: u8 = 0x19;

// Default register table sequence from manufacturer
const REGISTER_INIT_TABLE: [u16; 34] = [
    0x011E, 0x0000, 0x0201, 0x0000, 0x4010, 0x000B, 0xCE00, 0x0000, 0x5B11, 0x0000, 0x411E, 0x0000,
    0xCE00, 0x0000, 0x0000, 0x1000, 0x3197, 0x0000, 0x13FF, 0x9852, 0x0000, 0x0000, 0x0008, 0x0000,
    0x51E1, 0xA8BC, 0x2645, 0x00E4, 0x1CD8, 0x3A50, 0xEAE0, 0x3000, 0x0200, 0x0000,
];

pub struct Bk1080<I2C, D> {
    i2c: I2C,
    delay: D,
    is_initialized: bool,
    frequency_10khz: u16,
    is_muted: bool,
}

impl<I2C: I2c, D: Delay> Bk1080<I2C, D> {
    pub const fn new(i2c: I2C, delay: D) -> Self {
        Self {
            i2c,
            delay,
            is_initialized: false,
            frequency_10khz: 0,
            is_muted: false,
        }
    }

    /// Read a 16-bit register from the BK1080.
    pub fn read_reg(&mut self, reg: u8) -> Result<u16, I2C::Error> {
        let mut buf = [0u8; 2];
        self.i2c
            .write_read(I2C_ADDR_BK1080, &[reg << 1], &mut buf)?;
        Ok(u16::from_be_bytes(buf))
    }

    /// Write a 16-bit register to the BK1080.
    pub fn write_reg(&mut self, reg: u8, val: u16) -> Result<(), I2C::Error> {
        let bytes = val.to_be_bytes();
        self.i2c
            .write(I2C_ADDR_BK1080, &[reg << 1, bytes[0], bytes[1]])
    }

    /// Full startup initialization sequence.
    pub fn init(&mut self) -> Result<(), I2C::Error> {
        for (idx, &val) in REGISTER_INIT_TABLE.iter().enumerate() {
            self.write_reg(idx as u8, val)?;
        }

        // Delay ~250ms for PLL stabilization
        self.delay.delay_ms(250);

        self.write_reg(REG_INTERNAL_25, 0xA83C)?;
        self.write_reg(REG_INTERNAL_25, 0xA8BC)?;

        self.delay.delay_ms(62);

        self.is_initialized = true;
        Ok(())
    }

    /// Set FM tuned frequency in 10 kHz units (e.g. 104.10 MHz = 10410).
    /// Supports automatic band selection across 64.0 - 108.0 MHz.
    pub fn set_frequency_10khz(&mut self, freq_10k: u16) -> Result<(), I2C::Error> {
        if !self.is_initialized {
            self.init()?;
        }

        self.frequency_10khz = freq_10k;

        // Auto band select: band 3 (64-76MHz) vs band 1 (76-108MHz)
        let auto_band: u8 = if freq_10k < 7600 { 3 } else { 1 };
        let limit_10k: u16 = if auto_band == 3 { 6400 } else { 7600 };

        let (channel, _) = utils::math::div10_u16(freq_10k.saturating_sub(limit_10k));

        let reg5 = self.read_reg(REG_SYSTEM_CONFIG2)?;
        let updated_reg5 = (reg5 & !0x00F0) | ((auto_band as u16 & 3) << 6) | (1 << 4); // 100kHz space
        self.write_reg(REG_SYSTEM_CONFIG2, updated_reg5)?;

        self.write_reg(REG_CHANNEL, channel)?;
        self.delay.delay_ms(10);
        self.write_reg(REG_CHANNEL, channel | 0x8000)?; // Tune bit

        Ok(())
    }

    /// Mute or un-mute audio output.
    pub fn set_mute(&mut self, mute: bool) -> Result<(), I2C::Error> {
        self.is_muted = mute;
        self.write_reg(REG_POWER_CONFIG, if mute { 0x4201 } else { 0x0201 })
    }

    /// Read Received Signal Strength Indicator (RSSI: 0 - 255).
    pub fn get_rssi(&mut self) -> u8 {
        self.read_reg(REG_STATUS2)
            .map(|v| (v & 0xFF) as u8)
            .unwrap_or(0)
    }

    /// Read Signal-to-Noise Ratio (SNR).
    pub fn get_snr(&mut self) -> u8 {
        self.read_reg(REG_STATUS1)
            .map(|v| ((v >> 4) & 0x0F) as u8)
            .unwrap_or(0)
    }

    /// Check if current station is broadcasting in Stereo.
    pub fn is_stereo(&mut self) -> bool {
        self.read_reg(REG_STATUS2)
            .map(|v| (v & (1 << 9)) != 0)
            .unwrap_or(false)
    }

    /// Set hardware volume (Reg 0x05 bits [3:0], 0..=15)
    pub fn set_volume(&mut self, volume: u8) -> Result<(), I2C::Error> {
        let reg5 = self.read_reg(REG_SYSTEM_CONFIG2).unwrap_or(0x0A1F);
        let updated = (reg5 & !0x000F) | (volume.min(15) as u16);
        self.write_reg(REG_SYSTEM_CONFIG2, updated)
    }

    /// Enter ultra-low power standby
    pub fn power_down(&mut self) -> Result<(), I2C::Error> {
        self.write_reg(REG_POWER_CONFIG, 0x0000)
    }

    /// Wake up receiver
    pub fn wake(&mut self) -> Result<(), I2C::Error> {
        self.write_reg(REG_POWER_CONFIG, 0x0201)
    }
}
