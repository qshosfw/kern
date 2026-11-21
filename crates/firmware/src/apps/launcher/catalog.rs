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
    /// Union of `abi::scope` bits over every declared action.
    pub scopes: u8,
    /// `0xFF` when no custom 16x16 asset bitmap was embedded in `SECTION_ASSETS`.
    pub icon_16_slot: u8,
    pub icon_id: u16,
    pub caps: u32,
    /// Self-contained 8x8 monochrome icon bitmap from `SECTION_ASSETS` (`[0; 8]` when absent).
    pub icon_8: [u8; 8],
}

const _: () = assert!(core::mem::size_of::<PackageCatalogEntry>() == 64);

impl PackageCatalogEntry {
    #[inline(always)]
    pub fn filename(&self) -> &str {
        extract_filename(self.path.as_str())
    }

    #[inline(always)]
    pub fn has_custom_icon_8(&self) -> bool {
        self.icon_8 != [0u8; 8]
    }

    /// Resolve the package's 8x8 icon: prefers embedded `SECTION_ASSETS` `ASSET_ICON_8`,
    /// falling back to the legacy `icon_id` lookup.
    #[inline(always)]
    pub fn resolved_icon_8(&self) -> &'static [u8; 8] {
        if self.has_custom_icon_8() {
            unsafe { &*(&self.icon_8 as *const [u8; 8]) }
        } else {
            crate::apps::settings::apps::package_icon_8(self.title.as_str(), self.icon_id)
        }
    }

    #[inline(always)]
    pub fn is_system_entry(&self, sys_caps: ::abi::CapabilitySet, scope: u8) -> bool {
        self.is_valid
            && self.exec_type == KrxExecType::Module
            && (self.scopes & scope) != 0
            && sys_caps.contains_all(::abi::CapabilitySet(self.caps))
    }

    #[inline(always)]
    pub fn is_system_module(&self, sys_caps: ::abi::CapabilitySet) -> bool {
        self.is_system_entry(sys_caps, ::abi::scope::SYSTEM_PAGE)
    }
}

utils::static_bytes!(static PKG_EXTS = b"appsvcdmndrvmodkrx";);

#[cfg(feature = "filesystem")]
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
pub const MAX_CATALOG_APP_ICONS_16: usize = 4;

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
    pub icons_16: [[u8; 32]; MAX_CATALOG_APP_ICONS_16],
    pub icons_16_len: u8,
    pub scanned: bool,
}

impl PackageCatalog {
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
            actions: heapless::Vec::new(),
            icons_16: [[0u8; 32]; MAX_CATALOG_APP_ICONS_16],
            icons_16_len: 0,
            scanned: false,
        }
    }

    /// Resolve both 8x8 and 16x16 icons for a catalog entry, preferring embedded
    /// `SECTION_ASSETS` bitmaps (`ASSET_ICON_8`, `ASSET_ICON_16`) over fallback ROM glyphs.
    pub fn resolved_icons(
        &self,
        entry: &PackageCatalogEntry,
    ) -> (&'static [u8; 8], &'static [u8; 32]) {
        let (fb8, fb16) =
            crate::apps::settings::apps::package_icons(entry.title.as_str(), entry.icon_id);
        let icon_8 = if entry.has_custom_icon_8() {
            unsafe { &*(&entry.icon_8 as *const [u8; 8]) }
        } else {
            fb8
        };
        let slot = entry.icon_16_slot as usize;
        let icon_16 = if slot < (self.icons_16_len as usize) {
            unsafe { &*(&self.icons_16[slot] as *const [u8; 32]) }
        } else {
            fb16
        };
        (icon_8, icon_16)
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
        self.icons_16_len = 0;

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
        let name_len = gfx::text::find_byte(&dir.name, 0).unwrap_or(24);
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
        entry.exec_type = KrxExecType::App;
        entry.icon_16_slot = 0xFF;

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
            if let Ok(summary) = crate::runtime::read_krx_summary(
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
                let is_app = entry.exec_type == KrxExecType::App;
                if entry.is_valid {
                    entry.has_config = is_app && summary.aux.config.is_some();
                    let act_body = summary.aux.actions.unwrap_or(&[]);
                    let mut off = 0usize;
                    while off + kernel::runtime::KrxActionDecl::SIZE <= act_body.len() {
                        entry.scopes |= act_body[off + 2];
                        let alen = self.actions.len();
                        if is_app && alen < MAX_CATALOG_ACTIONS {
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
                        }
                        off += kernel::runtime::KrxActionDecl::SIZE;
                    }
                }
            }
            if let Some(t) = title {
                entry.title.len = write_trunc_utf8(&mut entry.title.buf, 0, t);
            }
            if entry.is_valid {
                if let Some((_, _, ic8)) =
                    KrxPreamble::find_icon_asset(hdr_buf, kernel::runtime::ASSET_ICON_8)
                {
                    if ic8.len() >= 8 {
                        entry.icon_8.copy_from_slice(&ic8[..8]);
                    }
                }
                if entry.exec_type == KrxExecType::App
                    && (self.icons_16_len as usize) < MAX_CATALOG_APP_ICONS_16
                {
                    if let Some((_, _, ic16)) =
                        KrxPreamble::find_icon_asset(hdr_buf, kernel::runtime::ASSET_ICON_16)
                    {
                        if ic16.len() >= 32 {
                            let slot = self.icons_16_len;
                            self.icons_16[slot as usize].copy_from_slice(&ic16[..32]);
                            entry.icon_16_slot = slot;
                            self.icons_16_len = slot + 1;
                        }
                    }
                }
            }
        }

        if entry.title.len == 0 {
            let stem_len = if is_pkg { n - 4 } else { n };
            let stem = unsafe { core::str::from_utf8_unchecked(&bytes[..stem_len]) };
            entry.title.len = write_trunc_utf8(&mut entry.title.buf, 0, stem);
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
        const PANIC_MOD: &[u8] = include_bytes!("../../../../../packages/sys/panic/panic.mod");
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
        let action = kernel::runtime::KrxActionDecl::parse(summary.aux.actions.unwrap(), 0)
            .expect("panic.mod must declare its Settings placement as an action");
        assert_eq!(action.scope_mask, ::abi::scope::SYSTEM_PAGE);
        assert_eq!(summary.title(), "Crash Logs");
        assert_eq!(
            summary.abi,
            ::abi::VERSION,
            "rebuild packages: ./kern pkg all"
        );
        assert!(summary.exec().is_some());
        #[cfg(feature = "krx_native")]
        {
            let mut leaves = kernel::runtime::paged::StructuredLeafManager::<
                { crate::runtime::native_runner::MAX_STRUCTURED_LEAVES },
            >::new(0, 1024);
            leaves.configure(0, 1024, summary.aux.leaf_map);
            assert_eq!(leaves.leaf_count(), 18);
            assert_eq!(leaves.leaf_at(17).map(|entry| entry.leaf_id), Some(18));
        }
    }

    #[test]
    fn rust_krx_package_and_self_contained_icon_assets_validate() {
        const HELLO_RUST: &[u8] =
            include_bytes!("../../../../../packages/examples/hello_rust/hello_rust.krx");
        let hdr_len = KrxPreamble::MAX_HEADER_SIZE.min(HELLO_RUST.len());
        let summary = KrxPreamble::summarize(&HELLO_RUST[..hdr_len])
            .expect("hello_rust.krx header must parse");
        assert_eq!(summary.title(), "Hello Rust");
        assert_eq!(summary.abi, ::abi::VERSION);
        assert_eq!(
            summary.preamble.execution_class(),
            Some(kernel::runtime::KrxClass::Native)
        );
        let (w8, h8, bits8) =
            KrxPreamble::find_icon_asset(HELLO_RUST, kernel::runtime::ASSET_ICON_8)
                .expect("hello_rust.krx must carry its 8x8 icon in SECTION_ASSETS");
        assert_eq!((w8, h8, bits8.len()), (8, 8, 8));
        assert_eq!(bits8[0], 0b0011_1100);
        let (w16, h16, bits16) =
            KrxPreamble::find_icon_asset(HELLO_RUST, kernel::runtime::ASSET_ICON_16)
                .expect("hello_rust.krx must carry its 16x16 icon in SECTION_ASSETS");
        assert_eq!((w16, h16, bits16.len()), (16, 16, 32));

        let mut cat = PackageCatalog::new();
        let mut entry = PackageCatalogEntry {
            app_class: kernel::runtime::KrxClass::Native,
            exec_type: KrxExecType::App,
            lifecycle_flags: 0,
            is_valid: true,
            has_config: false,
            title: SmallStr::from_str("Hello Rust"),
            path: SmallStr::from_str("/apps/hello_rust.krx"),
            scopes: 0,
            icon_16_slot: 0,
            icon_id: summary.icon_id,
            caps: summary.caps.raw(),
            icon_8: [0; 8],
        };
        entry.icon_8.copy_from_slice(bits8);
        cat.icons_16[0].copy_from_slice(bits16);
        cat.icons_16_len = 1;

        let (res8, res16) = cat.resolved_icons(&entry);
        assert_eq!(res8, bits8);
        assert_eq!(res16, bits16);
    }
}
