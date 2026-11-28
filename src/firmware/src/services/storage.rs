pub use kernel::service::{
    ATTR_COMPRESSED, ATTR_ENCRYPTED, ATTR_HIDDEN, ATTR_READ_ONLY, ATTR_SYSTEM, ATTR_UNENCRYPTED,
    FileAttributes, StorageError,
};

#[cfg(not(feature = "filesystem"))]
pub struct NoStorage;

#[cfg(not(feature = "filesystem"))]
impl NoStorage {
    pub fn exists(&mut self, _path: &str) -> bool {
        false
    }
    pub fn stat_info(&mut self, _path: &str) -> Result<(usize, bool), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn file_size(&mut self, _path: &str) -> Result<usize, StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn remove(&mut self, _path: &str) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn create_dir(&mut self, _path: &str) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn rename(&mut self, _old_path: &str, _new_path: &str) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn read_file(&mut self, _path: &str, _buf: &mut [u8]) -> Result<usize, StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn read_file_at(
        &mut self,
        _path: &str,
        _offset: usize,
        _buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn lookup_ino(&mut self, _path: &str) -> Result<u16, StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn read_ino_at(
        &mut self,
        _ino: u16,
        _offset: usize,
        _buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn write_file(&mut self, _path: &str, _data: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn write_ring(
        &mut self,
        _path: &str,
        _max_bytes: usize,
        _data: &[u8],
    ) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn get_attributes(&mut self, _path: &str) -> Result<FileAttributes, StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn set_attributes(
        &mut self,
        _path: &str,
        _attrs: FileAttributes,
    ) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn for_each_entry<F: FnMut(&str, usize, u8, bool)>(
        &mut self,
        _dir_path: &str,
        _cb: F,
    ) -> Result<usize, StorageError> {
        Ok(0)
    }
    pub fn write_at(
        &mut self,
        _path: &str,
        _offset: usize,
        _data: &[u8],
    ) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn count_encrypted_files(&mut self) -> usize {
        0
    }
    pub fn stats(&mut self) -> kernfs::FsStats {
        kernfs::FsStats::default()
    }
    pub fn read_struct<T: Copy>(
        &mut self,
        _path: &str,
        _out: &mut T,
    ) -> Result<usize, StorageError> {
        Err(StorageError::NotFound)
    }
    pub fn write_struct<T: Copy>(&mut self, _path: &str, _data: &T) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
}

#[cfg(feature = "filesystem")]
pub type SystemStorage<'a> =
    drivers::filesystem::FileSystemDriver<'a, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>;
#[cfg(not(feature = "filesystem"))]
pub type SystemStorage<'a> = NoStorage;

fn app_config_path_into<'a>(app_name: &str, buf: &'a mut [u8; 32]) -> &'a str {
    const PFX: &[u8] = b"/pref/app/";
    const SFX: &[u8] = b".cfg";
    let n = app_name.len().min(buf.len() - PFX.len() - SFX.len());
    buf[..PFX.len()].copy_from_slice(PFX);
    buf[PFX.len()..PFX.len() + n].copy_from_slice(&app_name.as_bytes()[..n]);
    buf[PFX.len() + n..PFX.len() + n + SFX.len()].copy_from_slice(SFX);
    unsafe { core::str::from_utf8_unchecked(&buf[..PFX.len() + n + SFX.len()]) }
}

pub fn load_app_config(
    storage: &mut SystemStorage,
    app_name: &str,
    buf: &mut [u8],
) -> Result<usize, ()> {
    let mut pbuf = [0u8; 32];
    storage
        .read_file(app_config_path_into(app_name, &mut pbuf), buf)
        .map_err(|_| ())
}

pub fn save_app_config(storage: &mut SystemStorage, app_name: &str, data: &[u8]) -> Result<(), ()> {
    let mut pbuf = [0u8; 32];
    crate::security::write_classified_file(storage, app_config_path_into(app_name, &mut pbuf), data)
        .map_err(|_| ())
}

pub fn list_krx_dir_at(
    storage: &mut SystemStorage,
    dir_path: &str,
    skip: usize,
    out_entries: &mut [kernel::proc::abi::KrxDirEntry],
) -> Result<(usize, usize), ()> {
    let mut total = 0usize;
    let mut written = 0usize;
    storage
        .for_each_entry(dir_path, |path, size, _perm, is_dir| {
            if total >= skip {
                if let Some(dst) = out_entries.get_mut(written) {
                    unsafe { core::ptr::write_bytes(dst, 0, 1) };
                    let b = path.as_bytes();
                    let n = b.len().min(24);
                    dst.name[..n].copy_from_slice(&b[..n]);
                    dst.size = size as u32;
                    dst.is_dir = u8::from(is_dir);
                    written += 1;
                }
            }
            total += 1;
        })
        .map_err(|_| ())?;
    Ok((total, written))
}
