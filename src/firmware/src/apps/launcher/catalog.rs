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

fn extract_filename(path: &str) -> &str {
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
    fn matches_kind(e: &PackageCatalogEntry, is_service: bool) -> bool {
        if is_service {
            e.is_valid && e.exec_type != KrxExecType::App
        } else {
            !e.is_valid || e.exec_type == KrxExecType::App
        }
    }

    pub fn get_entry(&self, is_service: bool, idx: usize) -> Option<&PackageCatalogEntry> {
        self.entries
            .iter()
            .filter(|e| Self::matches_kind(e, is_service))
            .nth(idx)
    }

    pub fn count_entries(&self, is_service: bool) -> usize {
        self.entries
            .iter()
            .filter(|e| Self::matches_kind(e, is_service))
            .count()
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

        const EMPTY_DIR: kernel::service::DirEntryInfo = kernel::service::DirEntryInfo::empty();
        let mut dir_entries = [EMPTY_DIR; 8];

        if let Ok(count) = filesystem.list_dir("/apps", &mut dir_entries) {
            for entry in &dir_entries[..count] {
                if !entry.is_dir && self.entries.len() < MAX_CATALOG_PACKAGES {
                    if let Some(pkg) =
                        Self::inspect_package(filesystem, entry.path.as_str(), entry.size)
                    {
                        let _ = self.entries.push(pkg);
                    }
                }
            }
        }

        // Sort invalid/corrupted packages to the top so they are immediately visible for cleanup
        let n = self.entries.len();
        for i in 1..n {
            let mut j = i;
            while j > 0 && !self.entries[j].is_valid && self.entries[j - 1].is_valid {
                self.entries.swap(j - 1, j);
                j -= 1;
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
    ) -> Option<PackageCatalogEntry> {
        let base_name = extract_filename(raw_name);
        if base_name.is_empty() {
            return None;
        }

        let bytes = base_name.as_bytes();
        let n = bytes.len();
        let is_pkg = n > 4 && (&bytes[n - 4..] == b".app" || &bytes[n - 4..] == b".krx");

        let mut path = heapless::String::<24>::new();
        if !raw_name.starts_with("/apps/") {
            let _ = path.push_str("/apps/");
        }
        let _ = path.push_str(&raw_name[..raw_name.len().min(24 - path.len())]);

        let mut app_class = KrxClass::Declare;
        let mut exec_type = KrxExecType::App;
        let mut title = heapless::String::<16>::new();
        let mut icon_id = 0u16;
        let mut caps = 0u32;
        let mut is_valid = false;

        if is_pkg && size >= KrxPreamble::SIZE {
            let hdr_buf = &mut crate::runtime::transient_arena_bytes_mut()[..256];
            if let Ok(read_len) = filesystem.read_file_at(path.as_str(), 0, hdr_buf) {
                if let Ok(summary) = KrxPreamble::summarize(&hdr_buf[..read_len]) {
                    if let Some(cls) = summary.preamble.execution_class() {
                        is_valid = true;
                        app_class = cls;
                    }
                    exec_type = summary.exec_type;
                    icon_id = summary.icon_id;
                    caps = summary.caps.raw();
                    if !summary.title.is_empty() {
                        let _ = title.push_str(summary.title);
                    }
                }
            }
        }

        if title.is_empty() {
            let stem = if is_pkg {
                &base_name[..n - 4]
            } else {
                base_name
            };
            let _ = title.push_str(&stem[..stem.len().min(16)]);
        }

        Some(PackageCatalogEntry {
            title,
            path,
            size_bytes: size,
            app_class,
            exec_type,
            icon_id,
            caps,
            is_valid,
        })
    }

    #[cfg(not(feature = "filesystem"))]
    pub fn scan(&mut self, _filesystem: &mut SystemStorage) {}
}

pub static mut PACKAGE_CATALOG: PackageCatalog = PackageCatalog::new();

pub fn get_catalog() -> &'static mut PackageCatalog {
    unsafe { &mut *core::ptr::addr_of_mut!(PACKAGE_CATALOG) }
}
