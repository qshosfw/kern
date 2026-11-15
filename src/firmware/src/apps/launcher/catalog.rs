//! Dynamic Application Package Catalog & Discovery.
//!
//! Scans KernFS (`/apps/`) for installable packages (`.app` and `.krx`),
//! extracts identity metadata, flags invalid/corrupted files at the top,
//! and caches entries for the OS launcher and Settings manager.

use crate::services::SystemStorage;
#[cfg(feature = "filesystem")]
use kernel::proc::krx::KrxPreamble;
use kernel::proc::krx::{KrxClass, KrxExecType};

/// Cached catalog entry for an installed package.
#[derive(Debug, Clone)]
pub struct PackageCatalogEntry {
    pub title: heapless::String<16>,
    pub path: heapless::String<24>,
    pub size_bytes: usize,
    pub app_class: KrxClass,
    pub exec_type: KrxExecType,
    pub icon_id: u16,
    pub caps: u32,
    pub is_valid: bool,
}

impl PackageCatalogEntry {
    #[inline(always)]
    pub fn filename(&self) -> &str {
        extract_filename(self.path.as_str())
    }
}

pub fn extract_filename(path: &str) -> &str {
    let start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
    &path[start..]
}

pub const MAX_CATALOG_PACKAGES: usize = 6;

/// In-memory catalog of discovered dynamic packages.
pub struct PackageCatalog {
    pub entries: heapless::Vec<PackageCatalogEntry, MAX_CATALOG_PACKAGES>,
    pub scanned: bool,
}

impl PackageCatalog {
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
            scanned: false,
        }
    }

    /// Mark catalog dirty to trigger a re-scan on next access.
    #[inline(always)]
    pub fn mark_dirty(&mut self) {
        self.scanned = false;
    }

    #[inline(always)]
    pub fn matches_kind(e: &PackageCatalogEntry, is_service: bool) -> bool {
        (e.is_valid && e.exec_type != KrxExecType::App) == is_service
    }

    pub fn get_entry(&self, is_service: bool, mut idx: usize) -> Option<&PackageCatalogEntry> {
        for e in self.entries.as_slice() {
            if Self::matches_kind(e, is_service) {
                if idx == 0 {
                    return Some(e);
                }
                idx -= 1;
            }
        }
        None
    }

    pub fn count_entries(&self, is_service: bool) -> usize {
        let mut n = 0;
        for e in self.entries.as_slice() {
            if Self::matches_kind(e, is_service) {
                n += 1;
            }
        }
        n
    }

    #[inline(always)]
    pub fn app_count(&self) -> usize {
        self.count_entries(false)
    }

    #[inline(always)]
    pub fn service_count(&self) -> usize {
        self.count_entries(true)
    }

    /// Perform a zero-allocation discovery pass across `/apps/` in storage.
    #[inline(always)]
    #[cfg(feature = "filesystem")]
    pub fn scan(&mut self, filesystem: &mut SystemStorage) {
        if self.scanned {
            return;
        }
        self.scan_internal(filesystem);
    }

    #[inline(never)]
    #[cfg(feature = "filesystem")]
    fn scan_internal(&mut self, filesystem: &mut SystemStorage) {
        self.entries.clear();

        // Keep scan scratch strictly inside TRANSIENT_ARENA[4096..6144) so [0..4096) KRX code is never clobbered.
        const HDR_SCRATCH_BYTES: usize = 256;
        const {
            assert!(
                core::mem::align_of::<kernel::service::DirEntryInfo>()
                    <= core::mem::align_of::<crate::data::arena::AlignedArena>()
            );
            assert!(
                HDR_SCRATCH_BYTES + 8 * core::mem::size_of::<kernel::service::DirEntryInfo>()
                    <= crate::data::arena::TRANSIENT_ARENA_BYTES
                        - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
            );
        }
        let dir_ptr = crate::data::arena::scratch_tail_bytes_mut()[HDR_SCRATCH_BYTES..].as_mut_ptr()
            as *mut kernel::service::DirEntryInfo;
        unsafe {
            core::ptr::write_bytes(dir_ptr, 0, 8);
        }
        let dir_entries = unsafe { core::slice::from_raw_parts_mut(dir_ptr, 8) };

        if let Ok(count) = filesystem.list_dir("/apps", dir_entries) {
            for i in 0..count {
                let (is_dir, size, path_ptr, path_len) = {
                    let e = &dir_entries[i];
                    (e.is_dir, e.size, e.path.as_ptr(), e.path.len())
                };
                if !is_dir && self.entries.len() < MAX_CATALOG_PACKAGES {
                    let path_str = unsafe {
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                            path_ptr, path_len,
                        ))
                    };
                    Self::inspect_package(filesystem, path_str, size, &mut self.entries);
                }
            }
        }

        // Sort invalid/corrupted packages to the top so they are immediately visible for cleanup
        let base = self.entries.as_mut_ptr();
        for i in 1..self.entries.len() {
            let mut j = i;
            unsafe {
                while j > 0 && !(*base.add(j)).is_valid && (*base.add(j - 1)).is_valid {
                    let a = base.add(j - 1) as *mut u8;
                    let b = base.add(j) as *mut u8;
                    for k in 0..core::mem::size_of::<PackageCatalogEntry>() {
                        core::ptr::swap(a.add(k), b.add(k));
                    }
                    j -= 1;
                }
            }
        }

        self.scanned = true;
    }

    #[inline(never)]
    #[cfg(feature = "filesystem")]
    fn inspect_package(
        filesystem: &mut SystemStorage,
        raw_name: &str,
        size: usize,
        entries: &mut heapless::Vec<PackageCatalogEntry, MAX_CATALOG_PACKAGES>,
    ) {
        let base_name = extract_filename(raw_name);
        let len = entries.len();
        if base_name.is_empty() || len >= MAX_CATALOG_PACKAGES {
            return;
        }
        let entry = unsafe {
            let ptr = entries.as_mut_ptr().add(len);
            core::ptr::write_bytes(ptr, 0, 1);
            entries.set_len(len + 1);
            &mut *ptr
        };
        entry.size_bytes = size;
        entry.app_class = KrxClass::Declare;
        entry.exec_type = KrxExecType::App;

        let bytes = base_name.as_bytes();
        let n = bytes.len();
        let is_pkg = n > 4 && (&bytes[n - 4..] == b".app" || &bytes[n - 4..] == b".krx");

        if !raw_name.starts_with("/apps/") {
            let _ = entry.path.push_str("/apps/");
        }
        let _ = entry
            .path
            .push_str(&raw_name[..raw_name.len().min(24 - entry.path.len())]);

        if is_pkg && size >= KrxPreamble::SIZE {
            let hdr_buf = &mut crate::data::arena::scratch_tail_bytes_mut()[..256];
            let mut summary_uninit = core::mem::MaybeUninit::uninit();
            if let Ok(summary) = crate::runtime::loader::read_krx_summary(
                filesystem,
                entry.path.as_str(),
                hdr_buf,
                &mut summary_uninit,
            ) {
                if let Some(cls) = summary.preamble.execution_class() {
                    entry.is_valid = true;
                    entry.app_class = cls;
                }
                entry.exec_type = summary.exec_type;
                entry.icon_id = summary.icon_id;
                entry.caps = summary.caps.raw();
                if !summary.title.is_empty() {
                    let _ = entry.title.push_str(summary.title);
                }
            }
        }

        if entry.title.is_empty() {
            let stem = if is_pkg {
                &base_name[..n - 4]
            } else {
                base_name
            };
            let _ = entry.title.push_str(&stem[..stem.len().min(16)]);
        }
    }

    #[cfg(not(feature = "filesystem"))]
    pub fn scan(&mut self, _filesystem: &mut SystemStorage) {}
}

pub static mut PACKAGE_CATALOG: PackageCatalog = PackageCatalog::new();

pub fn get_catalog() -> &'static mut PackageCatalog {
    unsafe { &mut *core::ptr::addr_of_mut!(PACKAGE_CATALOG) }
}
