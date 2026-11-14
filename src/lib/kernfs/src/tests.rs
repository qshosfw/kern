//! Host Test Suite for KernFS v2 (Power-Cut Fuzz, Wear Leveling, Large Files, Directories, Bad Blocks).

extern crate std;
use std::vec;
use std::vec::Vec;

use crate::device::{BlockDevice, FsError};
use crate::format::BlockHeader;
use crate::fs::KernFS;
use crypto::crc::CrcEngine;

struct SmallFlash {
    data: [u8; 16 * 4096],
    program_ops: usize,
    fail_on_op: Option<usize>,
    fail_erase_block: Option<usize>,
}

impl SmallFlash {
    fn new() -> Self {
        Self {
            data: [0xFF; 16 * 4096],
            program_ops: 0,
            fail_on_op: None,
            fail_erase_block: None,
        }
    }
}

impl BlockDevice for SmallFlash {
    const TOTAL_BLOCKS: usize = 16;

    fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), FsError> {
        if offset + buf.len() > self.data.len() {
            return Err(FsError::IoError);
        }
        buf.copy_from_slice(&self.data[offset..offset + buf.len()]);
        Ok(())
    }

    fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), FsError> {
        if offset + data.len() > self.data.len() {
            return Err(FsError::IoError);
        }
        self.program_ops += 1;
        if let Some(cut) = self.fail_on_op
            && self.program_ops >= cut
        {
            return Err(FsError::IoError);
        }
        for (i, &b) in data.iter().enumerate() {
            self.data[offset + i] &= b;
        }
        Ok(())
    }

    fn erase_sector(&mut self, sector_addr: usize) -> Result<(), FsError> {
        let block = sector_addr / Self::BLOCK_SIZE;
        if self.fail_erase_block == Some(block) {
            return Err(FsError::IoError);
        }
        let start = sector_addr;
        let end = start + Self::BLOCK_SIZE;
        if end > self.data.len() {
            return Err(FsError::IoError);
        }
        self.data[start..end].fill(0xFF);
        Ok(())
    }
}

struct FullFlash {
    data: Vec<u8>,
}

impl FullFlash {
    fn new() -> Self {
        Self {
            data: vec![0xFF; 512 * 4096],
        }
    }
}

impl BlockDevice for FullFlash {
    const TOTAL_BLOCKS: usize = 512;

    fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), FsError> {
        if offset + buf.len() > self.data.len() {
            return Err(FsError::IoError);
        }
        buf.copy_from_slice(&self.data[offset..offset + buf.len()]);
        Ok(())
    }

    fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), FsError> {
        if offset + data.len() > self.data.len() {
            return Err(FsError::IoError);
        }
        for (i, &b) in data.iter().enumerate() {
            self.data[offset + i] &= b;
        }
        Ok(())
    }

    fn erase_sector(&mut self, sector_addr: usize) -> Result<(), FsError> {
        let start = sector_addr;
        let end = start + Self::BLOCK_SIZE;
        if end > self.data.len() {
            return Err(FsError::IoError);
        }
        self.data[start..end].fill(0xFF);
        Ok(())
    }
}

#[test]
fn test_inline_direct_indirect_and_read_file_at() {
    let mut flash = SmallFlash::new();
    let mut fs = KernFS::new(&mut flash);
    fs.format(None).unwrap();

    // 1. Inline (<= 16 B, 0 data pages)
    fs.write_file("/pref/small.cfg", b"hello_inline").unwrap();
    let mut out = [0u8; 32];
    let n = fs
        .read_file_at("/pref/small.cfg", 6, &mut out[..6])
        .unwrap();
    assert_eq!(&out[..n], b"inline");

    // 2. Direct (256 B) + partial CoW write_at
    let mut big = [0u8; 1024];
    for (i, b) in big.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(7).wrapping_add(3);
    }
    fs.write_file("/user/channels.ch", &big).unwrap();
    fs.write_at("/user/channels.ch", 300, b"PATCH").unwrap();

    let mut read_back = [0u8; 1024];
    assert_eq!(
        fs.read_file("/user/channels.ch", &mut read_back).unwrap(),
        1024
    );
    assert_eq!(&read_back[300..305], b"PATCH");
    assert_eq!(&read_back[..300], &big[..300]);
    assert_eq!(&read_back[305..], &big[305..]);

    // Remount preserves all files across reboot
    let mut fs2 = KernFS::new(&mut flash);
    fs2.mount().unwrap();
    let mut s_out = [0u8; 16];
    assert_eq!(fs2.read_file("/pref/small.cfg", &mut s_out).unwrap(), 12);
    assert_eq!(&s_out[..12], b"hello_inline");
    assert_eq!(
        fs2.read_file("/user/channels.ch", &mut read_back).unwrap(),
        1024
    );
    assert_eq!(&read_back[300..305], b"PATCH");
}

#[test]
fn test_directories_metadata_rename_and_remove() {
    let mut flash = SmallFlash::new();
    let mut fs = KernFS::new(&mut flash);
    fs.format(None).unwrap();

    fs.create_dir("/user/vault").unwrap();
    fs.write_file("/user/vault/note.txt", b"secret_note_payload_123456")
        .unwrap();
    fs.set_permission("/user/vault/note.txt", 0x08).unwrap();

    fs.rename("/user/vault/note.txt", "/user/renamed.txt")
        .unwrap();
    assert!(!fs.exists("/user/vault/note.txt"));
    assert_eq!(fs.get_permission("/user/renamed.txt").unwrap(), 0x08);

    let mut items = 0usize;
    fs.list_dir("/user", |_name, _sz, _perm, _dir| items += 1)
        .unwrap();
    assert_eq!(items, 2); // vault dir + renamed.txt

    // Non-empty dir delete denied; empty dir delete succeeds
    fs.write_file("/user/vault/x", b"1").unwrap();
    assert_eq!(fs.remove("/user/vault"), Err(FsError::AccessDenied));
    fs.remove("/user/vault/x").unwrap();
    fs.remove("/user/vault").unwrap();
}

#[test]
fn test_power_cut_fuzz_every_program_op() {
    let mut base = SmallFlash::new();
    {
        let mut fs = KernFS::new(&mut base);
        fs.format(None).unwrap();
        fs.write_file("/pref/a.cfg", b"v1_payload_keep_safe")
            .unwrap();
        fs.write_file("/user/b.dat", &[0xA5; 600]).unwrap();
    }

    // Determine how many program ops an update takes
    let op_count = {
        let mut probe = SmallFlash::new();
        probe.data = base.data;
        {
            let mut fs = KernFS::new(&mut probe);
            fs.mount().unwrap();
            fs.dev.program_ops = 0;
            fs.write_file("/pref/a.cfg", b"v2_new_payload_value!")
                .unwrap();
        }
        probe.program_ops
    };
    assert!(op_count >= 3);

    // Cut power after EVERY possible program op from 1..=op_count and verify volume always mounts with valid v1 OR v2!
    for cut_at in 1..=op_count {
        let mut flash = SmallFlash::new();
        flash.data = base.data;
        flash.fail_on_op = Some(cut_at);
        {
            let mut fs = KernFS::new(&mut flash);
            fs.mount().unwrap();
            let _ = fs.write_file("/pref/a.cfg", b"v2_new_payload_value!");
        }
        flash.fail_on_op = None;
        let mut fs2 = KernFS::new(&mut flash);
        fs2.mount().unwrap();
        let mut out = [0u8; 64];
        let n = fs2.read_file("/pref/a.cfg", &mut out).unwrap();
        let got = &out[..n];
        assert!(
            got == b"v1_payload_keep_safe" || got == b"v2_new_payload_value!",
            "power cut at op {cut_at} corrupted /pref/a.cfg: {:?}",
            got
        );
        // Untouched file must also stay 100% intact
        let mut bout = [0u8; 600];
        assert_eq!(fs2.read_file("/user/b.dat", &mut bout).unwrap(), 600);
        assert!(bout.iter().all(|&b| b == 0xA5));
    }
}

#[test]
fn test_format_resurrection_prevented_by_epoch() {
    let mut flash = SmallFlash::new();
    {
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();
        for _ in 0..10 {
            fs.write_file("/user/ghost.txt", b"old_epoch_data").unwrap();
        }
    }
    let mut fs = KernFS::new(&mut flash);
    fs.format(None).unwrap();
    assert!(!fs.exists("/user/ghost.txt"));
    let mut fs2 = KernFS::new(&mut flash);
    fs2.mount().unwrap();
    assert!(!fs2.exists("/user/ghost.txt"));
}

#[test]
fn test_bit_rot_detection_on_data_page() {
    let mut flash = SmallFlash::new();
    {
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();
        let payload = [0x5Au8; 300];
        fs.write_file("/user/rot.bin", &payload).unwrap();
    }

    // Flip a bit in a committed data page payload on flash
    let mut corrupted = false;
    for b in 2..16 {
        let base = b * 4096;
        if &flash.data[base..base + 4] == b"K2BL" {
            for s in 1..=15 {
                let tag_off = base + s * 16;
                if flash.data[tag_off + 8] == 0x3F && flash.data[tag_off + 9] == 1 {
                    flash.data[base + s * 256 + 10] ^= 0x01;
                    corrupted = true;
                    break;
                }
            }
        }
        if corrupted {
            break;
        }
    }
    assert!(corrupted);

    let mut fs = KernFS::new(&mut flash);
    fs.mount().unwrap();
    let mut out = [0u8; 300];
    assert_eq!(
        fs.read_file("/user/rot.bin", &mut out),
        Err(FsError::CorruptedData)
    );
}

#[test]
fn test_bad_block_retirement_and_append_ring_log() {
    let mut flash = SmallFlash::new();
    {
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();

        // Test append and cyclic ring log
        fs.append_file("/log/events.log", b"ev1:").unwrap();
        fs.append_file("/log/events.log", b"ev2:").unwrap();
        let mut buf = [0u8; 16];
        let n = fs.read_file("/log/events.log", &mut buf).unwrap();
        assert_eq!(&buf[..n], b"ev1:ev2:");

        for i in 0..8u8 {
            fs.write_ring("/log/ring.bin", 300, &[i; 64]).unwrap();
        }
        assert!(fs.file_size("/log/ring.bin").unwrap() <= 300);
    }

    // Inject erase failure on block 5 during GC/writes
    flash.fail_erase_block = Some(5);
    let mut fs = KernFS::new(&mut flash);
    fs.mount().unwrap();
    for i in 0..40u8 {
        let _ = fs.write_file("/user/churn.dat", &[i; 300]);
    }
    // Filesystem remains usable and intact
    fs.write_file("/user/churn.dat", b"survives_bad_block_payload")
        .unwrap();
    let mut check = [0u8; 32];
    let n = fs.read_file("/user/churn.dat", &mut check).unwrap();
    assert_eq!(&check[..n], b"survives_bad_block_payload");
}

#[test]
fn test_wear_leveling_ratio_under_heavy_rewrites() {
    let mut flash = SmallFlash::new();
    let mut fs = KernFS::new(&mut flash);
    fs.format(None).unwrap();

    for i in 0..300u32 {
        fs.write_file("/pref/churn.bin", &[i as u8; 512]).unwrap();
    }

    let mut min_ec = u32::MAX;
    let mut max_ec = 0u32;
    for b in 2..16 {
        let hdr = BlockHeader::from_bytes(&flash.data[b * 4096..b * 4096 + 16]);
        min_ec = min_ec.min(hdr.erase_count);
        max_ec = max_ec.max(hdr.erase_count);
    }
    assert!(min_ec >= 1);
    assert!(
        max_ec * 2 <= min_ec * 3 + 3,
        "wear spread too wide: min={min_ec}, max={max_ec}"
    );
}

#[test]
fn test_large_volume_1000_files_and_double_indirect_file() {
    let mut flash = FullFlash::new();
    let mut fs = KernFS::new(&mut flash);
    fs.format(None).unwrap();

    // Write a 96 KB double-indirect file (> 32 KB single indirect limit)
    let mut large = [0u8; 96 * 1024];
    for (i, b) in large.iter_mut().enumerate() {
        *b = (i ^ (i >> 8)) as u8;
    }
    fs.write_file("/apps/waterfall.krx", &large).unwrap();

    let mut verify = [0u8; 96 * 1024];
    assert_eq!(
        fs.read_file("/apps/waterfall.krx", &mut verify).unwrap(),
        large.len()
    );
    assert_eq!(CrcEngine::crc32(&verify), CrcEngine::crc32(&large));

    // Create 1,000 files across chained MetaIdx pages
    for i in 0..1000usize {
        let mut path = [0u8; 16];
        path[..7].copy_from_slice(b"/dir/f_");
        path[7] = b'0' + ((i / 100) % 10) as u8;
        path[8] = b'0' + ((i / 10) % 10) as u8;
        path[9] = b'0' + (i % 10) as u8;
        let p = core::str::from_utf8(&path[..10]).unwrap();
        fs.write_file(p, &[(i & 0xFF) as u8; 8]).unwrap();
    }

    let mut listed = 0usize;
    let returned = fs.list_dir("/dir", |_, _, _, _| listed += 1).unwrap();
    assert_eq!(listed, 1000);
    assert_eq!(returned, 1000);

    // Remount and verify 1000th file + 96 KB double-indirect file
    let mut fs2 = KernFS::new(&mut flash);
    fs2.mount().unwrap();
    let mut tiny = [0u8; 8];
    assert_eq!(fs2.read_file("/dir/f_999", &mut tiny).unwrap(), 8);
    assert_eq!(tiny, [(999usize & 0xFF) as u8; 8]);
    assert_eq!(
        fs2.read_file("/apps/waterfall.krx", &mut verify).unwrap(),
        large.len()
    );
    assert_eq!(CrcEngine::crc32(&verify), CrcEngine::crc32(&large));
}

#[test]
fn test_list_dir_returns_exact_count_and_nested_dir() {
    let mut flash = SmallFlash::new();
    let mut fs = KernFS::new(&mut flash);
    fs.format(None).unwrap();
    fs.write_file("/user/splash/demo_01.rle", b"RLE1").unwrap();
    fs.write_file("/user/splash/second.img", b"IMG2").unwrap();
    fs.write_file("/user/splash/deleted.rle", b"DEL3").unwrap();
    fs.remove("/user/splash/deleted.rle").unwrap();

    let mut seen = 0usize;
    let count = fs
        .list_dir("/user/splash", |_name, _sz, _perm, is_dir| {
            assert!(!is_dir);
            seen += 1;
        })
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(seen, 2);
}
