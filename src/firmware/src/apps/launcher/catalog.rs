//! Dynamic Application Package Catalog & Discovery.
//!
//! Scans KernFS (`/apps/`) for installable packages (`.app` and `.krx`),
//! extracts identity metadata, flags invalid/corrupted files at the top,
//! and caches entries for the OS launcher and Settings manager.

use crate::services::SystemStorage;
use kernel::proc::krx::{KrxClass, KrxExecType};
#[cfg(feature = "filesystem")]
use kernel::proc::krx::{
    KrxPreamble, SECTION_IDENTITY, SECTION_TYPE_LIFECYCLE, SectionIdentity, SectionType,
    find_section,
};

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

#[inline(always)]
fn extract_filename(path: &str) -> &str {
    let bytes = path.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes[i] == b'/' {
            return &path[i + 1..];
        }
    }
    path
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

    /// Iterator over discovered application packages (including invalid entries sorted first).
    pub fn apps(&self) -> impl Iterator<Item = &PackageCatalogEntry> {
        self.entries
            .iter()
            .filter(|e| !e.is_valid || e.exec_type == KrxExecType::App)
    }

    /// Iterator over discovered background services, daemons, and drivers.
    pub fn services(&self) -> impl Iterator<Item = &PackageCatalogEntry> {
        self.entries
            .iter()
            .filter(|e| e.is_valid && e.exec_type != KrxExecType::App)
    }

    /// Count of application entries (including invalid entries flagged for cleanup).
    pub fn app_count(&self) -> usize {
        self.apps().count()
    }

    /// Count of valid service packages.
    pub fn service_count(&self) -> usize {
        self.services().count()
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
        let _ = path.push_str("/apps/");
        let _ = path.push_str(&base_name[..n.min(18)]);

        let mut app_class = KrxClass::Declare;
        let mut exec_type = KrxExecType::App;
        let mut title = heapless::String::<16>::new();
        let mut icon_id = 0u16;
        let mut caps = 0u32;
        let mut is_valid = false;

        if is_pkg && size >= KrxPreamble::SIZE {
            let hdr_buf = &mut crate::runtime::transient_arena_bytes_mut()[..160];
            if let Ok(read_len) = filesystem.read_file(path.as_str(), hdr_buf) {
                if let Ok(preamble) = KrxPreamble::parse(&hdr_buf[..read_len]) {
                    if let Some(cls) = preamble.execution_class() {
                        is_valid = true;
                        app_class = cls;
                    }

                    let h_len = (preamble.header_len as usize).min(read_len);
                    let hdr = &hdr_buf[..h_len];
                    if let Some(ident) = find_section(hdr_buf, h_len, SECTION_IDENTITY)
                        .and_then(SectionIdentity::parse)
                    {
                        icon_id = ident.icon_asset;
                        if let Some(name_str) = ident.name(hdr) {
                            let _ = title.push_str(name_str);
                        }
                    }

                    if let Some(st) = find_section(hdr_buf, h_len, SECTION_TYPE_LIFECYCLE)
                        .and_then(SectionType::parse)
                    {
                        if let Some(et) = st.exec_type() {
                            exec_type = et;
                        }
                        caps = st.cap_mask;
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
