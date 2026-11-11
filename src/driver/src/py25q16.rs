//! PY25Q16 2MB External SPI Flash Memory Driver.

pub const FLASH_PAGE_SIZE: usize = 256;
pub const FLASH_SECTOR_SIZE: usize = 4096;
pub const FLASH_TOTAL_SIZE: usize = 2 * 1024 * 1024; // 2MB

use kern_platform_traits::{gpio::OutputPin, spi::SpiDevice};

pub struct Py25q16FlashDriver<'a> {
    spi: &'a mut dyn SpiDevice,
    cs: &'a mut dyn OutputPin,
}

impl<'a> Py25q16FlashDriver<'a> {
    pub fn new(spi: &'a mut dyn SpiDevice, cs: &'a mut dyn OutputPin) -> Self {
        Self { spi, cs }
    }

    pub fn init(&mut self) {
        self.cs.set_high();
    }

    #[inline(always)]
    fn cs_assert(&mut self) {
        self.cs.set_low();
    }

    #[inline(always)]
    fn cs_release(&mut self) {
        self.cs.set_high();
    }

    /// Read JEDEC Manufacturer and Device ID.
    pub fn read_jedec_id(&mut self) -> u32 {
        self.cs_assert();
        self.spi.transfer_byte(0x9F);
        let b1 = self.spi.transfer_byte(0xFF) as u32;
        let b2 = self.spi.transfer_byte(0xFF) as u32;
        let b3 = self.spi.transfer_byte(0xFF) as u32;
        self.cs_release();
        (b1 << 16) | (b2 << 8) | b3
    }

    fn read_status_reg(&mut self) -> u8 {
        self.cs_assert();
        self.spi.transfer_byte(0x05);
        let status = self.spi.transfer_byte(0xFF);
        self.cs_release();
        status
    }

    pub fn wait_while_busy(&mut self) {
        while (self.read_status_reg() & 1) == 1 {
            bsp::Board::wdt_feed();
        }
    }

    pub fn write_enable(&mut self) {
        self.cs_assert();
        self.spi.write_byte(0x06);
        self.cs_release();
    }

    fn write_address(&mut self, addr: u32) {
        self.spi.write_byte(((addr >> 16) & 0xFF) as u8);
        self.spi.write_byte(((addr >> 8) & 0xFF) as u8);
        self.spi.write_byte((addr & 0xFF) as u8);
    }

    pub fn erase_sector(&mut self, sector_addr: u32) -> Result<(), ()> {
        if sector_addr as usize >= FLASH_TOTAL_SIZE {
            return Err(());
        }
        let addr = sector_addr - (sector_addr % FLASH_SECTOR_SIZE as u32);
        self.wait_while_busy();
        self.write_enable();

        self.cs_assert();
        self.spi.write_byte(0x20); // Sector Erase
        self.write_address(addr);
        self.cs_release();

        self.wait_while_busy();
        Ok(())
    }

    pub fn program_page(&mut self, address: u32, data: &[u8]) -> Result<(), ()> {
        if address as usize + data.len() > FLASH_TOTAL_SIZE || data.len() > FLASH_PAGE_SIZE {
            return Err(());
        }
        self.wait_while_busy();
        self.write_enable();

        self.cs_assert();
        self.spi.write_byte(0x02); // Page Program
        self.write_address(address);
        self.spi.write_bytes(data);
        self.cs_release();

        self.wait_while_busy();
        Ok(())
    }

    pub fn read_data(&mut self, address: u32, buffer: &mut [u8]) -> Result<(), ()> {
        if address as usize + buffer.len() > FLASH_TOTAL_SIZE {
            return Err(());
        }
        self.wait_while_busy();

        self.cs_assert();
        self.spi.write_byte(0x03); // Normal Read
        self.write_address(address);
        self.spi.read_bytes(buffer);
        self.cs_release();
        Ok(())
    }
}

impl<'a> kernfs::BlockDevice for Py25q16FlashDriver<'a> {
    fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), kernfs::FsError> {
        self.read_data(offset as u32, buf)
            .map_err(|_| kernfs::FsError::IoError)
    }

    fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), kernfs::FsError> {
        let mut pos = 0usize;
        let mut addr = offset;
        while pos < data.len() {
            let page_remaining = FLASH_PAGE_SIZE - (addr % FLASH_PAGE_SIZE);
            let chunk_len = (data.len() - pos).min(page_remaining);
            self.program_page(addr as u32, &data[pos..pos + chunk_len])
                .map_err(|_| kernfs::FsError::IoError)?;
            pos += chunk_len;
            addr += chunk_len;
        }
        Ok(())
    }

    fn erase_sector(&mut self, sector_addr: usize) -> Result<(), kernfs::FsError> {
        self.erase_sector(sector_addr as u32)
            .map_err(|_| kernfs::FsError::IoError)
    }
}
