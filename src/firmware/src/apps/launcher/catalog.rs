//! Dynamic Application Package Catalog & Discovery.
//!
//! Scans KernFS (`/apps/`) for installable packages (`.app` and `.krx`),
//! extracts identity metadata, flags invalid/corrupted files at the top,
//! and caches entries for the OS launcher and Settings manager.

use crate::services::SystemStorage;
#[cfg(feature = "filesystem")]
use kernel::runtime::KrxPreamble;
use kernel::runtime::{KrxClass, KrxExecType};

/// Fixed-capacity UTF-8 string with 1-byte length at offset 0 (single-instruction `ldrb`
/// on Cortex-M0+ and zero monomorphized `heapless::String::push_str` codegen).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SmallStr<const N: usize> {
    len: u8,
    buf: [u8; N],
}

impl<const N: usize> SmallStr<N> {
    pub const fn new() -> Self {
        Self {
            len: 0,
            buf: [0; N],
        }
    }

    pub fn from_str(s: &str) -> Self {
        let mut out = Self::new();
        out.len = write_trunc_utf8(&mut out.buf, 0, s);
        out
    }

    #[inline(always)]
    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len as usize]) }
    }
}

#[inline(never)]
fn write_trunc_utf8(dst: &mut [u8], start: usize, s: &str) -> u8 {
    let b = s.as_bytes();
    let max = dst.len().saturating_sub(start);
    let mut n = b.len().min(max);
    while n > 0 && n < b.len() && (b[n] & 0xC0) == 0x80 {
        n -= 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(b.as_ptr(), dst.as_mut_ptr().add(start), n);
    }
    (start + n) as u8
}

/// Cached catalog entry for an installed package (exact 64 B = 2^6 for 1-insn `lsls #6` indexing;
/// all `u8`/`bool`/`SmallStr::len` fields sit in offsets `0..31` for 1-insn Cortex-M0+ `ldrb`).
#[derive(Debug, Clone)]
#[repr(C)]
pub struct PackageCatalogEntry {
    pub app_class: KrxClass,
    pub exec_type: KrxExecType,
    pub lifecycle_flags: u8,
    pub is_valid: bool,
    pub has_config: bool,
    pub title: SmallStr<16>,
    pub path: SmallStr<24>,
    pub _pad: [u8; 3],
    pub icon_id: u16,
    pub caps: u32,
    pub size_bytes: u32,
    pub _pad2: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<PackageCatalogEntry>() == 64);

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
        self.is_system_entry(sys_caps, kernel::runtime::SectionType::FLAG_SYSTEM_MENU)
    }
}

utils::static_bytes!(static PKG_EXTS = b"appsvcdmndrvmodkrx";);

fn is_package_ext(bytes: &[u8]) -> bool {
    let n = bytes.len();
    if n <= 4 || bytes[n - 4] != b'.' {
        return false;
    }
    let ext = &bytes[n - 3..];
    let mut i = 0;
    while i + 3 <= PKG_EXTS.len() {
        if ext[0] == PKG_EXTS[i] && ext[1] == PKG_EXTS[i + 1] && ext[2] == PKG_EXTS[i + 2] {
            return true;
        }
        i += 3;
    }
    false
}

pub fn extract_filename(path: &str) -> &str {
    let start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
    &path[start..]
}

pub const MAX_CATALOG_PACKAGES: usize = 8;
pub const MAX_CATALOG_ACTIONS: usize = 16;

/// Cached declared action from a KRX app's `SECTION_ACTIONS` (16 B, wire-identical to `KrxActionDecl`
/// with `flags` byte repurposed as `pkg_idx` for zero-unpacking copy).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CatalogAction {
    pub action_id: u16,
    pub scope_mask: u8,
    pub pkg_idx: u8,
    pub label: [u8; 12],
}

const _: () = assert!(core::mem::size_of::<CatalogAction>() == 16);

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
        const HDR_SCRATCH_BYTES: usize = kernel::runtime::KrxPreamble::MAX_HEADER_SIZE;
        let dir_ptr = crate::data::arena::scratch_tail_bytes_mut()[HDR_SCRATCH_BYTES..].as_mut_ptr()
            as *mut kernel::proc::abi::KrxDirEntry;
        let dir_entries = unsafe { core::slice::from_raw_parts_mut(dir_ptr, 8) };

        if let Ok(count) =
            crate::services::primitives::list_krx_dir(filesystem, "/apps", dir_entries)
        {
            for e in &dir_entries[..count] {
                if e.is_dir == 0 && self.entries.len() < MAX_CATALOG_PACKAGES {
                    self.inspect_package(filesystem, e);
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
        &mut self,
        filesystem: &mut SystemStorage,
        dir: &kernel::proc::abi::KrxDirEntry,
    ) {
        let name_len = dir.name.iter().position(|&b| b == 0).unwrap_or(24);
        let raw_name = unsafe { core::str::from_utf8_unchecked(&dir.name[..name_len]) };
        let base_name = extract_filename(raw_name);
        let len = self.entries.len();
        if base_name.is_empty() || len >= MAX_CATALOG_PACKAGES {
            return;
        }
        let size = dir.size;
        let entry = unsafe {
            let ptr = self.entries.as_mut_ptr().add(len);
            core::ptr::write_bytes(ptr, 0, 1);
            self.entries.set_len(len + 1);
            &mut *ptr
        };
        entry.size_bytes = size;
        entry.exec_type = KrxExecType::App;

        let bytes = base_name.as_bytes();
        let n = bytes.len();
        let is_pkg = is_package_ext(bytes);

        let start = if raw_name.as_bytes().first() == Some(&b'/') {
            0
        } else {
            entry.path.buf[..6].copy_from_slice(b"/apps/");
            6
        };
        entry.path.len = write_trunc_utf8(&mut entry.path.buf, start, raw_name);

        let mut title: Option<&str> = None;
        if is_pkg && (size as usize) >= KrxPreamble::SIZE {
            const HDR_BYTES: usize = kernel::runtime::KrxPreamble::MAX_HEADER_SIZE;
            const DIR_RESERVE: usize = core::mem::size_of::<kernel::proc::abi::KrxDirEntry>() * 8;
            const SUMMARY_OFF: usize = HDR_BYTES + DIR_RESERVE;
            const _: () = assert!(
                SUMMARY_OFF + core::mem::size_of::<kernel::runtime::KrxSummary<'static>>()
                    <= crate::data::arena::TRANSIENT_ARENA_BYTES
                        - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
            );
            let (hdr_buf, tail) =
                crate::data::arena::scratch_tail_bytes_mut().split_at_mut(HDR_BYTES);
            let summary_slot = unsafe {
                &mut *(tail.as_mut_ptr().add(DIR_RESERVE)
                    as *mut core::mem::MaybeUninit<kernel::runtime::KrxSummary<'_>>)
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
                title = summary.title;
                if entry.is_valid && entry.exec_type == KrxExecType::App {
                    entry.has_config = summary.aux.config.is_some();
                    if let Some(act_body) = summary.aux.actions {
                        let mut off = 0usize;
                        while off + kernel::runtime::KrxActionDecl::SIZE <= act_body.len()
                            && self.actions.len() < MAX_CATALOG_ACTIONS
                        {
                            let alen = self.actions.len();
                            unsafe {
                                let dst = self.actions.as_mut_ptr().add(alen);
                                core::ptr::copy_nonoverlapping(
                                    act_body.as_ptr().add(off),
                                    dst.cast::<u8>(),
                                    kernel::runtime::KrxActionDecl::SIZE,
                                );
                                (*dst).pkg_idx = len as u8;
                                self.actions.set_len(alen + 1);
                            }
                            off += kernel::runtime::KrxActionDecl::SIZE;
                        }
                    }
                }
            }
        }

        let title_str = title.unwrap_or_else(|| {
            let stem_len = if is_pkg { n - 4 } else { n };
            unsafe { core::str::from_utf8_unchecked(&bytes[..stem_len]) }
        });
        entry.title.len = write_trunc_utf8(&mut entry.title.buf, 0, title_str);
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
            Some(kernel::runtime::KrxClass::Paged)
        );
        assert_eq!(summary.exec_type, KrxExecType::Module);
        assert_ne!(
            summary.type_flags & kernel::runtime::SectionType::FLAG_SYSTEM_MENU,
            0
        );
        assert_eq!(summary.title(), "Crash Logs");
        assert!(summary.exec().is_some());
        assert_eq!(summary.aux.leaf_map.map(|m| m.len()), Some(17 * 16));
    }
}
