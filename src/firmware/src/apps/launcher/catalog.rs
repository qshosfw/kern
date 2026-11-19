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
    pub lifecycle_flags: u8,
    pub icon_id: u16,
    pub caps: u32,
    pub is_valid: bool,
    pub has_config: bool,
}

impl PackageCatalogEntry {
    #[inline(always)]
    pub fn filename(&self) -> &str {
        extract_filename(self.path.as_str())
    }

    #[inline(always)]
    pub fn is_system_entry(&self, sys_caps: kernel::caps::CapabilitySet, flag: u8) -> bool {
        self.is_valid
            && self.exec_type == KrxExecType::Module
            && (self.lifecycle_flags & flag) != 0
            && sys_caps.contains_all(kernel::caps::CapabilitySet(self.caps))
    }

    #[inline(always)]
    pub fn is_system_module(&self, sys_caps: kernel::caps::CapabilitySet) -> bool {
        self.is_system_entry(sys_caps, kernel::proc::krx::SectionType::FLAG_SYSTEM_MENU)
    }
}

#[inline(always)]
fn is_package_ext(bytes: &[u8]) -> bool {
    let n = bytes.len();
    if n <= 4 {
        return false;
    }
    let ext = &bytes[n - 4..];
    matches!(
        ext,
        b".app" | b".svc" | b".dmn" | b".drv" | b".mod" | b".krx"
    )
}

pub fn extract_filename(path: &str) -> &str {
    let start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
    &path[start..]
}

pub const MAX_CATALOG_PACKAGES: usize = 8;
pub const MAX_CATALOG_ACTIONS: usize = 16;

/// Cached declared action from a KRX app's `SECTION_ACTIONS` (16 B).
#[derive(Debug, Clone, Copy)]
pub struct CatalogAction {
    pub pkg_idx: u8,
    pub scope_mask: u8,
    pub action_id: u16,
    pub label: [u8; 12],
}

impl CatalogAction {
    pub fn label_str(&self) -> &str {
        let len = self.label.iter().position(|&b| b == 0).unwrap_or(12);
        utils::format::ascii_str(&self.label[..len]).unwrap_or("")
    }
}

/// In-memory catalog of discovered dynamic packages.
pub struct PackageCatalog {
    pub entries: heapless::Vec<PackageCatalogEntry, MAX_CATALOG_PACKAGES>,
    pub actions: heapless::Vec<CatalogAction, MAX_CATALOG_ACTIONS>,
    pub scanned: bool,
}

impl PackageCatalog {
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
            actions: heapless::Vec::new(),
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
        self.actions.clear();

        // Keep scan scratch strictly inside TRANSIENT_ARENA[4096..6144) so [0..4096) KRX code is never clobbered.
        const HDR_SCRATCH_BYTES: usize = kernel::proc::krx::KrxPreamble::MAX_HEADER_SIZE;
        let dir_ptr = crate::data::arena::scratch_tail_bytes_mut()[HDR_SCRATCH_BYTES..].as_mut_ptr()
            as *mut kernel::proc::abi::KrxDirEntry;
        let dir_entries = unsafe { core::slice::from_raw_parts_mut(dir_ptr, 8) };

        if let Ok(count) =
            crate::services::primitives::list_krx_dir(filesystem, "/apps", dir_entries)
        {
            for i in 0..count {
                let e = &dir_entries[i];
                if e.is_dir == 0 && self.entries.len() < MAX_CATALOG_PACKAGES {
                    let len = e.name.iter().position(|&b| b == 0).unwrap_or(24);
                    let path_str = unsafe { core::str::from_utf8_unchecked(&e.name[..len]) };
                    Self::inspect_package(
                        filesystem,
                        path_str,
                        e.size as usize,
                        &mut self.entries,
                        &mut self.actions,
                    );
                }
            }
        }

        // Sort invalid/corrupted packages to the top, keeping action `pkg_idx` synced
        const _: () = {
            assert!(core::mem::align_of::<PackageCatalogEntry>() >= 4);
            assert!(core::mem::size_of::<PackageCatalogEntry>() % 4 == 0);
        };
        let base = self.entries.as_mut_ptr();
        for i in 1..self.entries.len() {
            let mut j = i;
            unsafe {
                while j > 0 && !(*base.add(j)).is_valid && (*base.add(j - 1)).is_valid {
                    let pa = base.add(j - 1) as *mut u32;
                    let pb = base.add(j) as *mut u32;
                    for w in 0..(core::mem::size_of::<PackageCatalogEntry>() / 4) {
                        let tmp = *pa.add(w);
                        *pa.add(w) = *pb.add(w);
                        *pb.add(w) = tmp;
                    }
                    let a = (j - 1) as u8;
                    let b = j as u8;
                    for act in self.actions.iter_mut() {
                        if act.pkg_idx == a {
                            act.pkg_idx = b;
                        } else if act.pkg_idx == b {
                            act.pkg_idx = a;
                        }
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
        actions: &mut heapless::Vec<CatalogAction, MAX_CATALOG_ACTIONS>,
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
        entry.app_class = KrxClass::Native;
        entry.exec_type = KrxExecType::App;

        let bytes = base_name.as_bytes();
        let n = bytes.len();
        let is_pkg = is_package_ext(bytes);

        if !raw_name.starts_with("/apps/") {
            let _ = entry.path.push_str("/apps/");
        }
        let _ = entry
            .path
            .push_str(&raw_name[..raw_name.len().min(24 - entry.path.len())]);

        if is_pkg && size >= KrxPreamble::SIZE {
            const HDR_BYTES: usize = kernel::proc::krx::KrxPreamble::MAX_HEADER_SIZE;
            const DIR_RESERVE: usize = core::mem::size_of::<kernel::proc::abi::KrxDirEntry>() * 8;
            const SUMMARY_OFF: usize = HDR_BYTES + DIR_RESERVE;
            const _: () = assert!(
                SUMMARY_OFF + core::mem::size_of::<kernel::proc::krx::KrxSummary<'static>>()
                    <= crate::data::arena::TRANSIENT_ARENA_BYTES
                        - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
            );
            let (hdr_buf, tail) =
                crate::data::arena::scratch_tail_bytes_mut().split_at_mut(HDR_BYTES);
            let summary_slot = unsafe {
                &mut *(tail.as_mut_ptr().add(DIR_RESERVE)
                    as *mut core::mem::MaybeUninit<kernel::proc::krx::KrxSummary<'_>>)
            };
            if let Ok(summary) = crate::runtime::loader::read_krx_summary(
                filesystem,
                entry.path.as_str(),
                hdr_buf,
                summary_slot,
            ) {
                if let Some(cls) = summary.preamble.execution_class() {
                    entry.is_valid = true;
                    entry.app_class = cls;
                }
                entry.exec_type = summary.exec_type;
                entry.lifecycle_flags = summary.type_flags;
                entry.icon_id = summary.icon_id;
                entry.caps = summary.caps.raw();
                if !summary.title.is_empty() {
                    let _ = entry.title.push_str(summary.title);
                }
                if entry.is_valid && entry.exec_type == KrxExecType::App {
                    entry.has_config = summary.aux.config.is_some();
                    if let Some(act_body) = summary.aux.actions {
                        let count = kernel::proc::krx::KrxActionDecl::count(act_body);
                        for k in 0..count {
                            if actions.len() >= MAX_CATALOG_ACTIONS {
                                break;
                            }
                            if let Some(decl) = kernel::proc::krx::KrxActionDecl::parse(act_body, k)
                            {
                                let _ = actions.push(CatalogAction {
                                    pkg_idx: len as u8,
                                    scope_mask: decl.scope_mask,
                                    action_id: decl.action_id,
                                    label: decl.label,
                                });
                            }
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_mod_header_validates_within_max_header_size() {
        const PANIC_MOD: &[u8] = include_bytes!("../../../../../packages/panic/panic.mod");
        let hdr_len = KrxPreamble::MAX_HEADER_SIZE.min(PANIC_MOD.len());
        let summary = KrxPreamble::summarize(&PANIC_MOD[..hdr_len])
            .expect("panic.mod header must parse within KrxPreamble::MAX_HEADER_SIZE");

        assert!(
            summary.preamble.header_len as usize > 256,
            "panic.mod header exceeds legacy 256B limit"
        );
        assert_eq!(
            summary.preamble.execution_class(),
            Some(kernel::proc::krx::KrxClass::Paged)
        );
        assert_eq!(summary.exec_type, KrxExecType::Module);
        assert_ne!(
            summary.type_flags & kernel::proc::krx::SectionType::FLAG_SYSTEM_MENU,
            0
        );
        assert_eq!(summary.title, "Crash Logs");
        assert!(summary.exec.is_some());
        assert_eq!(summary.aux.leaf_map.map(|m| m.len()), Some(17 * 16));
    }
}
