use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::i2c::{Error, ErrorKind, ErrorType, I2c, Operation};

#[allow(dead_code)]
pub struct BitbangI2c<SCL, SDA> {
    scl: SCL,
    sda: SDA,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BitbangI2cError {
    NoAcknowledge,
    Timeout,
}

impl Error for BitbangI2cError {
    fn kind(&self) -> ErrorKind {
        match self {
            Self::NoAcknowledge => {
                ErrorKind::NoAcknowledge(embedded_hal::i2c::NoAcknowledgeSource::Unknown)
            }
            Self::Timeout => ErrorKind::Other,
        }
    }
}

impl<SCL, SDA> ErrorType for BitbangI2c<SCL, SDA> {
    type Error = BitbangI2cError;
}

impl<SCL: OutputPin + InputPin, SDA: OutputPin + InputPin> BitbangI2c<SCL, SDA> {
    pub fn new(scl: SCL, sda: SDA) -> Self {
        Self { scl, sda }
    }

    #[inline(never)]
    fn delay(&self) {
        // ~1 us half-bit delay scaled to active SYSCLK without hardware division.
        let cycles = (crate::rcc::sysclk_hz() >> 20).max(4);
        cortex_m::asm::delay(cycles);
    }

    #[inline(never)]
    fn release_scl_and_wait(&mut self) -> Result<(), BitbangI2cError> {
        self.scl.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        let mut spins = 1_000u32;
        while self.scl.is_low().unwrap_or(false) && spins > 0 {
            spins -= 1;
        }
        if spins == 0 {
            return Err(BitbangI2cError::Timeout);
        }
        self.delay();
        Ok(())
    }

    fn start(&mut self) -> Result<(), BitbangI2cError> {
        self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.release_scl_and_wait()?;
        self.sda.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        Ok(())
    }

    fn stop(&mut self) -> Result<(), BitbangI2cError> {
        self.sda.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.release_scl_and_wait()?;
        self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        Ok(())
    }

    fn write_byte(&mut self, mut byte: u8) -> Result<(), BitbangI2cError> {
        for _ in 0..8 {
            if (byte & 0x80) != 0 {
                self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
            } else {
                self.sda.set_low().map_err(|_| BitbangI2cError::Timeout)?;
            }
            byte <<= 1;
            self.delay();
            self.release_scl_and_wait()?;
            self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
            self.delay();
        }

        // ACK Check
        self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.release_scl_and_wait()?;

        let ack = self.sda.is_low().unwrap_or(false);

        self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();

        if ack {
            Ok(())
        } else {
            Err(BitbangI2cError::NoAcknowledge)
        }
    }

    fn read_byte(&mut self, ack: bool) -> Result<u8, BitbangI2cError> {
        self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        let mut byte = 0u8;

        for _ in 0..8 {
            self.release_scl_and_wait()?;
            byte <<= 1;
            if self.sda.is_high().unwrap_or(false) {
                byte |= 1;
            }
            self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
            self.delay();
        }

        if ack {
            self.sda.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        } else {
            self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        }

        self.release_scl_and_wait()?;
        self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();

        Ok(byte)
    }

    /// Clock out up to 9 SCL pulses + STOP to release a slave wedged holding SDA low after a glitch.
    pub fn recover_bus(&mut self) {
        let _ = self.sda.set_high();
        for _ in 0..9 {
            let _ = self.scl.set_low();
            self.delay();
            let _ = self.scl.set_high();
            self.delay();
            if self.sda.is_high().unwrap_or(false) {
                break;
            }
        }
        let _ = self.stop();
    }

    /// Raw START / WRITE / STOP sequence for 2-wire chips with non-standard framing (e.g. BK1080).
    pub fn write_raw(&mut self, bytes: &[u8]) -> Result<(), BitbangI2cError> {
        let res = (|| {
            self.start()?;
            for &b in bytes {
                self.write_byte(b)?;
            }
            Ok(())
        })();
        let stop_res = self.stop();
        res.and(stop_res)
    }

    /// Raw START / WRITE prefix + RESTART / READ / STOP sequence.
    pub fn write_read_raw(&mut self, write: &[u8], read: &mut [u8]) -> Result<(), BitbangI2cError> {
        let res = (|| {
            self.start()?;
            for &b in write {
                self.write_byte(b)?;
            }
            let n = read.len();
            for (i, dst) in read.iter_mut().enumerate() {
                *dst = self.read_byte(i + 1 < n)?;
            }
            Ok(())
        })();
        let stop_res = self.stop();
        res.and(stop_res)
    }

    /// Direct register write/read avoiding generic `embedded_hal::i2c::Operation` trait machinery.
    #[inline(never)]
    pub fn xfer_reg(
        &mut self,
        addr: u8,
        reg: u8,
        tx: Option<&[u8]>,
        rx: Option<&mut [u8]>,
    ) -> Result<(), BitbangI2cError> {
        let res = (|| {
            self.start()?;
            self.write_byte(addr << 1)?;
            self.write_byte(reg)?;
            if let Some(data) = tx {
                for &b in data {
                    self.write_byte(b)?;
                }
            } else if let Some(buf) = rx {
                self.start()?;
                self.write_byte((addr << 1) | 1)?;
                let len = buf.len();
                for (i, dst) in buf.iter_mut().enumerate() {
                    *dst = self.read_byte(i + 1 < len)?;
                }
            }
            Ok(())
        })();
        let stop_res = self.stop();
        res.and(stop_res)
    }
}

impl<SCL: OutputPin + InputPin, SDA: OutputPin + InputPin> I2c for BitbangI2c<SCL, SDA> {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        for op in operations {
            let res = match op {
                Operation::Read(buf) => (|| {
                    self.start()?;
                    self.write_byte((address << 1) | 1)?;
                    let len = buf.len();
                    for (i, byte) in buf.iter_mut().enumerate() {
                        *byte = self.read_byte(i + 1 < len)?;
                    }
                    Ok(())
                })(),
                Operation::Write(buf) => (|| {
                    self.start()?;
                    self.write_byte(address << 1)?;
                    for &byte in buf.iter() {
                        self.write_byte(byte)?;
                    }
                    Ok(())
                })(),
            };
            let stop_res = self.stop();
            res.and(stop_res)?;
        }
        Ok(())
    }
}

/// Hardware I2C1 controller wrapper using `pac::i2c` registers.
pub struct HwI2c1;

impl HwI2c1 {
    /// Initialize I2C1 in standard mode (100 kHz) for the current APB1 clock (`pclk_mhz`).
    pub fn init_100khz(pclk_mhz: u32) {
        let freq = pclk_mhz.clamp(2, 48);
        let ccr = (freq * 5).max(4);
        unsafe {
            let apb1 = pac::rcc::RCC_APB1ENR.read_volatile();
            pac::rcc::RCC_APB1ENR.write_volatile(apb1 | pac::rcc::APB1ENR_I2C1);

            pac::i2c::I2C1_CR1.write_volatile(pac::i2c::CR1_SWRST);
            pac::i2c::I2C1_CR1.write_volatile(0);
            pac::i2c::I2C1_CR2.write_volatile(freq);
            pac::i2c::I2C1_CCR.write_volatile(ccr);
            pac::i2c::I2C1_TRISE.write_volatile(freq + 1);
            pac::i2c::I2C1_CR1.write_volatile(pac::i2c::CR1_PE);
        }
    }

    fn wait_sr1(mask: u32) -> Result<(), BitbangI2cError> {
        let mut timeout = 50_000u32;
        while timeout > 0 {
            let sr1 = unsafe { pac::i2c::I2C1_SR1.read_volatile() };
            if (sr1 & pac::i2c::SR1_AF) != 0 {
                unsafe {
                    pac::i2c::I2C1_SR1.write_volatile(sr1 & !pac::i2c::SR1_AF);
                    pac::i2c::I2C1_CR1.write_volatile(pac::i2c::CR1_PE | pac::i2c::CR1_STOP);
                }
                return Err(BitbangI2cError::NoAcknowledge);
            }
            if (sr1 & mask) != 0 {
                return Ok(());
            }
            timeout -= 1;
        }
        // Issue STOP on timeout so I2C1 does not remain wedged with BUSY=1.
        unsafe {
            pac::i2c::I2C1_CR1.write_volatile(pac::i2c::CR1_PE | pac::i2c::CR1_STOP);
        }
        Err(BitbangI2cError::Timeout)
    }

    /// Write `data` to 7-bit `addr` over hardware I2C1.
    pub fn write(&mut self, addr: u8, data: &[u8]) -> Result<(), BitbangI2cError> {
        unsafe {
            pac::i2c::I2C1_CR1.write_volatile(pac::i2c::CR1_PE | pac::i2c::CR1_START);
        }
        Self::wait_sr1(pac::i2c::SR1_SB)?;
        unsafe {
            pac::i2c::I2C1_DR.write_volatile(((addr as u32) << 1) & 0xFE);
        }
        Self::wait_sr1(pac::i2c::SR1_ADDR)?;
        let _ = unsafe { pac::i2c::I2C1_SR2.read_volatile() };
        for &b in data {
            Self::wait_sr1(pac::i2c::SR1_TXE)?;
            unsafe { pac::i2c::I2C1_DR.write_volatile(b as u32) };
        }
        Self::wait_sr1(pac::i2c::SR1_BTF)?;
        unsafe {
            pac::i2c::I2C1_CR1.write_volatile(pac::i2c::CR1_PE | pac::i2c::CR1_STOP);
        }
        Ok(())
    }
}
