//! USB Mass Storage Class (MSC) Bulk-Only Transport (BOT) & SCSI Handler.
//!
//! Implements USB Mass Storage Class Bulk-Only Transport Spec v1.0 and SCSI
//! Transparent Command Set (RBC/SPC-2) for zero-driver disk mounting.
//!
//! # Architectural Status: Optional Module (Disabled by Default)
//! While fully implemented in the Kern framework for portability and modularity,
//! MSC and virtual FAT translation are left uncompiled by default:
//! 1. Memory Footprint: Consumes ~5 KB of Flash and requires 512–1024 byte sector
//!    buffers, consuming ~6% of the MCU's 16 KB SRAM budget.
//! 2. Host OS Caching & Desync: Operating systems (Windows, macOS, Linux) cache
//!    FAT tables in host memory. Concurrent flash writes by radio firmware cause
//!    unrecoverable cache desynchronization and corruption.
//! 3. Metadata Pollution: Host OSes spam `.DS_Store`, `System Volume Information`,
//!    and `.Trashes`, exhausting limited embedded flash inodes and space.
//! 4. Unified Serial Strategy: The native KernFS filesystem is instead exposed
//!    directly through our unified CDC ACM packet protocol, providing zero-overhead,
//!    corruption-immune, WebSerial-compatible filesystem access.

#[cfg(feature = "usb_msc_fat")]
use super::fat::{SECTOR_SIZE, TOTAL_SECTORS, VirtualFat};

#[cfg(not(feature = "usb_msc_fat"))]
pub const SECTOR_SIZE: usize = 512;
#[cfg(not(feature = "usb_msc_fat"))]
pub const TOTAL_SECTORS: u32 = 4096;

#[cfg(not(feature = "usb_msc_fat"))]
struct VirtualFat;
#[cfg(not(feature = "usb_msc_fat"))]
impl VirtualFat {
    pub fn read_sector(_lba: u32, buf: &mut [u8; SECTOR_SIZE]) {
        buf.fill(0);
    }
    pub fn write_sector(_lba: u32, _buf: &[u8; SECTOR_SIZE]) -> Result<(), ()> {
        Ok(())
    }
}

use py32_hal::pac::usbd::*;

pub const CBW_SIGNATURE: u32 = 0x43425355; // "USBC"
pub const CSW_SIGNATURE: u32 = 0x53425355; // "USBS"

pub const CSW_STATUS_PASSED: u8 = 0x00;
pub const CSW_STATUS_FAILED: u8 = 0x01;
pub const CSW_STATUS_PHASE_ERROR: u8 = 0x02;

// SCSI Command Codes
pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
pub const SCSI_REQUEST_SENSE: u8 = 0x03;
pub const SCSI_INQUIRY: u8 = 0x12;
pub const SCSI_MODE_SENSE_6: u8 = 0x1A;
pub const SCSI_START_STOP_UNIT: u8 = 0x1B;
pub const SCSI_PREVENT_ALLOW_MEDIUM_REMOVAL: u8 = 0x1E;
pub const SCSI_READ_FORMAT_CAPACITIES: u8 = 0x23;
pub const SCSI_READ_CAPACITY_10: u8 = 0x25;
pub const SCSI_READ_10: u8 = 0x28;
pub const SCSI_WRITE_10: u8 = 0x2A;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MscState {
    Idle,
    DataIn,
    DataOut,
    Status,
}

pub struct MscEngine {
    state: MscState,
    tag: u32,
    transfer_len: u32,
    residue: u32,
    status: u8,
    cur_lba: u32,
    blocks_remaining: u16,
    sector_offset: usize,
    sector_buf: [u8; SECTOR_SIZE],
}

impl MscEngine {
    pub const fn new() -> Self {
        Self {
            state: MscState::Idle,
            tag: 0,
            transfer_len: 0,
            residue: 0,
            status: CSW_STATUS_PASSED,
            cur_lba: 0,
            blocks_remaining: 0,
            sector_offset: 0,
            sector_buf: [0u8; SECTOR_SIZE],
        }
    }

    pub fn state(&self) -> MscState {
        self.state
    }

    /// Process received Bulk OUT packet on EP2
    pub fn handle_out_packet(&mut self, ep: u8, len: usize) {
        if self.state == MscState::Idle {
            if len >= 31 {
                let mut cbw_bytes = [0u8; 31];
                read_fifo(ep, &mut cbw_bytes);
                self.process_cbw(&cbw_bytes);
            }
        } else if self.state == MscState::DataOut {
            // Read incoming write data from host into sector buffer
            let to_read = len.min(SECTOR_SIZE - self.sector_offset);
            read_fifo(
                ep,
                &mut self.sector_buf[self.sector_offset..self.sector_offset + to_read],
            );
            self.sector_offset += to_read;
            self.residue = self.residue.saturating_sub(to_read as u32);

            if self.sector_offset >= SECTOR_SIZE {
                let _ = VirtualFat::write_sector(self.cur_lba, &self.sector_buf);
                self.cur_lba += 1;
                self.blocks_remaining = self.blocks_remaining.saturating_sub(1);
                self.sector_offset = 0;
            }

            if self.blocks_remaining == 0 || self.residue == 0 {
                self.state = MscState::Status;
                self.send_csw();
            }
        }
    }

    /// Handle Bulk IN packet completion on EP1
    pub fn handle_in_packet(&mut self) {
        if self.state == MscState::DataIn {
            if self.blocks_remaining > 0 {
                if self.sector_offset >= SECTOR_SIZE {
                    self.cur_lba += 1;
                    self.blocks_remaining -= 1;
                    self.sector_offset = 0;
                    if self.blocks_remaining > 0 {
                        VirtualFat::read_sector(self.cur_lba, &mut self.sector_buf);
                    }
                }

                if self.blocks_remaining > 0 {
                    let chunk = 64.min(SECTOR_SIZE - self.sector_offset);
                    write_fifo(
                        1,
                        &self.sector_buf[self.sector_offset..self.sector_offset + chunk],
                    );
                    self.sector_offset += chunk;
                    self.residue = self.residue.saturating_sub(chunk as u32);
                    self.arm_in_endpoint(1);
                    return;
                }
            }

            // Transfer complete, transition to CSW status
            self.state = MscState::Status;
            self.send_csw();
        } else if self.state == MscState::Status {
            self.state = MscState::Idle;
        }
    }

    fn process_cbw(&mut self, bytes: &[u8; 31]) {
        let sig = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if sig != CBW_SIGNATURE {
            self.status = CSW_STATUS_PHASE_ERROR;
            self.state = MscState::Status;
            self.send_csw();
            return;
        }

        self.tag = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        self.transfer_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        self.residue = self.transfer_len;
        self.status = CSW_STATUS_PASSED;

        let cmd = bytes[15];
        match cmd {
            SCSI_TEST_UNIT_READY => {
                self.status = CSW_STATUS_PASSED;
                self.state = MscState::Status;
                self.send_csw();
            }

            SCSI_REQUEST_SENSE => {
                let sense_data: [u8; 18] = [
                    0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00,
                ];
                self.send_data(&sense_data);
            }

            SCSI_INQUIRY => {
                let inquiry: [u8; 36] = [
                    0x00, // Direct Access Block Device
                    0x80, // Removable
                    0x02, // ANSI SCSI-2
                    0x02, // Response format
                    31,   // Additional length
                    0x00, 0x00, 0x00, // Vendor Identification (8 bytes)
                    b'K', b'E', b'R', b'N', b'-', b'O', b'S', b' ',
                    // Product Identification (16 bytes)
                    b'R', b'A', b'D', b'I', b'O', b' ', b'D', b'I', b'S', b'K', b' ', b' ', b' ',
                    b' ', b' ', b' ', // Product Revision (4 bytes)
                    b'1', b'.', b'0', b'0',
                ];
                self.send_data(&inquiry);
            }

            SCSI_MODE_SENSE_6 => {
                let mode: [u8; 4] = [0x03, 0x00, 0x00, 0x00];
                self.send_data(&mode);
            }

            SCSI_START_STOP_UNIT | SCSI_PREVENT_ALLOW_MEDIUM_REMOVAL => {
                self.status = CSW_STATUS_PASSED;
                self.state = MscState::Status;
                self.send_csw();
            }

            SCSI_READ_FORMAT_CAPACITIES => {
                let mut cap = [0u8; 12];
                cap[3] = 0x08; // Capacity list length
                // Number of blocks
                cap[4..8].copy_from_slice(&TOTAL_SECTORS.to_be_bytes());
                // Formatted Media (0x02) + Block length 512
                cap[8] = 0x02;
                cap[9] = 0x00;
                cap[10] = (SECTOR_SIZE >> 8) as u8;
                cap[11] = (SECTOR_SIZE & 0xFF) as u8;
                self.send_data(&cap);
            }

            SCSI_READ_CAPACITY_10 => {
                let mut cap = [0u8; 8];
                let last_lba = TOTAL_SECTORS - 1;
                cap[0..4].copy_from_slice(&last_lba.to_be_bytes());
                cap[4..8].copy_from_slice(&(SECTOR_SIZE as u32).to_be_bytes());
                self.send_data(&cap);
            }

            SCSI_READ_10 => {
                let lba = u32::from_be_bytes([bytes[17], bytes[18], bytes[19], bytes[20]]);
                let count = u16::from_be_bytes([bytes[22], bytes[23]]);

                self.cur_lba = lba;
                self.blocks_remaining = count;
                self.sector_offset = 0;
                self.state = MscState::DataIn;

                if count > 0 {
                    VirtualFat::read_sector(self.cur_lba, &mut self.sector_buf);
                    let chunk = 64.min(SECTOR_SIZE);
                    write_fifo(1, &self.sector_buf[..chunk]);
                    self.sector_offset = chunk;
                    self.residue = self.residue.saturating_sub(chunk as u32);
                    self.arm_in_endpoint(1);
                } else {
                    self.state = MscState::Status;
                    self.send_csw();
                }
            }

            SCSI_WRITE_10 => {
                let lba = u32::from_be_bytes([bytes[17], bytes[18], bytes[19], bytes[20]]);
                let count = u16::from_be_bytes([bytes[22], bytes[23]]);

                self.cur_lba = lba;
                self.blocks_remaining = count;
                self.sector_offset = 0;
                self.state = MscState::DataOut;
            }

            _ => {
                self.status = CSW_STATUS_FAILED;
                self.state = MscState::Status;
                self.send_csw();
            }
        }
    }

    fn send_data(&mut self, data: &[u8]) {
        let send_len = data.len().min(self.transfer_len as usize);
        self.blocks_remaining = 0;
        self.sector_offset = 0;
        write_fifo(1, &data[..send_len]);
        self.residue = self.residue.saturating_sub(send_len as u32);
        self.arm_in_endpoint(1);
        self.state = MscState::DataIn;
    }

    fn send_csw(&mut self) {
        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&self.tag.to_le_bytes());
        csw[8..12].copy_from_slice(&self.residue.to_le_bytes());
        csw[12] = self.status;

        write_fifo(1, &csw);
        self.arm_in_endpoint(1);
    }

    fn arm_in_endpoint(&self, ep: u8) {
        set_active_ep(ep);
        unsafe {
            let csr = USBD_IN_CSR1_U8.read_volatile();
            USBD_IN_CSR1_U8.write_volatile(csr | INCSR1_INPKTRDY);
        }
    }
}
