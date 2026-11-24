//! UV-K1 / UV-K5v3 board-level peripheral models:
//! - ST7565 128×64 monochrome/dithered SPI LCD controller
//! - PY25Q16 2 MiB SPI NOR Flash with optional persistent file backing & test corruption/blank modes
//! - BK4829 / BK4819 3-wire RF transceiver register & telemetry model
//! - 5×4 Keypad Matrix + dedicated PTT pin

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const FLASH_SIZE: usize = 2 * 1024 * 1024;
pub const SECTOR_SIZE: usize = 4096;

/// RLE-compressed factory radio calibration (512 B) placed at SPI flash `0x010000` in `Normal` mode.
const DEFAULT_CALIB_RLE: [u8; 310] = [
    0x0A, 0x53, 0x55, 0x57, 0x59, 0x5B, 0x5D, 0x5F, 0x62, 0x65, 0xFF, 0x06, 0x05, 0x50, 0x52, 0x54,
    0x56, 0x58, 0x5A, 0x5C, 0x5F, 0x62, 0xFF, 0x06, 0x5A, 0x2D, 0x29, 0x26, 0x23, 0x20, 0x1D, 0x1A,
    0x17, 0x14, 0xFF, 0x06, 0x64, 0x30, 0x2D, 0x29, 0x26, 0x23, 0x20, 0x1D, 0x1A, 0x17, 0xFF, 0x06,
    0x5A, 0x14, 0x11, 0x0E, 0x0B, 0x08, 0x03, 0x02, 0x02, 0x02, 0xFF, 0x06, 0x64, 0x11, 0x0E, 0x0B,
    0x08, 0x05, 0x05, 0x04, 0x04, 0x04, 0xFF, 0x06, 0x32, 0x58, 0x5B, 0x5E, 0x60, 0x63, 0x65, 0x67,
    0x69, 0x6B, 0xFF, 0x06, 0x28, 0x55, 0x58, 0x5B, 0x5D, 0x60, 0x62, 0x64, 0x66, 0x68, 0xFF, 0x06,
    0x41, 0x32, 0x2D, 0x28, 0x24, 0x21, 0x1E, 0x1A, 0x17, 0x16, 0xFF, 0x06, 0x46, 0x37, 0x32, 0x2D,
    0x28, 0x25, 0x22, 0x1E, 0x1B, 0x19, 0xFF, 0x06, 0x5A, 0x19, 0x0F, 0x0A, 0x09, 0x08, 0x07, 0x06,
    0x05, 0x04, 0xFF, 0x06, 0x64, 0x1E, 0x14, 0x0F, 0x0D, 0x0C, 0x0B, 0x0A, 0x09, 0x08, 0xFF, 0x06,
    0x6E, 0x00, 0x78, 0x00, 0x82, 0x00, 0x8C, 0x00, 0xB4, 0x00, 0xBE, 0x00, 0xC8, 0x00, 0xD2, 0x00,
    0x32, 0x32, 0x32, 0x64, 0x64, 0x64, 0x8C, 0x8C, 0x8C, 0xFF, 0x07, 0x32, 0x32, 0x32, 0x64, 0x64,
    0x64, 0x8C, 0x8C, 0x8C, 0xFF, 0x07, 0x4B, 0x4B, 0x4B, 0x78, 0x78, 0x78, 0xA0, 0xA0, 0xA0, 0xFF,
    0x07, 0x32, 0x32, 0x32, 0x64, 0x64, 0x64, 0x8C, 0x8C, 0x8C, 0xFF, 0x07, 0x5A, 0x5A, 0x5A, 0x64,
    0x64, 0x64, 0xA0, 0xA0, 0xA0, 0xFF, 0x07, 0x4B, 0x4B, 0x4B, 0x78, 0x78, 0x78, 0xA0, 0xA0, 0xA0,
    0xFF, 0x07, 0x32, 0x32, 0x32, 0x64, 0x64, 0x64, 0x94, 0x8C, 0x8C, 0xFF, 0x07, 0x9E, 0x05, 0x02,
    0x08, 0x5A, 0x08, 0xB3, 0x08, 0x0A, 0x09, 0x28, 0x0A, 0xFF, 0x04, 0x1E, 0x00, 0x32, 0x00, 0x46,
    0x00, 0x5A, 0x00, 0x6E, 0x00, 0x82, 0x00, 0x96, 0x00, 0xAA, 0x00, 0xC8, 0x00, 0xE6, 0x00, 0xFF,
    0x04, 0x14, 0x00, 0x28, 0x00, 0x3C, 0x00, 0x50, 0x00, 0x64, 0x00, 0x78, 0x00, 0x8C, 0x00, 0xA0,
    0x00, 0xBE, 0x00, 0xDC, 0x00, 0xFF, 0x04, 0x08, 0x0E, 0x13, 0x18, 0x1E, 0xFF, 0x03, 0x04, 0x00,
    0x46, 0x00, 0x50, 0x00, 0x37, 0x0E,
];

pub fn expand_default_calibration() -> [u8; 512] {
    let mut dst = [0xFFu8; 512];
    let mut i = 0;
    let mut o = 0;
    while i < DEFAULT_CALIB_RLE.len() {
        let b = DEFAULT_CALIB_RLE[i];
        i += 1;
        if b == 0xFF {
            o += DEFAULT_CALIB_RLE[i] as usize;
            i += 1;
        } else {
            dst[o] = b;
            o += 1;
        }
    }
    dst
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashInitMode {
    /// Load from disk (or create blank + factory calib at `0x010000`).
    Normal,
    /// Brand-new blank erased SPI flash (`0xFF`), with factory calib at `0x010000`.
    BlankWithCalib,
    /// Completely blank erased SPI flash (`0xFF` everywhere, no factory calib).
    BlankNoCalib,
    /// Corrupted non-blank superblocks at `0x0000` / `0x1000` (with factory calib at `0x010000`).
    Corrupt,
}

pub struct St7565Panel {
    pub gram: [u8; 1024],
    pub display_on: bool,
    pub inverted: bool,
    pub all_points_on: bool,
    pub contrast: u8,
    pub start_line: u8,
    page: u8,
    col: u8,
    awaiting_ev: bool,
    pub frame_seq: u64,
}

impl St7565Panel {
    pub fn new() -> Self {
        Self {
            gram: [0u8; 1024],
            display_on: false,
            inverted: false,
            all_points_on: false,
            contrast: 31,
            start_line: 0,
            page: 0,
            col: 0,
            awaiting_ev: false,
            frame_seq: 0,
        }
    }

    pub fn reset(&mut self) {
        self.display_on = false;
        self.inverted = false;
        self.all_points_on = false;
        self.contrast = 31;
        self.start_line = 0;
        self.page = 0;
        self.col = 0;
        self.awaiting_ev = false;
    }

    pub fn spi_byte(&mut self, dc_high: bool, byte: u8) {
        if !dc_high {
            if self.awaiting_ev {
                self.contrast = byte & 0x3F;
                self.awaiting_ev = false;
                return;
            }
            match byte {
                0x81 => self.awaiting_ev = true,
                0xAE => self.display_on = false,
                0xAF => self.display_on = true,
                0xA4 => self.all_points_on = false,
                0xA5 => self.all_points_on = true,
                0xA6 => self.inverted = false,
                0xA7 => self.inverted = true,
                0xE2 => self.reset(),
                0x40..=0x7F => self.start_line = byte & 0x3F,
                0xB0..=0xB7 => self.page = byte & 0x07,
                0x00..=0x0F => self.col = (self.col & 0xF0) | (byte & 0x0F),
                0x10..=0x1F => self.col = (self.col & 0x0F) | ((byte & 0x0F) << 4),
                _ => {}
            }
        } else {
            self.awaiting_ev = false;
            // UV-K1 / UV-K5v3 ST7565 panel shifts columns by COL_OFFSET = 4
            let panel_col = self.col.wrapping_sub(4);
            if (panel_col as usize) < 128 && (self.page as usize) < 8 {
                let idx = (self.page as usize) * 128 + (panel_col as usize);
                if self.gram[idx] != byte {
                    self.gram[idx] = byte;
                    self.frame_seq = self.frame_seq.wrapping_add(1);
                }
            }
            self.col = self.col.wrapping_add(1);
        }
    }

    pub fn visible_framebuffer(&self) -> [u8; 1024] {
        if !self.display_on {
            return [0u8; 1024];
        }
        if self.all_points_on {
            return [0xFFu8; 1024];
        }
        self.gram
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlashSpiState {
    Idle,
    CmdJEDEC(u8),
    CmdStatus,
    Addr { cmd: u8, step: u8, addr: u32 },
    Reading { addr: u32 },
    Programming { addr: u32 },
}

pub struct Py25q16Flash {
    pub data: Vec<u8>,
    backing_path: Option<PathBuf>,
    ephemeral: bool,
    pub dirty: bool,
    wel: bool,
    power_down: bool,
    cs_low: bool,
    state: FlashSpiState,
}

impl Py25q16Flash {
    pub fn new(backing: Option<&Path>, ephemeral: bool, mode: FlashInitMode) -> io::Result<Self> {
        let mut data = vec![0xFFu8; FLASH_SIZE];
        match mode {
            FlashInitMode::Normal => {
                if let Some(p) = backing
                    && p.is_file()
                {
                    let raw = fs::read(p)?;
                    let copy_len = raw.len().min(FLASH_SIZE);
                    data[..copy_len].copy_from_slice(&raw[..copy_len]);
                } else {
                    let cal = expand_default_calibration();
                    data[0x010000..0x010200].copy_from_slice(&cal);
                    if !ephemeral && let Some(p) = backing {
                        if let Some(parent) = p.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(p, &data)?;
                    }
                }
            }
            FlashInitMode::BlankWithCalib => {
                let cal = expand_default_calibration();
                data[0x010000..0x010200].copy_from_slice(&cal);
            }
            FlashInitMode::BlankNoCalib => {}
            FlashInitMode::Corrupt => {
                // Non-0xFF garbage in LittleFS superblock pair (0x0000 and 0x1000) + valid factory calib at 0x010000
                for off in [0usize, 0x1000usize] {
                    for i in 0..256 {
                        data[off + i] = ((i as u8).wrapping_mul(37)) ^ 0xA5;
                    }
                }
                let cal = expand_default_calibration();
                data[0x010000..0x010200].copy_from_slice(&cal);
            }
        }

        Ok(Self {
            data,
            backing_path: if ephemeral {
                None
            } else {
                backing.map(Path::to_path_buf)
            },
            ephemeral,
            dirty: false,
            wel: false,
            power_down: false,
            cs_low: false,
            state: FlashSpiState::Idle,
        })
    }

    pub fn set_cs(&mut self, low: bool) {
        if self.cs_low && !low {
            // Rising edge completes command
            if matches!(self.state, FlashSpiState::Programming { .. }) {
                self.wel = false;
            }
            self.state = FlashSpiState::Idle;
        }
        self.cs_low = low;
    }

    pub fn transfer(&mut self, tx: u8) -> u8 {
        if !self.cs_low {
            return 0xFF;
        }
        if self.power_down && tx != 0xAB {
            return 0xFF;
        }
        match self.state {
            FlashSpiState::Idle => match tx {
                0xAB => {
                    self.power_down = false;
                    0xFF
                }
                0xB9 => {
                    self.power_down = true;
                    0xFF
                }
                0x06 => {
                    self.wel = true;
                    0xFF
                }
                0x04 => {
                    self.wel = false;
                    0xFF
                }
                0x05 => {
                    self.state = FlashSpiState::CmdStatus;
                    0xFF
                }
                0x9F => {
                    self.state = FlashSpiState::CmdJEDEC(0);
                    0xFF
                }
                0x03 | 0x02 | 0x20 | 0x52 | 0xD8 => {
                    self.state = FlashSpiState::Addr {
                        cmd: tx,
                        step: 0,
                        addr: 0,
                    };
                    0xFF
                }
                0x60 | 0xC7 => {
                    if self.wel {
                        self.data.fill(0xFF);
                        self.dirty = true;
                        self.wel = false;
                    }
                    0xFF
                }
                _ => 0xFF,
            },
            FlashSpiState::CmdStatus => if self.wel { 0x02 } else { 0x00 },
            FlashSpiState::CmdJEDEC(idx) => {
                let out = match idx {
                    0 => 0x85, // Puya Semiconductor
                    1 => 0x20,
                    2 => 0x15, // PY25Q16 (16 Mbit / 2 MiB)
                    _ => 0xFF,
                };
                self.state = FlashSpiState::CmdJEDEC(idx.saturating_add(1));
                out
            }
            FlashSpiState::Addr { cmd, step, addr } => {
                let next_addr = (addr << 8) | (tx as u32);
                if step < 2 {
                    self.state = FlashSpiState::Addr {
                        cmd,
                        step: step + 1,
                        addr: next_addr,
                    };
                } else {
                    match cmd {
                        0x03 => self.state = FlashSpiState::Reading { addr: next_addr },
                        0x02 => self.state = FlashSpiState::Programming { addr: next_addr },
                        0x20 => {
                            if self.wel {
                                let base = (next_addr as usize) & !(SECTOR_SIZE - 1);
                                if base + SECTOR_SIZE <= self.data.len() {
                                    self.data[base..base + SECTOR_SIZE].fill(0xFF);
                                    self.dirty = true;
                                }
                                self.wel = false;
                            }
                            self.state = FlashSpiState::Idle;
                        }
                        0x52 => {
                            if self.wel {
                                let base = (next_addr as usize) & !(32768 - 1);
                                let end = (base + 32768).min(self.data.len());
                                if base < end {
                                    self.data[base..end].fill(0xFF);
                                    self.dirty = true;
                                }
                                self.wel = false;
                            }
                            self.state = FlashSpiState::Idle;
                        }
                        0xD8 => {
                            if self.wel {
                                let base = (next_addr as usize) & !(65536 - 1);
                                let end = (base + 65536).min(self.data.len());
                                if base < end {
                                    self.data[base..end].fill(0xFF);
                                    self.dirty = true;
                                }
                                self.wel = false;
                            }
                            self.state = FlashSpiState::Idle;
                        }
                        _ => self.state = FlashSpiState::Idle,
                    }
                }
                0xFF
            }
            FlashSpiState::Reading { addr } => {
                let idx = (addr as usize) % self.data.len();
                let b = self.data[idx];
                self.state = FlashSpiState::Reading {
                    addr: addr.wrapping_add(1),
                };
                b
            }
            FlashSpiState::Programming { addr } => {
                if self.wel {
                    let idx = addr as usize;
                    if idx < self.data.len() {
                        self.data[idx] &= tx;
                        self.dirty = true;
                    }
                    // Page program wraps within 256-byte page
                    let next_in_page = (addr & !0xFF) | (addr.wrapping_add(1) & 0xFF);
                    self.state = FlashSpiState::Programming { addr: next_in_page };
                }
                0xFF
            }
        }
    }

    pub fn flush(&mut self) {
        if !self.dirty || self.ephemeral {
            return;
        }
        if let Some(p) = &self.backing_path {
            let tmp = p.with_extension("img.tmp");
            if fs::write(&tmp, &self.data).is_ok() {
                let _ = fs::rename(&tmp, p);
                self.dirty = false;
            }
        }
    }
}

/// BK4829 / BK4819 3-wire serial transceiver model (`CSN`, `SCL`, `SDA`).
pub struct Bk4829Chip {
    pub regs: [u16; 128],
    csn: bool,
    scl: bool,
    bit_idx: u8,
    cmd_byte: u8,
    write_val: u16,
    read_val: u16,
    sda_out: bool,
}

impl Bk4829Chip {
    pub fn new() -> Self {
        let mut regs = [0u16; 128];
        regs[0x33] = 0x9000; // GPIO output latch default (LEDs off)
        regs[0x67] = 72;     // Quiet noise floor RSSI (~ -124 dBm)
        regs[0x65] = 0x2A18; // High noise/glitch when squelched
        Self {
            regs,
            csn: true,
            scl: true,
            bit_idx: 0,
            cmd_byte: 0,
            write_val: 0,
            read_val: 0,
            sda_out: true,
        }
    }

    pub fn on_pins(&mut self, csn: bool, scl: bool, sda_mosi: bool) {
        if csn {
            self.csn = true;
            self.scl = scl;
            self.bit_idx = 0;
            self.sda_out = true;
            return;
        }
        if self.csn && !csn {
            // Falling edge of CSN starts a new 24-bit frame
            self.csn = false;
            self.bit_idx = 0;
            self.cmd_byte = 0;
            self.write_val = 0;
        }
        // Rising edge of SCL clocks bits
        if !self.scl && scl {
            if self.bit_idx < 8 {
                self.cmd_byte = (self.cmd_byte << 1) | u8::from(sda_mosi);
                self.bit_idx += 1;
                if self.bit_idx == 8 && (self.cmd_byte & 0x80) != 0 {
                    let reg = (self.cmd_byte & 0x7F) as usize;
                    self.read_val = self.regs[reg];
                }
            } else if (self.cmd_byte & 0x80) != 0 {
                // Read cycle: present MSB first during SCL high
                let shift = 15u8.saturating_sub(self.bit_idx - 8);
                self.sda_out = ((self.read_val >> shift) & 1) != 0;
                self.bit_idx = self.bit_idx.saturating_add(1);
            } else if self.bit_idx < 24 {
                self.write_val = (self.write_val << 1) | u16::from(sda_mosi);
                self.bit_idx += 1;
                if self.bit_idx == 24 {
                    let reg = (self.cmd_byte & 0x7F) as usize;
                    self.regs[reg] = self.write_val;
                }
            }
        }
        self.scl = scl;
    }

    pub fn sda_level(&self) -> bool {
        self.sda_out
    }

    pub fn status_led(&self) -> &'static str {
        match self.regs[0x33] & 0x03 {
            1 => "red",
            2 => "green",
            3 => "orange",
            _ => "off",
        }
    }

    pub fn tuned_freq_hz(&self) -> u32 {
        let lo = self.regs[0x38] as u32;
        let hi = self.regs[0x39] as u32;
        ((hi << 16) | lo) * 10
    }

    pub fn set_rf_signal(&mut self, rssi_dbm: i16, carrier: bool) {
        // BK4829 RSSI register = (dBm + 160) * 2
        let raw = ((rssi_dbm.clamp(-155, -20) + 160) * 2) as u16;
        self.regs[0x67] = raw & 0x01FF;
        if carrier {
            self.regs[0x65] = 0x0402;
            self.regs[0x0C] |= 1 << 1;
        } else {
            self.regs[0x65] = 0x2A18;
            self.regs[0x0C] &= !(1 << 1);
        }
    }
}

/// Maps friendly key names to the 19-bit `ALL_KEYS` bit index (`0..=18`).
pub fn key_name_to_bit(name: &str) -> Option<u8> {
    match name.trim().to_ascii_lowercase().as_str() {
        "side1" => Some(0),
        "side2" => Some(1),
        "menu" | "m" | "enter" => Some(2),
        "1" => Some(3),
        "4" => Some(4),
        "7" => Some(5),
        "up" => Some(6),
        "2" => Some(7),
        "5" => Some(8),
        "8" => Some(9),
        "down" => Some(10),
        "3" => Some(11),
        "6" => Some(12),
        "9" => Some(13),
        "exit" | "esc" | "back" => Some(14),
        "star" | "*" => Some(15),
        "0" => Some(16),
        "f" | "fn" | "hash" | "#" => Some(17),
        "ptt" => Some(18),
        _ => None,
    }
}

pub struct KeypadMatrix {
    /// Sustainably held key bitmask (cleared by `release_all` or timed release).
    pub held_mask: u32,
    /// Optional scheduled auto-release `(bit_mask, release_at_virtual_ms)`.
    timed_releases: Vec<(u32, u64)>,
}

impl KeypadMatrix {
    pub fn new() -> Self {
        Self {
            held_mask: 0,
            timed_releases: Vec::new(),
        }
    }

    pub fn press_for_ms(&mut self, bit: u8, hold_ms: u64, now_ms: u64) {
        let m = 1u32 << bit;
        self.held_mask |= m;
        self.timed_releases.retain(|(bm, _)| *bm != m);
        self.timed_releases.push((m, now_ms.saturating_add(hold_ms.max(45))));
    }

    pub fn set_held(&mut self, bit: u8, down: bool) {
        let m = 1u32 << bit;
        self.timed_releases.retain(|(bm, _)| *bm != m);
        if down {
            self.held_mask |= m;
        } else {
            self.held_mask &= !m;
        }
    }

    pub fn release_all(&mut self) {
        self.held_mask = 0;
        self.timed_releases.clear();
    }

    pub fn tick_ms(&mut self, now_ms: u64) {
        let mut expired = 0u32;
        self.timed_releases.retain(|&(m, until)| {
            if now_ms >= until {
                expired |= m;
                false
            } else {
                true
            }
        });
        self.held_mask &= !expired;
    }

    /// Synthesize `GPIOB` input levels for rows `PB15..PB12` and `PTT` (`PB10`)
    /// given `GPIOB` `MODER` and `ODR` (columns `PB6..PB3`).
    pub fn overlay_gpiob_idr(&self, base_idr: u32, moder: u32, odr: u32) -> u32 {
        let mut idr = base_idr | (1 << 10) | (0x0F << 12);
        let mask = self.held_mask;
        if mask == 0 {
            return idr;
        }

        // PTT (bit 18) pulls PB10 directly to GND
        if (mask & (1 << 18)) != 0 {
            idr &= !(1 << 10);
        }
        // Side1 (bit 0) -> PB15 to GND; Side2 (bit 1) -> PB14 to GND
        if (mask & (1 << 0)) != 0 {
            idr &= !(1 << 15);
        }
        if (mask & (1 << 1)) != 0 {
            idr &= !(1 << 14);
        }

        // Matrix columns: col 0 = PB6, col 1 = PB5, col 2 = PB4, col 3 = PB3
        for col in 0..4usize {
            let col_pin = 6 - col;
            let col_mode = (moder >> (col_pin * 2)) & 0b11;
            let col_driven_low = col_mode == 0b01 && ((odr >> col_pin) & 1) == 0;
            if !col_driven_low {
                continue;
            }
            let base_bit = 2 + col * 4;
            // row 0 -> PB15, row 1 -> PB14, row 2 -> PB13, row 3 -> PB12
            for row in 0..4usize {
                if (mask & (1 << (base_bit + row))) != 0 {
                    let row_pin = 15 - row;
                    idr &= !(1 << row_pin);
                }
            }
        }
        idr
    }
}
