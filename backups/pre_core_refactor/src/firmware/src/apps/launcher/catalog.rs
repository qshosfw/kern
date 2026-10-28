//! Dynamic Application Package Catalog & Discovery.
//!
//! Scans KernFS (`/apps/`) for installable packages (`.app` and `.krx`),
//! extracts identity metadata, and caches launchable entries for the OS launcher.

use crate::data::app_bundle::{AppClass, AppPreamble, ExecType};
use crate::services::SystemStorage;
use kernel::proc::krx::{
    KrxPreamble, SECTION_IDENTITY, SECTION_TYPE_LIFECYCLE, SectionIdentity, SectionType,
    find_section,
};

/// Cached catalog entry for an installed package.
#[derive(Debug, Clone)]
pub struct PackageCatalogEntry {
    pub title: heapless::String<16>,
    pub filename: heapless::String<24>,
    pub size_bytes: usize,
    pub app_class: AppClass,
    pub exec_type: ExecType,
    pub icon_id: u16,
    pub caps: u32,
}

/// In-memory catalog of discovered dynamic packages.
pub struct PackageCatalog {
    pub entries: heapless::Vec<PackageCatalogEntry, 8>,
    pub scanned: bool,
}

impl PackageCatalog {
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
            scanned: false,
        }
    }

    /// Mark catalog dirty to trigger a re-scan on next launcher entry.
    #[inline(always)]
    pub fn mark_dirty(&mut self) {
        self.scanned = false;
    }

    /// Iterator over discovered application packages (visible in launcher)
    pub fn apps(&self) -> impl Iterator<Item = &PackageCatalogEntry> {
        self.entries.iter().filter(|e| e.exec_type == ExecType::App)
    }

    /// Iterator over discovered background services, daemons, and drivers
    pub fn services(&self) -> impl Iterator<Item = &PackageCatalogEntry> {
        self.entries.iter().filter(|e| e.exec_type != ExecType::App)
    }

    /// Count of application packages
    pub fn app_count(&self) -> usize {
        self.apps().count()
    }

    /// Count of service packages
    pub fn service_count(&self) -> usize {
        self.services().count()
    }

    /// Perform a zero-allocation discovery pass across `/apps/` in storage.
    #[inline(always)]
    #[cfg(feature = "filesystem")]
    pub fn scan(&mut self, vfs: &mut SystemStorage) {
        if self.scanned {
            return;
        }
        self.scan_internal(vfs);
    }

    #[inline(never)]
    #[cfg(feature = "filesystem")]
    fn scan_internal(&mut self, vfs: &mut SystemStorage) {
        self.entries.clear();

        const EMPTY_DIR: kernel::service::DirEntryInfo = kernel::service::DirEntryInfo::empty();
        let mut dir_entries = [EMPTY_DIR; 8];

        if let Ok(count) = vfs.list_dir("/apps", &mut dir_entries) {
            for entry in &dir_entries[..count] {
                if !entry.is_dir && self.entries.len() < 8 {
                    if let Some(pkg) = Self::inspect_package(vfs, entry.path.as_str(), entry.size) {
                        let _ = self.entries.push(pkg);
                    }
                }
            }
        }
        self.scanned = true;
    }

    #[inline(never)]
    #[cfg(feature = "filesystem")]
    fn inspect_package(
        vfs: &mut SystemStorage,
        name: &str,
        size: usize,
    ) -> Option<PackageCatalogEntry> {
        let bytes = name.as_bytes();
        let n = bytes.len();
        let is_app = n >= 4 && &bytes[n - 4..] == b".app";
        let is_krx = n >= 4 && &bytes[n - 4..] == b".krx";
        if !is_app && !is_krx {
            return None;
        }

        let mut filename = heapless::String::<24>::new();
        let _ = filename.push_str(&name[..n.min(24)]);

        let mut full_path = [0u8; 32];
        full_path[..6].copy_from_slice(b"/apps/");
        let copy_len = n.min(26);
        full_path[6..6 + copy_len].copy_from_slice(&bytes[..copy_len]);
        let path_str = unsafe { core::str::from_utf8_unchecked(&full_path[..6 + copy_len]) };

        let mut app_class = AppClass::Declare;
        let mut exec_type = ExecType::App;
        let mut title = heapless::String::<16>::new();
        let mut icon_id = 0u16;
        let mut caps = 0u32;
        let mut hdr_buf = [0u8; 160];

        if let Ok(read_len) = vfs.read_file(path_str, &mut hdr_buf) {
            if read_len >= KrxPreamble::SIZE {
                if let Ok(preamble) = KrxPreamble::parse(&hdr_buf[..read_len]) {
                    if let Some(cls) = preamble.execution_class() {
                        app_class = match cls {
                            kernel::proc::krx::KrxClass::Native => AppClass::Native,
                            kernel::proc::krx::KrxClass::Paged => AppClass::Paged,
                            kernel::proc::krx::KrxClass::VMachine => AppClass::VMachine,
                            kernel::proc::krx::KrxClass::Declare => AppClass::Declare,
                        };
                    }

                    let header_len = (preamble.header_len as usize).min(read_len);

                    // Check Identity section for metadata & string-pool display name
                    if let Some(ident_body) = find_section(&hdr_buf, header_len, SECTION_IDENTITY) {
                        if let Some(ident) = SectionIdentity::parse(ident_body) {
                            icon_id = ident.icon_asset;
                            if let Some(name_str) = ident.name(&hdr_buf[..header_len]) {
                                let _ = title.push_str(name_str);
                            }
                        }
                    }

                    // Check Type & Lifecycle section for citizen role and capability mask
                    if let Some(type_body) =
                        find_section(&hdr_buf, header_len, SECTION_TYPE_LIFECYCLE)
                    {
                        if let Some(st) = SectionType::parse(type_body) {
                            if let Some(et) = ExecType::from_raw(st.exec_type) {
                                exec_type = et;
                            }
                            caps = st.cap_mask;
                        }
                    }
                } else if let Some(legacy) = AppPreamble::from_bytes(&hdr_buf) {
                    app_class = legacy.execution_class();
                    // Legacy explicit name payload at offset 128
                    if read_len >= 144 && hdr_buf[128] != 0 {
                        let mut len = 0;
                        while len < 16 && hdr_buf[128 + len] != 0 {
                            len += 1;
                        }
                        let s = unsafe { core::str::from_utf8_unchecked(&hdr_buf[128..128 + len]) };
                        let _ = title.push_str(s);
                    }
                }
            }
        }

        // Fallback: derive title from filename stem if not specified in binary header
        if title.is_empty() {
            let stem = &name[..n - 4];
            let start = stem
                .as_bytes()
                .iter()
                .rposition(|&b| b == b'/')
                .map_or(0, |p| p + 1);
            let stem = &stem[start..];
            let _ = title.push_str(&stem[..stem.len().min(16)]);
        }

        Some(PackageCatalogEntry {
            title,
            filename,
            size_bytes: size,
            app_class,
            exec_type,
            icon_id,
            caps,
        })
    }

    #[cfg(not(feature = "filesystem"))]
    pub fn scan(&mut self, _vfs: &mut SystemStorage) {}
}

pub static mut PACKAGE_CATALOG: PackageCatalog = PackageCatalog::new();

pub fn get_catalog() -> &'static mut PackageCatalog {
    unsafe { &mut *core::ptr::addr_of_mut!(PACKAGE_CATALOG) }
}
