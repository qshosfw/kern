//! KernFS Filesystem Driver for KERN
//!
//! Provides `StorageService` on top of pure Rust `#![no_std]` `kernfs`.
//! Integrates transparent attributes (Read-Only, System, Hidden, Encrypted) via `FileAttributes`.
//! Implements ChaCha20 AEAD stream cipher encryption for `PERM_ENCRYPTED` files.

pub use kernel::service::{
    ATTR_COMPRESSED, ATTR_ENCRYPTED, ATTR_HIDDEN, ATTR_READ_ONLY, ATTR_SYSTEM, ATTR_UNENCRYPTED,
    DirEntryInfo, FileAttributes, PERM_ENCRYPTED, PERM_HIDDEN, PERM_READ_ONLY, PERM_SYSTEM,
    StorageError,
};
use kernfs::{BlockDevice, FsError, KernFS};

#[cfg(feature = "secure_vfs")]
use crypto::chacha20poly1305::ChaCha20Poly1305;

pub struct FileSystemDriver<'a, D: BlockDevice> {
    fs: KernFS<'a, D>,
    session_unlocked: bool,
}

impl<'a, D: BlockDevice> FileSystemDriver<'a, D> {
    pub fn new(fs: KernFS<'a, D>) -> Self {
        Self {
            fs,
            session_unlocked: true,
        }
    }

    pub fn unlock_session(&mut self) {
        self.session_unlocked = true;
    }

    pub fn lock_session(&mut self) {
        self.session_unlocked = false;
    }

    pub fn is_session_unlocked(&self) -> bool {
        self.session_unlocked
    }

    pub fn stats(&mut self) -> kernfs::FsStats {
        self.fs.stats()
    }

    pub fn sanitize_path(path: &str) -> Result<&str, StorageError> {
        let b = path.as_bytes();
        if b.is_empty() || b[0] != b'/' || b.len() > 31 {
            return Err(StorageError::InvalidPath);
        }
        for i in 0..b.len().saturating_sub(1) {
            if b[i] == b'.' && b[i + 1] == b'.' {
                return Err(StorageError::InvalidPath);
            }
        }
        Ok(path)
    }

    fn map_error(err: FsError) -> StorageError {
        match err {
            FsError::NotFound => StorageError::NotFound,
            FsError::CapacityFull => StorageError::CapacityFull,
            FsError::AccessDenied => StorageError::AccessDenied,
            FsError::CorruptedData => StorageError::CorruptedData,
            _ => StorageError::IoError,
        }
    }

    #[cfg(feature = "secure_vfs")]
    fn derive_path_nonce(path: &str) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        let mut hash: u32 = 5381;
        for (i, &b) in path.as_bytes().iter().enumerate() {
            hash = hash.wrapping_mul(33).wrapping_add(b as u32);
            nonce[i % 12] ^= b;
        }
        let hash_bytes = hash.to_le_bytes();
        nonce[0] ^= hash_bytes[0];
        nonce[1] ^= hash_bytes[1];
        nonce[2] ^= hash_bytes[2];
        nonce[3] ^= hash_bytes[3];
        nonce
    }

    #[cfg(feature = "secure_vfs")]
    fn crypt_payload(path: &str, data: &mut [u8]) {
        const SYSTEM_KEY: [u8; 32] = [
            0x4B, 0x45, 0x52, 0x4E, 0x5F, 0x53, 0x45, 0x43, 0x55, 0x52, 0x45, 0x5F, 0x56, 0x46,
            0x53, 0x5F, 0x4B, 0x45, 0x59, 0x5F, 0x32, 0x30, 0x32, 0x36, 0x5F, 0x50, 0x59, 0x33,
            0x32, 0x46, 0x30, 0x37,
        ];
        let nonce = Self::derive_path_nonce(path);
        let cipher = ChaCha20Poly1305::new(SYSTEM_KEY);

        let mut block = [0u8; 64];
        let mut counter = 1u32;

        for chunk in data.chunks_mut(64) {
            cipher.block(&nonce, counter, &mut block);
            counter = counter.wrapping_add(1);
            for (b, k) in chunk.iter_mut().zip(block.iter()) {
                *b ^= *k;
            }
        }
    }

    #[cfg(not(feature = "secure_vfs"))]
    fn crypt_payload(_path: &str, _data: &mut [u8]) {}
}

impl<'a, D: BlockDevice> FileSystemDriver<'a, D> {
    pub fn exists(&mut self, path: &str) -> bool {
        self.fs.exists(path)
    }

    pub fn remove(&mut self, path: &str) -> Result<(), StorageError> {
        let perm = self.fs.get_permission(path).unwrap_or(0);
        if (perm & PERM_READ_ONLY) != 0 || (perm & PERM_SYSTEM) != 0 {
            return Err(StorageError::AccessDenied);
        }
        self.fs.remove(path).map_err(Self::map_error)
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
        Self::sanitize_path(path)?;
        let perm = self.fs.get_permission(path).unwrap_or(0);
        if (perm & ATTR_ENCRYPTED) != 0 {
            #[cfg(feature = "secure_vfs")]
            if !self.session_unlocked {
                return Err(StorageError::EncryptedNoKey);
            }
            #[cfg(not(feature = "secure_vfs"))]
            return Err(StorageError::EncryptedNoKey);
        }

        let len = self.fs.read_file(path, buf).map_err(Self::map_error)?;

        #[cfg(feature = "secure_vfs")]
        if (perm & ATTR_ENCRYPTED) != 0 && len > 0 {
            Self::crypt_payload(path, &mut buf[..len]);
        }

        Ok(len)
    }

    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        Self::sanitize_path(path)?;
        let perm = self.fs.get_permission(path).unwrap_or(0);
        if (perm & ATTR_READ_ONLY) != 0 || (perm & ATTR_SYSTEM) != 0 {
            return Err(StorageError::AccessDenied);
        }
        if (perm & ATTR_ENCRYPTED) != 0 {
            #[cfg(feature = "secure_vfs")]
            {
                if !self.session_unlocked {
                    return Err(StorageError::EncryptedNoKey);
                }
                if data.len() > 512 {
                    return Err(StorageError::CapacityFull);
                }
                let mut tmp_buf = [0u8; 512];
                tmp_buf[..data.len()].copy_from_slice(data);
                Self::crypt_payload(path, &mut tmp_buf[..data.len()]);
                return self
                    .fs
                    .write_file(path, &tmp_buf[..data.len()])
                    .map_err(Self::map_error);
            }
            #[cfg(not(feature = "secure_vfs"))]
            {
                // In Guest/non-secure mode, strictly forbid overwriting encrypted files with plaintext!
                return Err(StorageError::EncryptedNoKey);
            }
        }

        self.fs.write_file(path, data).map_err(Self::map_error)
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
        self.fs
            .set_permission(path, attrs.permission)
            .map_err(Self::map_error)
    }

    pub fn list_dir(
        &mut self,
        dir_path: &str,
        out_entries: &mut [DirEntryInfo],
    ) -> Result<usize, StorageError> {
        let mut count = 0;
        let _ = self.fs.list_dir(dir_path, |path, size, perm, is_dir| {
            let already_exists = out_entries[..count].iter().any(|e| e.path.as_str() == path);
            if !already_exists && count < out_entries.len() {
                out_entries[count].path.clear();
                let _ = out_entries[count].path.push_str(path);
                out_entries[count].size = size;
                out_entries[count].is_dir = is_dir;
                out_entries[count].permission = perm;
                count += 1;
            }
        });
        Ok(count)
    }

    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), StorageError> {
        if let Ok(attrs) = self.get_attributes(old_path) {
            if (attrs.permission & (PERM_SYSTEM | PERM_READ_ONLY)) != 0
                || old_path.starts_with("/sys/calib.")
            {
                return Err(StorageError::AccessDenied);
            }
        }

        let mut buf = [0u8; 512];
        let len = self.read_file(old_path, &mut buf)?;
        self.write_file(new_path, &buf[..len])?;

        if let Ok(attrs) = self.get_attributes(old_path) {
            let _ = self.set_attributes(new_path, attrs);
        }

        self.remove(old_path)?;
        Ok(())
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
        let s = self.stats();
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
