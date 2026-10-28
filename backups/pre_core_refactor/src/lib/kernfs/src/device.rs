//! BlockDevice trait for underlying flash hardware drivers.

pub enum FsError {
    IoError,
    NotFound,
    AccessDenied,
    CapacityFull,
    CorruptedData,
    InvalidPath,
}

pub trait BlockDevice {
    const READ_SIZE: usize = 256;
    const WRITE_SIZE: usize = 256;
    const BLOCK_SIZE: usize = 4096;
    const TOTAL_BLOCKS: usize = 512; // 2MB SPI Flash (512 * 4KB)

    fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), FsError>;
    fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), FsError>;
    fn erase_sector(&mut self, sector_addr: usize) -> Result<(), FsError>;
}
