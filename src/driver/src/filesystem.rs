//! KernFS Filesystem Driver for KERN
//!
//! Provides `StorageService` on top of pure Rust `#![no_std]` `kernfs`.
//! Integrates transparent attributes (Read-Only, System, Hidden, Encrypted) via `FileAttributes`.
//! Implements Ascon-128 stream cipher encryption for `PERM_ENCRYPTED` files.

pub use kernel::service::{
    ATTR_COMPRESSED, ATTR_ENCRYPTED, ATTR_HIDDEN, ATTR_READ_ONLY, ATTR_SYSTEM, ATTR_UNENCRYPTED,
    DirEntryInfo, FileAttributes, StorageError,
};
use kernfs::{BlockDevice, FsError, KernFS};

#[cfg(feature = "secure_fs")]
use crypto::ascon::{Ascon128, AsconPermutation};

#[cfg(feature = "secure_fs")]
const SECURE_NONCE_LEN: usize = 16;

pub struct FileSystemDriver<'a, D: BlockDevice> {
    pub fs: KernFS<'a, D>,
    session_unlocked: bool,
    #[cfg(feature = "secure_fs")]
    enc_key: [u8; 16],
    #[cfg(feature = "secure_fs")]
    key_installed: bool,
    #[cfg(feature = "secure_fs")]
    nonce_ctr: u32,
}

impl<'a, D: BlockDevice> FileSystemDriver<'a, D> {
    /// Initialize a `FileSystemDriver` and its `KernFS` directly in uninitialized
    /// storage to avoid large stack copy temporaries on Cortex-M0+.
    pub unsafe fn init_in_place(dst: *mut Self, dev: &'a mut D) {
        unsafe {
            KernFS::init_in_place(core::ptr::addr_of_mut!((*dst).fs), dev);
            core::ptr::write(
                core::ptr::addr_of_mut!((*dst).session_unlocked),
                !cfg!(feature = "secure_fs"),
            );
            #[cfg(feature = "secure_fs")]
            {
                core::ptr::write_bytes(core::ptr::addr_of_mut!((*dst).enc_key) as *mut u8, 0, 16);
                core::ptr::write(core::ptr::addr_of_mut!((*dst).key_installed), false);
                core::ptr::write(core::ptr::addr_of_mut!((*dst).nonce_ctr), 1);
            }
        }
    }

    pub fn new(fs: KernFS<'a, D>) -> Self {
        Self {
            fs,
            session_unlocked: !cfg!(feature = "secure_fs"),
            #[cfg(feature = "secure_fs")]
            enc_key: [0u8; 16],
            #[cfg(feature = "secure_fs")]
            key_installed: false,
            #[cfg(feature = "secure_fs")]
            nonce_ctr: 1,
        }
    }

    #[cfg(feature = "secure_fs")]
    pub fn set_encryption_key(&mut self, key: [u8; 16]) {
        self.enc_key = key;
        self.key_installed = true;
        self.session_unlocked = true;
    }

    pub fn unlock_session(&mut self) {
        #[cfg(feature = "secure_fs")]
        {
            self.session_unlocked = self.key_installed;
        }
        #[cfg(not(feature = "secure_fs"))]
        {
            self.session_unlocked = true;
        }
    }

    pub fn lock_session(&mut self) {
        self.session_unlocked = false;
        #[cfg(feature = "secure_fs")]
        {
            self.enc_key.fill(0);
            self.key_installed = false;
        }
    }

    pub fn is_session_unlocked(&self) -> bool {
        self.session_unlocked
    }

    pub fn stats(&self) -> kernfs::FsStats {
        self.fs.stats()
    }

    #[inline(never)]
    fn map_error(err: FsError) -> StorageError {
        if err as u8 == FsError::LowPower as u8 {
            return StorageError::IoError;
        }
        // FsError discriminants (0,1,2,4,5,7) match StorageError identically.
        unsafe { core::mem::transmute::<FsError, StorageError>(err) }
    }

    #[inline(never)]
    fn check_writable(&mut self, path: &str) -> Result<u8, StorageError> {
        let perm = self.fs.get_permission(path).unwrap_or(0);
        if (perm & (ATTR_READ_ONLY | ATTR_SYSTEM)) != 0 {
            return Err(StorageError::AccessDenied);
        }
        Ok(perm)
    }

    #[cfg(feature = "secure_fs")]
    fn next_write_nonce(&mut self, path: &str, data: &[u8]) -> [u8; SECURE_NONCE_LEN] {
        self.nonce_ctr = self.nonce_ctr.wrapping_add(1);
        let stats = self.fs.stats();
        let mut perm = AsconPermutation::new();
        perm.x[0] = u64::from_le_bytes([
            self.nonce_ctr as u8,
            (self.nonce_ctr >> 8) as u8,
            (self.nonce_ctr >> 16) as u8,
            (self.nonce_ctr >> 24) as u8,
            stats.wear_cycles as u8,
            (stats.wear_cycles >> 8) as u8,
            stats.used_sectors as u8,
            stats.active_files as u8,
        ]) ^ 0x4E4F_4E43_4531_5F56;
        perm.x[1] = u64::from_le_bytes(self.enc_key[..8].try_into().unwrap())
            ^ (data.len() as u64).rotate_left(17);
        perm.x[4] = u64::from_le_bytes(self.enc_key[8..16].try_into().unwrap());
        for (i, &b) in path.as_bytes().iter().take(16).enumerate() {
            perm.x[2] ^= (b as u64) << ((i & 7) * 8);
        }
        for &b in data.iter().take(16) {
            perm.x[3] = perm.x[3].rotate_left(7) ^ (b as u64);
        }
        perm.permute(12);
        let mut nonce = [0u8; SECURE_NONCE_LEN];
        nonce[..8].copy_from_slice(&perm.x[0].to_le_bytes());
        nonce[8..16].copy_from_slice(&perm.x[1].to_le_bytes());
        nonce
    }
}

impl<'a, D: BlockDevice> FileSystemDriver<'a, D> {
    pub fn exists(&mut self, path: &str) -> bool {
        self.fs.exists(path)
    }

    pub fn remove(&mut self, path: &str) -> Result<(), StorageError> {
        self.check_writable(path)?;
        self.fs.remove(path).map_err(Self::map_error)
    }

    pub fn stat_info(&mut self, path: &str) -> Result<(usize, bool), StorageError> {
        self.fs.stat_info(path).map_err(Self::map_error)
    }

    pub fn file_size(&mut self, path: &str) -> Result<usize, StorageError> {
        self.fs.file_size(path).map_err(Self::map_error)
    }

    pub fn create_dir(&mut self, path: &str) -> Result<(), StorageError> {
        self.fs.create_dir(path).map_err(Self::map_error)
    }

    pub fn read_struct<T: Copy>(&mut self, path: &str, out: &mut T) -> Result<usize, StorageError> {
        let size = core::mem::size_of::<T>();
        let slice = unsafe { core::slice::from_raw_parts_mut(out as *mut T as *mut u8, size) };
        self.read_file(path, slice)
    }

    pub fn write_struct<T: Copy>(&mut self, path: &str, data: &T) -> Result<(), StorageError> {
        let size = core::mem::size_of::<T>();
        let slice = unsafe { core::slice::from_raw_parts(data as *const T as *const u8, size) };
        self.write_file(path, slice)
    }

    pub fn read_file(&mut self, path: &str, buf: &mut [u8]) -> Result<usize, StorageError> {
        #[cfg(feature = "secure_fs")]
        {
            let perm = self.fs.get_permission(path).unwrap_or(0);
            if (perm & ATTR_ENCRYPTED) != 0 {
                if !self.session_unlocked {
                    return Err(StorageError::EncryptedNoKey);
                }
                let mut raw = [0u8; 512 + SECURE_NONCE_LEN];
                let raw_len = self.fs.read_file(path, &mut raw).map_err(Self::map_error)?;
                if raw_len < SECURE_NONCE_LEN {
                    return Err(StorageError::CorruptedData);
                }
                let (nonce_slice, ct) = raw[..raw_len].split_at_mut(SECURE_NONCE_LEN);
                let mut nonce = [0u8; SECURE_NONCE_LEN];
                nonce.copy_from_slice(nonce_slice);
                Ascon128::new(self.enc_key).apply_keystream(&nonce, ct);
                let copy_len = ct.len().min(buf.len());
                buf[..copy_len].copy_from_slice(&ct[..copy_len]);
                return Ok(copy_len);
            }
            self.fs.read_file(path, buf).map_err(Self::map_error)
        }
        #[cfg(not(feature = "secure_fs"))]
        {
            self.read_file_at(path, 0, buf)
        }
    }

    pub fn read_file_at(
        &mut self,
        path: &str,
        offset: usize,
        buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        let perm = self.fs.get_permission(path).unwrap_or(0);
        if (perm & ATTR_ENCRYPTED) != 0 {
            return Err(StorageError::EncryptedNoKey);
        }
        self.fs
            .read_file_at(path, offset, buf)
            .map_err(Self::map_error)
    }

    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let perm = self.check_writable(path)?;
        if (perm & ATTR_ENCRYPTED) != 0 {
            #[cfg(feature = "secure_fs")]
            {
                if !self.session_unlocked {
                    return Err(StorageError::EncryptedNoKey);
                }
                if data.len() > 512 {
                    return Err(StorageError::CapacityFull);
                }
                let mut tmp_buf = [0u8; 512 + SECURE_NONCE_LEN];
                let nonce = self.next_write_nonce(path, data);
                tmp_buf[..SECURE_NONCE_LEN].copy_from_slice(&nonce);
                tmp_buf[SECURE_NONCE_LEN..SECURE_NONCE_LEN + data.len()].copy_from_slice(data);
                Ascon128::new(self.enc_key).apply_keystream(
                    &nonce,
                    &mut tmp_buf[SECURE_NONCE_LEN..SECURE_NONCE_LEN + data.len()],
                );
                return self
                    .fs
                    .write_file(path, &tmp_buf[..SECURE_NONCE_LEN + data.len()])
                    .map_err(Self::map_error);
            }
            #[cfg(not(feature = "secure_fs"))]
            {
                return Err(StorageError::EncryptedNoKey);
            }
        }

        self.fs.write_file(path, data).map_err(Self::map_error)
    }

    pub fn write_ring(
        &mut self,
        path: &str,
        max_bytes: usize,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let perm = self.check_writable(path)?;
        if (perm & ATTR_ENCRYPTED) != 0 {
            return Err(StorageError::EncryptedNoKey);
        }
        self.fs
            .write_ring(path, max_bytes, data)
            .map_err(Self::map_error)
    }

    pub fn get_attributes(&mut self, path: &str) -> Result<FileAttributes, StorageError> {
        let perm = self.fs.get_permission(path).map_err(Self::map_error)?;
        Ok(FileAttributes { permission: perm })
    }

    pub fn set_attributes(
        &mut self,
        path: &str,
        attrs: FileAttributes,
    ) -> Result<(), StorageError> {
        let cur_perm = self.fs.get_permission(path).unwrap_or(0);
        if (cur_perm & ATTR_ENCRYPTED) != 0
            && (attrs.permission & ATTR_ENCRYPTED) == 0
            && !self.session_unlocked
        {
            return Err(StorageError::EncryptedNoKey);
        }
        self.fs
            .set_permission(path, attrs.permission)
            .map_err(Self::map_error)
    }

    pub fn count_encrypted_in_dir(&mut self, dir_path: &str) -> usize {
        let mut count = 0usize;
        let _ = self.fs.list_dir(dir_path, |_path, _size, perm, is_dir| {
            if !is_dir && (perm & ATTR_ENCRYPTED) != 0 {
                count += 1;
            }
        });
        count
    }

    pub fn list_dir(
        &mut self,
        dir_path: &str,
        out_entries: &mut [DirEntryInfo],
    ) -> Result<usize, StorageError> {
        let mut count = 0;
        self.fs
            .list_dir(dir_path, |path, size, perm, is_dir| {
                if count < out_entries.len() {
                    let entry = &mut out_entries[count];
                    entry.path.clear();
                    let _ = entry.path.push_str(path);
                    entry.size = size;
                    entry.is_dir = is_dir;
                    entry.permission = perm;
                    count += 1;
                }
            })
            .map_err(Self::map_error)?;
        Ok(count)
    }

    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), StorageError> {
        let perm = self.fs.get_permission(old_path).map_err(Self::map_error)?;
        if (perm & (ATTR_SYSTEM | ATTR_READ_ONLY)) != 0
            || old_path.starts_with("/sys/calib.")
            || new_path.starts_with("/sys/calib.")
        {
            return Err(StorageError::AccessDenied);
        }
        self.check_writable(new_path)?;

        #[cfg(feature = "secure_fs")]
        if (perm & ATTR_ENCRYPTED) != 0 {
            let mut buf = [0u8; 512];
            let len = self.read_file(old_path, &mut buf)?;
            self.write_file(new_path, &buf[..len])?;
            let _ = self.set_attributes(new_path, FileAttributes { permission: perm });
            self.remove(old_path)?;
            return Ok(());
        }

        self.fs.rename(old_path, new_path).map_err(Self::map_error)
    }
}

impl<'a, D: BlockDevice> kernel::service::StorageService for FileSystemDriver<'a, D> {
    fn exists(&mut self, path: &str) -> bool {
        self.exists(path)
    }
    fn file_size(&mut self, path: &str) -> Result<usize, StorageError> {
        self.file_size(path)
    }
    fn remove(&mut self, path: &str) -> Result<(), StorageError> {
        self.remove(path)
    }
    fn create_dir(&mut self, path: &str) -> Result<(), StorageError> {
        self.create_dir(path)
    }
    fn read_file(&mut self, path: &str, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.read_file(path, buf)
    }
    fn read_file_at(
        &mut self,
        path: &str,
        offset: usize,
        buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        self.read_file_at(path, offset, buf)
    }
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.write_file(path, data)
    }
    fn get_attributes(&mut self, path: &str) -> Result<FileAttributes, StorageError> {
        self.get_attributes(path)
    }
    fn set_attributes(&mut self, path: &str, attrs: FileAttributes) -> Result<(), StorageError> {
        self.set_attributes(path, attrs)
    }
    fn list_dir(
        &mut self,
        dir_path: &str,
        out_entries: &mut [DirEntryInfo],
    ) -> Result<usize, StorageError> {
        self.list_dir(dir_path, out_entries)
    }
    fn stats(&mut self) -> Option<kernel::service::StorageStats> {
        let s = self.fs.stats();
        Some(kernel::service::StorageStats {
            active_files: s.active_files,
            used_sectors: s.used_sectors,
            free_sectors: s.free_sectors,
            deleted_sectors: s.deleted_sectors,
            wear_cycles: s.wear_cycles,
            total_sectors: s.total_sectors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemFlash {
        sectors: [[u8; 4096]; 16],
    }

    impl MemFlash {
        fn new() -> Self {
            Self {
                sectors: [[0xFFu8; 4096]; 16],
            }
        }
    }

    impl BlockDevice for MemFlash {
        const TOTAL_BLOCKS: usize = 16;

        fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), FsError> {
            let s = offset / 4096;
            let o = offset % 4096;
            if s >= Self::TOTAL_BLOCKS || o + buf.len() > 4096 {
                return Err(FsError::IoError);
            }
            buf.copy_from_slice(&self.sectors[s][o..o + buf.len()]);
            Ok(())
        }

        fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), FsError> {
            let s = offset / 4096;
            let o = offset % 4096;
            if s >= Self::TOTAL_BLOCKS || o + data.len() > 4096 {
                return Err(FsError::IoError);
            }
            for (dst, &src) in self.sectors[s][o..o + data.len()].iter_mut().zip(data) {
                *dst &= src;
            }
            Ok(())
        }

        fn erase_sector(&mut self, sector_addr: usize) -> Result<(), FsError> {
            let s = sector_addr / 4096;
            if s >= Self::TOTAL_BLOCKS {
                return Err(FsError::IoError);
            }
            self.sectors[s].fill(0xFF);
            Ok(())
        }
    }

    #[test]
    fn sanitize_path_enforced_across_all_fs_ops() {
        let mut flash = MemFlash::new();
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();
        let mut drv = FileSystemDriver::new(fs);

        for bad in [
            "",
            "relative",
            "/user/../sys",
            "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p",
        ] {
            assert!(!drv.exists(bad));
            assert_eq!(drv.remove(bad), Err(StorageError::InvalidPath));
            assert_eq!(drv.file_size(bad), Err(StorageError::InvalidPath));
            assert_eq!(drv.create_dir(bad), Err(StorageError::InvalidPath));
            assert_eq!(drv.get_attributes(bad), Err(StorageError::InvalidPath));
            assert_eq!(
                drv.set_attributes(bad, FileAttributes { permission: 0 }),
                Err(StorageError::InvalidPath)
            );
            let mut entries = [const { DirEntryInfo::empty() }; 2];
            assert_eq!(
                drv.list_dir(bad, &mut entries),
                Err(StorageError::InvalidPath)
            );
        }

        let mut entries = [const { DirEntryInfo::empty() }; 2];
        assert!(drv.list_dir("/", &mut entries).is_ok());
    }

    #[test]
    fn write_ring_respects_protected_and_encrypted_flags() {
        let mut flash = MemFlash::new();
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();
        let mut drv = FileSystemDriver::new(fs);

        drv.write_file("/ro.log", b"abc").unwrap();
        drv.set_attributes(
            "/ro.log",
            FileAttributes {
                permission: ATTR_READ_ONLY,
            },
        )
        .unwrap();
        assert_eq!(
            drv.write_ring("/ro.log", 64, b"def"),
            Err(StorageError::AccessDenied)
        );

        drv.write_file("/sys.log", b"abc").unwrap();
        drv.set_attributes(
            "/sys.log",
            FileAttributes {
                permission: ATTR_SYSTEM,
            },
        )
        .unwrap();
        assert_eq!(
            drv.write_ring("/sys.log", 64, b"def"),
            Err(StorageError::AccessDenied)
        );

        drv.set_attributes(
            "/ro.log",
            FileAttributes {
                permission: ATTR_ENCRYPTED,
            },
        )
        .unwrap();
        assert_eq!(
            drv.write_ring("/ro.log", 64, b"def"),
            Err(StorageError::EncryptedNoKey)
        );
    }

    #[cfg(feature = "secure_fs")]
    #[test]
    fn secure_fs_uses_unique_nonces_per_write() {
        let mut flash = MemFlash::new();
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();
        let mut drv = FileSystemDriver::new(fs);
        drv.set_encryption_key([0x5Au8; 16]);

        drv.write_file("/secret", b"initial").unwrap();
        drv.set_attributes(
            "/secret",
            FileAttributes {
                permission: ATTR_ENCRYPTED,
            },
        )
        .unwrap();

        let msg = b"top-secret-payload";
        drv.write_file("/secret", msg).unwrap();
        let mut raw1 = [0u8; 64];
        let n1 = drv.fs.read_file("/secret", &mut raw1).unwrap();

        drv.write_file("/secret", msg).unwrap();
        let mut raw2 = [0u8; 64];
        let n2 = drv.fs.read_file("/secret", &mut raw2).unwrap();

        assert_eq!(n1, SECURE_NONCE_LEN + msg.len());
        assert_eq!(n2, n1);
        assert_ne!(&raw1[..n1], &raw2[..n2]);

        let mut dec = [0u8; 64];
        let dec_len = drv.read_file("/secret", &mut dec).unwrap();
        assert_eq!(&dec[..dec_len], msg);
    }

    #[cfg(feature = "secure_fs")]
    #[test]
    fn secure_fs_requires_key_and_zeroizes_on_lock() {
        let mut flash = MemFlash::new();
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();
        let mut drv = FileSystemDriver::new(fs);

        assert!(!drv.is_session_unlocked());
        drv.unlock_session();
        assert!(!drv.is_session_unlocked());

        drv.write_file("/secret", b"initial").unwrap();
        drv.set_attributes(
            "/secret",
            FileAttributes {
                permission: ATTR_ENCRYPTED,
            },
        )
        .unwrap();

        let mut buf = [0u8; 32];
        assert_eq!(
            drv.read_file("/secret", &mut buf),
            Err(StorageError::EncryptedNoKey)
        );
        assert_eq!(
            drv.write_file("/secret", b"blocked"),
            Err(StorageError::EncryptedNoKey)
        );

        drv.set_encryption_key([0x3Cu8; 16]);
        assert!(drv.is_session_unlocked());
        drv.write_file("/secret", b"sealed-ok").unwrap();
        let n = drv.read_file("/secret", &mut buf).unwrap();
        assert_eq!(&buf[..n], b"sealed-ok");

        drv.lock_session();
        assert!(!drv.is_session_unlocked());
        drv.unlock_session();
        assert!(!drv.is_session_unlocked());
        assert_eq!(
            drv.read_file("/secret", &mut buf),
            Err(StorageError::EncryptedNoKey)
        );
    }

    #[test]
    fn test_rename_blocks_protected_destination() {
        let mut flash = MemFlash::new();
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();
        let mut drv = FileSystemDriver::new(fs);

        drv.write_file("/sys/calib.dat", b"factory").unwrap();
        drv.write_file("/sys/locked.bin", b"locked").unwrap();
        drv.set_attributes(
            "/sys/locked.bin",
            FileAttributes {
                permission: ATTR_SYSTEM | ATTR_READ_ONLY,
            },
        )
        .unwrap();
        drv.write_file("/tmp.dat", b"evil").unwrap();

        assert_eq!(
            drv.rename("/tmp.dat", "/sys/calib.dat"),
            Err(StorageError::AccessDenied)
        );
        assert_eq!(
            drv.rename("/tmp.dat", "/sys/locked.bin"),
            Err(StorageError::AccessDenied)
        );

        let mut buf = [0u8; 16];
        let n = drv.read_file("/sys/calib.dat", &mut buf).unwrap();
        assert_eq!(&buf[..n], b"factory");
        let n2 = drv.read_file("/sys/locked.bin", &mut buf).unwrap();
        assert_eq!(&buf[..n2], b"locked");
    }
}
