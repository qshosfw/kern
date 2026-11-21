//! Block storage abstraction for SPI NOR flash.

/// Error type for storage operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    /// Operation timed out waiting for flash.
    Timeout,
    /// Flash is write-protected.
    WriteProtect,
    /// Device is busy (e.g., erase in progress).
    Busy,
    /// Address or size out of bounds.
    OutOfBounds,
}

/// A block-addressable storage device (SPI NOR flash).
pub trait BlockDevice {
    /// Read `buf.len()` bytes starting at byte offset `offset`.
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), StorageError>;
    /// Program (write) `data` starting at byte offset `offset`.
    /// Caller must ensure the region has been erased first.
    /// Handles page-boundary splitting internally.
    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), StorageError>;
    /// Erase a sector by sector index.
    fn erase_sector(&mut self, sector: u32) -> Result<(), StorageError>;
    /// Sector size in bytes (typically 4096 for SPI NOR).
    fn sector_size(&self) -> u32;
    /// Total number of sectors.
    fn total_sectors(&self) -> u32;
}
