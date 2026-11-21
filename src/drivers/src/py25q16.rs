//! PY25Q16 2MB External SPI Flash Memory Driver.

pub const FLASH_PAGE_SIZE: usize = 256;
pub const FLASH_SECTOR_SIZE: usize = 4096;
pub const FLASH_TOTAL_SIZE: usize = 2 * 1024 * 1024; // 2MB

use kern_platform_traits::flash::SpiFlashBus;

#[inline(always)]
fn range_within_flash(address: u32, len: usize) -> bool {
    (address as usize)
        .checked_add(len)
        .is_some_and(|end| end <= FLASH_TOTAL_SIZE)
}

pub struct Py25q16FlashDriver<B> {
    bus: B,
}

impl<B: SpiFlashBus> Py25q16FlashDriver<B> {
    pub const fn new(bus: B) -> Self {
        Self { bus }
    }

    pub fn init(&mut self) {
        self.bus.cs_high();
        // Wake flash in case a reset (e.g. IWDG) occurred while it was in Deep Power-Down (0xB9).
        self.release_power_down();
    }

    #[inline(never)]
    fn send_cmd(&mut self, cmd: u8) {
        self.cs_assert();
        self.bus.write_byte(cmd);
        self.cs_release();
    }

    /// Enter Deep Power-Down (`0xB9`, <1 uA standby until `release_power_down()`).
    pub fn deep_power_down(&mut self) {
        self.send_cmd(0xB9);
    }

    /// Release from Deep Power-Down (`0xAB` + >=30 us recovery window).
    pub fn release_power_down(&mut self) {
        self.send_cmd(0xAB);
        #[cfg(target_arch = "arm")]
        self.bus.delay_us(30);
    }

    #[inline(always)]
    fn cs_assert(&mut self) {
        self.bus.cs_low();
    }

    #[inline(always)]
    fn cs_release(&mut self) {
        self.bus.wait_idle();
        self.bus.cs_high();
    }

    /// Read JEDEC Manufacturer and Device ID.
    pub fn read_jedec_id(&mut self) -> u32 {
        self.cs_assert();
        self.bus.transfer_byte(0x9F);
        let b1 = self.bus.transfer_byte(0xFF) as u32;
        let b2 = self.bus.transfer_byte(0xFF) as u32;
        let b3 = self.bus.transfer_byte(0xFF) as u32;
        self.cs_release();
        (b1 << 16) | (b2 << 8) | b3
    }

    fn read_status_reg(&mut self) -> u8 {
        self.cs_assert();
        self.bus.transfer_byte(0x05);
        let status = self.bus.transfer_byte(0xFF);
        self.cs_release();
        status
    }

    pub fn wait_while_busy(&mut self) {
        let mut polls = 0u32;
        while (self.read_status_reg() & 1) == 1 && polls < 2_000_000 {
            self.bus.feed();
            polls += 1;
        }
    }

    pub fn write_enable(&mut self) {
        self.send_cmd(0x06);
    }

    #[inline(never)]
    fn begin_addr_cmd(&mut self, cmd: u8, addr: u32) {
        self.cs_assert();
        self.bus.write_byte(cmd);
        self.bus.write_byte(((addr >> 16) & 0xFF) as u8);
        self.bus.write_byte(((addr >> 8) & 0xFF) as u8);
        self.bus.write_byte((addr & 0xFF) as u8);
    }

    pub fn erase_sector(&mut self, sector_addr: u32) -> Result<(), ()> {
        if sector_addr as usize >= FLASH_TOTAL_SIZE {
            return Err(());
        }
        let addr = sector_addr & !(FLASH_SECTOR_SIZE as u32 - 1);
        self.wait_while_busy();
        self.write_enable();

        self.begin_addr_cmd(0x20, addr);
        self.cs_release();

        self.wait_while_busy();
        Ok(())
    }

    pub fn program_page(&mut self, address: u32, data: &[u8]) -> Result<(), ()> {
        if !range_within_flash(address, data.len()) || data.len() > FLASH_PAGE_SIZE {
            return Err(());
        }
        self.wait_while_busy();
        self.write_enable();

        self.begin_addr_cmd(0x02, address);
        self.bus.write_bytes(data);
        self.cs_release();

        self.wait_while_busy();
        Ok(())
    }

    pub fn read_data(&mut self, address: u32, buffer: &mut [u8]) -> Result<(), ()> {
        if !range_within_flash(address, buffer.len()) {
            return Err(());
        }
        self.wait_while_busy();

        self.begin_addr_cmd(0x03, address);
        self.bus.read_bytes(buffer);
        self.cs_release();
        Ok(())
    }
}

impl<B: SpiFlashBus> kernfs::BlockDevice for Py25q16FlashDriver<B> {
    fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), kernfs::FsError> {
        self.read_data(offset as u32, buf)
            .map_err(|_| kernfs::FsError::IoError)
    }

    fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), kernfs::FsError> {
        let mut pos = 0usize;
        let mut addr = offset;
        while pos < data.len() {
            let page_remaining = FLASH_PAGE_SIZE - (addr & (FLASH_PAGE_SIZE - 1));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_bounds_check_rejects_overflow() {
        assert!(range_within_flash(0, FLASH_TOTAL_SIZE));
        assert!(!range_within_flash(FLASH_TOTAL_SIZE as u32, 1));
        assert!(!range_within_flash(u32::MAX, 1));
        assert!(!range_within_flash(1, usize::MAX));
    }
}
