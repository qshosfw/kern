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

impl<SCL: OutputPin, SDA: OutputPin + InputPin> BitbangI2c<SCL, SDA> {
    pub fn new(scl: SCL, sda: SDA) -> Self {
        Self { scl, sda }
    }

    fn delay(&self) {
        cortex_m::asm::delay(48); // ~1us delay at 48MHz clock
    }

    fn start(&mut self) -> Result<(), BitbangI2cError> {
        self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.scl.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.sda.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        Ok(())
    }

    fn stop(&mut self) -> Result<(), BitbangI2cError> {
        self.sda.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.scl.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
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
            self.scl.set_high().map_err(|_| BitbangI2cError::Timeout)?;
            self.delay();
            self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
            self.delay();
        }

        // ACK Check
        self.sda.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.scl.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();

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
            self.scl.set_high().map_err(|_| BitbangI2cError::Timeout)?;
            self.delay();
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

        self.scl.set_high().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();
        self.scl.set_low().map_err(|_| BitbangI2cError::Timeout)?;
        self.delay();

        Ok(byte)
    }
}

impl<SCL: OutputPin, SDA: OutputPin + InputPin> I2c for BitbangI2c<SCL, SDA> {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                Operation::Read(buf) => {
                    self.start()?;
                    self.write_byte((address << 1) | 1)?;
                    let len = buf.len();
                    for (i, byte) in buf.iter_mut().enumerate() {
                        *byte = self.read_byte(i < len - 1)?;
                    }
                    self.stop()?;
                }
                Operation::Write(buf) => {
                    self.start()?;
                    self.write_byte(address << 1)?;
                    for &byte in buf.iter() {
                        self.write_byte(byte)?;
                    }
                    self.stop()?;
                }
            }
        }
        Ok(())
    }
}
