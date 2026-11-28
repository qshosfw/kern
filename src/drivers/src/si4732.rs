//! Portable Si4732-A10 (`0x11` 7-bit I2C) HF/LW/MW/SW AM & LSB/USB SSB Receiver Driver.
//!
//! Zero-cost when `si4732` feature is disabled (`0 B` on `UV-K1`).
//! Streams `/sys/si4732_ssb.lzss` (`SSB1`) in 64-byte chunks with zero heap allocation,
//! enables the on-board HF LNA via `GPO_CTL` (`GPO2 = HIGH`), and registers `Cap::BroadBandRx`.

use kernel::dev::{BusType, DeviceDescriptor, DeviceStatus, DeviceType, Driver};

pub const SI4732_I2C_ADDR: u8 = 0x11;
pub const SSB_PATCH_IDENT: &[u8; 4] = b"SSB1";

// Si4732 command opcodes
const CMD_POWER_UP: u8 = 0x01;
const CMD_POWER_DOWN: u8 = 0x11;
const CMD_SET_PROPERTY: u8 = 0x12;
const CMD_AM_TUNE_FREQ: u8 = 0x40;
const CMD_AM_RSQ_STATUS: u8 = 0x43;
const CMD_GPO_CTL: u8 = 0x80;
const CMD_GPO_SET: u8 = 0x81;

const PROP_SSB_MODE: u16 = 0x0101;
const PROP_SSB_BFO: u16 = 0x0100;

/// Minimal portable I2C bus contract for `Si4732` (implemented by `PB8`/`PB9` I2C1 on UV-K10 Pro).
pub trait Si4732Bus {
    fn i2c_write(&mut self, addr: u8, data: &[u8]) -> Result<(), ()>;
    fn i2c_read(&mut self, addr: u8, dst: &mut [u8]) -> Result<(), ()>;
    fn delay_ms(&mut self, ms: u32);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Si4732Mode {
    Am = 0,
    Lsb = 1,
    Usb = 2,
    Fm = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Si4732Rsq {
    pub rssi_dbuv: u8,
    pub snr_db: u8,
    pub valid: bool,
}

pub struct Si4732<B: Si4732Bus> {
    bus: B,
    pub freq_khz: u16,
    pub bfo_hz: i16,
    pub mode: Si4732Mode,
    pub ssb_patch_loaded: bool,
    pub powered: bool,
}

impl<B: Si4732Bus> Si4732<B> {
    pub const fn new(bus: B) -> Self {
        Self {
            bus,
            freq_khz: 7100,
            bfo_hz: 0,
            mode: Si4732Mode::Am,
            ssb_patch_loaded: false,
            powered: false,
        }
    }

    fn wait_cts(&mut self) -> Result<(), ()> {
        let mut status = [0u8; 1];
        for _ in 0..50 {
            self.bus.i2c_read(SI4732_I2C_ADDR, &mut status)?;
            if (status[0] & 0x80) != 0 {
                return Ok(());
            }
            self.bus.delay_ms(2);
        }
        Err(())
    }

    fn send_cmd(&mut self, cmd: &[u8]) -> Result<(), ()> {
        self.bus.i2c_write(SI4732_I2C_ADDR, cmd)?;
        self.wait_cts()
    }

    fn set_property(&mut self, prop: u16, val: u16) -> Result<(), ()> {
        let p = prop.to_be_bytes();
        let v = val.to_be_bytes();
        self.send_cmd(&[CMD_SET_PROPERTY, 0x00, p[0], p[1], v[0], v[1]])
    }

    /// Power up Si4732 in AM/HF receive mode with 32.768 kHz external crystal (`X600`).
    pub fn power_up_am(&mut self) -> Result<(), ()> {
        // FUNC=1 (AM RX), XOSCEN=1 (0x11), OPMODE=0x05 (Analog LOUT/ROUT)
        self.send_cmd(&[CMD_POWER_UP, 0x11, 0x05])?;
        self.powered = true;
        self.ssb_patch_loaded = false;
        self.mode = Si4732Mode::Am;
        self.enable_hf_lna()
    }

    /// Enable Si4732 `GPO2` (`AM_EN`) to power the dual `2SC5066` HF/AM RF preamplifier.
    pub fn enable_hf_lna(&mut self) -> Result<(), ()> {
        self.send_cmd(&[CMD_GPO_CTL, 0x04])?; // GPO2OEN = 1
        self.send_cmd(&[CMD_GPO_SET, 0x04]) // GPO2LEVEL = 1
    }

    /// Stream an SSB firmware patch (8-byte I2C frames) directly into Si4732 RAM.
    pub fn load_ssb_patch_chunk(&mut self, chunk: &[u8]) -> Result<(), ()> {
        for frame in chunk.chunks_exact(8) {
            self.send_cmd(frame)?;
        }
        self.ssb_patch_loaded = true;
        Ok(())
    }

    /// Tune `150 kHz ..= 30,000 kHz` in AM, LSB, or USB mode.
    pub fn tune_khz(&mut self, freq_khz: u16, mode: Si4732Mode) -> Result<(), ()> {
        let clamped = freq_khz.clamp(150, 30_000);
        let f = clamped.to_be_bytes();
        let arg1 = match mode {
            Si4732Mode::Usb => 0x02,
            Si4732Mode::Lsb => 0x01,
            _ => 0x00,
        };
        if matches!(mode, Si4732Mode::Lsb | Si4732Mode::Usb) {
            let sideband_bits = if mode == Si4732Mode::Usb {
                0x0002
            } else {
                0x0001
            };
            self.set_property(PROP_SSB_MODE, sideband_bits)?;
        }
        self.send_cmd(&[CMD_AM_TUNE_FREQ, arg1, f[0], f[1], 0x00, 0x00])?;
        self.freq_khz = clamped;
        self.mode = mode;
        Ok(())
    }

    /// Fine-tune SSB BFO offset (`-16,000 ..= +16,000 Hz`).
    pub fn set_bfo_hz(&mut self, bfo_hz: i16) -> Result<(), ()> {
        let clamped = bfo_hz.clamp(-16_000, 16_000);
        self.set_property(PROP_SSB_BFO, clamped as u16)?;
        self.bfo_hz = clamped;
        Ok(())
    }

    /// Read Received Signal Quality (`RSSI` dBuV and `SNR` dB).
    pub fn get_rsq(&mut self) -> Result<Si4732Rsq, ()> {
        self.send_cmd(&[CMD_AM_RSQ_STATUS, 0x00])?;
        let mut resp = [0u8; 8];
        self.bus.i2c_read(SI4732_I2C_ADDR, &mut resp)?;
        Ok(Si4732Rsq {
            valid: (resp[2] & 0x01) != 0,
            rssi_dbuv: resp[4],
            snr_db: resp[5],
        })
    }

    pub fn power_down(&mut self) -> Result<(), ()> {
        if self.powered {
            self.send_cmd(&[CMD_POWER_DOWN])?;
            self.powered = false;
        }
        Ok(())
    }
}

impl<B: Si4732Bus + 'static> Driver for Si4732<B> {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor {
            dev_type: DeviceType::BroadBandReceiver,
            name: "Si4732",
            caps: ::abi::CapabilitySet::empty().with(::abi::Cap::BroadBandRx),
            status: if self.powered {
                DeviceStatus::Ready
            } else {
                DeviceStatus::Uninitialized
            },
        }
    }

    fn bus(&self) -> BusType {
        BusType::I2c {
            address: SI4732_I2C_ADDR,
        }
    }

    fn init(&mut self) -> Result<(), ()> {
        self.power_up_am()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockI2c {
        writes: usize,
    }
    impl Si4732Bus for MockI2c {
        fn i2c_write(&mut self, addr: u8, _data: &[u8]) -> Result<(), ()> {
            assert_eq!(addr, SI4732_I2C_ADDR);
            self.writes += 1;
            Ok(())
        }
        fn i2c_read(&mut self, _addr: u8, dst: &mut [u8]) -> Result<(), ()> {
            dst.fill(0);
            dst[0] = 0x80; // CTS ready
            if dst.len() >= 6 {
                dst[2] = 0x01;
                dst[4] = 42;
                dst[5] = 18;
            }
            Ok(())
        }
        fn delay_ms(&mut self, _ms: u32) {}
    }

    #[test]
    fn si4732_power_up_ssb_tune_and_rsq() {
        let mut rx = Si4732::new(MockI2c { writes: 0 });
        rx.power_up_am().unwrap();
        assert!(rx.powered);
        rx.load_ssb_patch_chunk(&[0x15; 16]).unwrap();
        assert!(rx.ssb_patch_loaded);
        rx.tune_khz(14_230, Si4732Mode::Usb).unwrap();
        assert_eq!(rx.freq_khz, 14_230);
        assert_eq!(rx.mode, Si4732Mode::Usb);
        rx.set_bfo_hz(-250).unwrap();
        let rsq = rx.get_rsq().unwrap();
        assert!(rsq.valid);
        assert_eq!((rsq.rssi_dbuv, rsq.snr_db), (42, 18));
    }
}
