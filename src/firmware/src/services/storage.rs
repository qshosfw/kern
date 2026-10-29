pub use kernel::service::{
    ATTR_COMPRESSED, ATTR_ENCRYPTED, ATTR_HIDDEN, ATTR_READ_ONLY, ATTR_SYSTEM, ATTR_UNENCRYPTED,
    DirEntryInfo, FileAttributes, StorageError, StorageExt, StorageService, StorageStats,
};

pub struct NoStorage;

impl NoStorage {
    pub fn exists(&mut self, _path: &str) -> bool {
        false
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
    pub fn write_file(&mut self, _path: &str, _data: &[u8]) -> Result<(), StorageError> {
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
    pub fn list_dir(
        &mut self,
        _dir_path: &str,
        _out_entries: &mut [DirEntryInfo],
    ) -> Result<usize, StorageError> {
        Ok(0)
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

impl StorageService for NoStorage {
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
    fn stats(&mut self) -> Option<StorageStats> {
        let s = self.stats();
        Some(StorageStats {
            active_files: s.active_files,
            used_sectors: s.used_sectors,
            free_sectors: s.free_sectors,
            deleted_sectors: s.deleted_sectors,
            wear_cycles: s.wear_cycles,
            total_sectors: s.total_sectors,
        })
    }
}

#[cfg(feature = "filesystem")]
pub type SystemStorage<'a> =
    drivers::filesystem::FileSystemDriver<'a, drivers::py25q16::Py25q16FlashDriver<'a>>;
#[cfg(not(feature = "filesystem"))]
pub type SystemStorage<'a> = NoStorage;
