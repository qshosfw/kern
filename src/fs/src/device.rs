//! BlockDevice trait for underlying flash hardware drivers.

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound = 0,
    IoError = 1,
    CorruptedData = 2,
    CapacityFull = 4,
    AccessDenied = 5,
    EncryptedNoKey = 6,
    InvalidPath = 7,
    LowPower = 8,
}

pub trait BlockDevice {
    const READ_SIZE: usize = 256;
    const WRITE_SIZE: usize = 256;
    const BLOCK_SIZE: usize = 4096;
    const TOTAL_BLOCKS: usize = 512; // 2MB SPI Flash (512 * 4KB)

    fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), FsError>;
    fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), FsError>;
    fn erase_sector(&mut self, sector_addr: usize) -> Result<(), FsError>;

    /// Optional SPI NOR Program/Erase Suspend (0x75) hook for zero-lag RX/UI reads.
    fn suspend_erase(&mut self) -> Result<(), FsError> {
        Ok(())
    }

    /// Optional SPI NOR Program/Erase Resume (0x7A) hook.
    fn resume_erase(&mut self) -> Result<(), FsError> {
        Ok(())
    }

    /// Erase the entire block device to `0xFF` (raw factory flash state).
    fn chip_erase(&mut self) -> Result<(), FsError> {
        for block in 0..Self::TOTAL_BLOCKS {
            self.erase_sector(block * Self::BLOCK_SIZE)?;
        }
        Ok(())
    }
}
