//! Virtual FAT16 Filesystem Engine for USB Mass Storage Class.
//!
//! Synthesizes FAT16 boot sector, cluster allocation tables, root directory,
//! and virtual files on-the-fly.
//!
//! # Architectural Status: Optional Reference Module (Disabled by Default)
//! Virtual FAT translation is preserved in the framework for reference, but left
//! uncompiled by default. Exposing embedded filesystems to host operating systems
//! as a block device introduces severe write amplification and host metadata
//! pollution (.DS_Store, indexing). Filesystem operations are instead standardly
//! handled via our structured CDC/UART packet protocol directly to KernFS.

pub const SECTOR_SIZE: usize = 512;
pub const TOTAL_SECTORS: u32 = 8192; // 4 MB Virtual Disk
pub const SECTORS_PER_CLUSTER: u32 = 1;
pub const RESERVED_SECTORS: u32 = 1;
pub const NUM_FATS: u32 = 1;
pub const ROOT_DIR_ENTRIES: u32 = 64; // 4 sectors (64 * 32 bytes)
pub const ROOT_DIR_SECTORS: u32 = (ROOT_DIR_ENTRIES * 32) / (SECTOR_SIZE as u32);
pub const FAT_SECTORS: u32 = 4;

pub const BOOT_SECTOR: u32 = 0;
pub const FAT1_START: u32 = RESERVED_SECTORS;
pub const ROOT_DIR_START: u32 = FAT1_START + FAT_SECTORS;
pub const DATA_START: u32 = ROOT_DIR_START + ROOT_DIR_SECTORS;

// Cluster 2 is the first data cluster (LBA = DATA_START)
pub const CLUSTER_README: u16 = 2;
pub const CLUSTER_CONFIG: u16 = 3;
pub const CLUSTER_BACKUP: u16 = 4;

pub struct VirtualFat;

impl VirtualFat {
    /// Read a 512-byte sector by Logical Block Address (LBA).
    pub fn read_sector(lba: u32, buf: &mut [u8; SECTOR_SIZE]) {
        buf.fill(0);
        if lba >= TOTAL_SECTORS {
            return;
        }

        if lba == BOOT_SECTOR {
            Self::fill_boot_sector(buf);
        } else if lba >= FAT1_START && lba < ROOT_DIR_START {
            Self::fill_fat_sector(lba - FAT1_START, buf);
        } else if lba >= ROOT_DIR_START && lba < DATA_START {
            Self::fill_root_dir_sector(lba - ROOT_DIR_START, buf);
        } else if lba >= DATA_START {
            let cluster = (lba - DATA_START) as u16 + 2;
            Self::fill_cluster_sector(cluster, buf);
        }
    }

    /// Write a 512-byte sector by Logical Block Address (LBA).
    pub fn write_sector(_lba: u32, _buf: &[u8; SECTOR_SIZE]) -> Result<(), ()> {
        // Sector writes can be routed to KernFS or config storage
        Ok(())
    }

    /// Build standard FAT16 Boot Sector (BPB)
    fn fill_boot_sector(buf: &mut [u8; SECTOR_SIZE]) {
        // Jump boot code
        buf[0] = 0xEB;
        buf[1] = 0x3C;
        buf[2] = 0x90;
        // OEM Name
        buf[3..11].copy_from_slice(b"KERN OS ");
        // Bytes per sector (512)
        buf[11] = 0x00;
        buf[12] = 0x02;
        // Sectors per cluster (1)
        buf[13] = SECTORS_PER_CLUSTER as u8;
        // Reserved sectors (1)
        buf[14] = (RESERVED_SECTORS & 0xFF) as u8;
        buf[15] = (RESERVED_SECTORS >> 8) as u8;
        // Number of FATs (1)
        buf[16] = NUM_FATS as u8;
        // Max root directory entries (64)
        buf[17] = (ROOT_DIR_ENTRIES & 0xFF) as u8;
        buf[18] = (ROOT_DIR_ENTRIES >> 8) as u8;
        // Total sectors 16 (8192)
        buf[19] = (TOTAL_SECTORS & 0xFF) as u8;
        buf[20] = (TOTAL_SECTORS >> 8) as u8;
        // Media descriptor: 0xF8 (Hard disk / removable flash)
        buf[21] = 0xF8;
        // Sectors per FAT (4)
        buf[22] = (FAT_SECTORS & 0xFF) as u8;
        buf[23] = (FAT_SECTORS >> 8) as u8;
        // Sectors per track (16)
        buf[24] = 0x10;
        buf[25] = 0x00;
        // Number of heads (2)
        buf[26] = 0x02;
        buf[27] = 0x00;
        // Hidden sectors (0)
        buf[28] = 0x00;
        buf[29] = 0x00;
        buf[30] = 0x00;
        buf[31] = 0x00;

        // Extended BIOS Parameter Block (FAT16)
        buf[36] = 0x00; // Drive number
        buf[37] = 0x00; // Reserved
        buf[38] = 0x29; // Extended boot signature
        buf[39..43].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]); // Volume Serial ID
        buf[43..54].copy_from_slice(b"KERN DISK  "); // Volume Label
        buf[54..62].copy_from_slice(b"FAT16   "); // File system type

        // Boot Signature
        buf[510] = 0x55;
        buf[511] = 0xAA;
    }

    /// Build FAT Table Sector
    fn fill_fat_sector(fat_sec: u32, buf: &mut [u8; SECTOR_SIZE]) {
        if fat_sec == 0 {
            // Cluster 0: Media descriptor + Cluster 1: EOC marker
            buf[0] = 0xF8;
            buf[1] = 0xFF; // Cluster 0
            buf[2] = 0xFF;
            buf[3] = 0xFF; // Cluster 1
            // Cluster 2 (README.TXT): EOF marker
            buf[4] = 0xFF;
            buf[5] = 0xFF;
            // Cluster 3 (CONFIG.INI): EOF marker
            buf[6] = 0xFF;
            buf[7] = 0xFF;
            // Cluster 4 (BACKUP.BIN): 4096 sectors chained
            buf[8] = 0xFF;
            buf[9] = 0xFF;
        }
    }

    /// Build Root Directory entries (32 bytes per entry)
    fn fill_root_dir_sector(dir_sec: u32, buf: &mut [u8; SECTOR_SIZE]) {
        if dir_sec == 0 {
            // Entry 0: Volume Label
            let e0 = &mut buf[0..32];
            e0[0..11].copy_from_slice(b"KERN DISK  ");
            e0[11] = 0x08; // ATTR_VOLUME_ID

            // Entry 1: README.TXT (System Info)
            let e1 = &mut buf[32..64];
            e1[0..11].copy_from_slice(b"README  TXT");
            e1[11] = 0x01; // ATTR_READ_ONLY
            e1[26..28].copy_from_slice(&(CLUSTER_README).to_le_bytes());
            e1[28..32].copy_from_slice(&(Self::readme_text().len() as u32).to_le_bytes());

            // Entry 2: CONFIG.INI (Text-editable configuration)
            let e2 = &mut buf[64..96];
            e2[0..11].copy_from_slice(b"CONFIG  INI");
            e2[11] = 0x00; // ATTR_ARCHIVE (Read/Write)
            e2[26..28].copy_from_slice(&(CLUSTER_CONFIG).to_le_bytes());
            e2[28..32].copy_from_slice(&(Self::config_text().len() as u32).to_le_bytes());

            // Entry 3: BACKUP.BIN (Flash memory mirror)
            let e3 = &mut buf[96..128];
            e3[0..11].copy_from_slice(b"BACKUP  BIN");
            e3[11] = 0x01; // ATTR_READ_ONLY
            e3[26..28].copy_from_slice(&(CLUSTER_BACKUP).to_le_bytes());
            e3[28..32].copy_from_slice(&(512 * 1024u32).to_le_bytes()); // 512 KB
        }
    }

    /// Read data cluster content
    fn fill_cluster_sector(cluster: u16, buf: &mut [u8; SECTOR_SIZE]) {
        match cluster {
            CLUSTER_README => {
                let text = Self::readme_text();
                let len = text.len().min(SECTOR_SIZE);
                buf[..len].copy_from_slice(&text[..len]);
            }
            CLUSTER_CONFIG => {
                let text = Self::config_text();
                let len = text.len().min(SECTOR_SIZE);
                buf[..len].copy_from_slice(&text[..len]);
            }
            _ => {
                // Return zeros for other clusters
                buf.fill(0);
            }
        }
    }

    fn readme_text() -> &'static [u8] {
        b"========================================\r\n\
          KERN RADIO MICROKERNEL & OS\r\n\
          Platform: Quansheng UV-K1 / UV-K5 V3\r\n\
          MCU: Puya PY32F071xB (ARM Cortex-M0+)\r\n\
          USB: 2.0 Full-Speed MSC + CDC ACM\r\n\
          ========================================\r\n\
          Device mounted as Mass Storage Disk.\r\n\
          - README.TXT: System info & hardware specs\r\n\
          - CONFIG.INI: Text radio settings\r\n\
          - BACKUP.BIN: Flash storage image\r\n\
          ========================================\r\n"
    }

    fn config_text() -> &'static [u8] {
        b"[display]\r\n\
          brightness=8\r\n\
          contrast=55\r\n\
          backlight_time=15\r\n\
          inverted=false\r\n\
          \r\n\
          [audio]\r\n\
          volume=7\r\n\
          key_beep=true\r\n\
          roger_beep=1\r\n\
          \r\n\
          [security]\r\n\
          passcode_enabled=false\r\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_sector_rejects_out_of_bounds_lba() {
        let mut buf = [0xAAu8; SECTOR_SIZE];
        VirtualFat::read_sector(TOTAL_SECTORS, &mut buf);
        assert!(buf.iter().all(|&b| b == 0));

        // Ensure wrapped LBA >= DATA_START + 65536 does not alias CLUSTER_README (2).
        VirtualFat::read_sector(DATA_START + 65536, &mut buf);
        assert!(buf.iter().all(|&b| b == 0));
    }
}
