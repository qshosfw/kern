//! Dynamic Application Package Catalog & Discovery.
//!
//! Scans KernFS (`/apps/`) for installable packages (`.app` and `.krx`),
//! extracts identity metadata, flags invalid/corrupted files at the top,
//! and caches entries for the OS launcher and Settings manager.

use crate::services::SystemStorage;
use kernel::runtime::KrxExecType;
#[cfg(feature = "filesystem")]
use kernel::runtime::KrxPreamble;
use utils::format::InlineStr;

/// Cached catalog entry for an installed package (`48 B` fixed stride, `4 B` aligned;
/// all `u8`/`bool` fields sit in offsets `0..31` for 1-insn Cortex-M0+ `ldrb`).
#[derive(Debug, Clone)]
#[repr(C, align(4))]
pub struct PackageCatalogEntry {
    pub exec_type: KrxExecType,
    pub is_valid: bool,
    pub icon_id: u16,
    /// Self-contained 8x8 monochrome icon bitmap from `SECTION_ASSETS` (`[0; 8]` when absent).
    /// Placed at offset 4 (4-byte aligned) to avoid Cortex-M0+ `__aeabi_uwrite4` traps.
    pub icon_8: [u8; 8],
    /// Reserved alignment padding (`48 B` stride).
    pub icon_16_off: u16,
    pub title: InlineStr<12>,
    pub path: InlineStr<20>,
}

const _: () = assert!(core::mem::size_of::<PackageCatalogEntry>() == 48);

impl PackageCatalogEntry {
    #[inline(always)]
    pub fn filename(&self) -> &str {
        extract_filename(self.path.as_str())
    }

    #[inline(always)]
    pub fn has_custom_icon_8(&self) -> bool {
        let w = unsafe { &*(&self.icon_8 as *const [u8; 8] as *const [u32; 2]) };
        (w[0] | w[1]) != 0
    }

    /// Resolve the package's 8x8 icon: prefers embedded `SECTION_ASSETS` `ASSET_ICON_8`,
    /// falling back to the legacy `icon_id` lookup.
    pub fn resolved_icon_8(&self) -> &'static [u8; 8] {
        if self.has_custom_icon_8() {
            unsafe { &*(&self.icon_8 as *const [u8; 8]) }
        } else {
            gfx::icons::system_icon(u8::try_from(self.icon_id).unwrap_or(::abi::icon::APPS))
                .unwrap_or(gfx::icons::ICON_APPS)
        }
    }
}

utils::static_bytes!(static PKG_EXTS = b"appsvcdmndrvmodkrx";);

#[cfg(feature = "filesystem")]
fn is_package_ext(bytes: &[u8]) -> bool {
    let n = bytes.len();
    if n <= 4 {
        return false;
    }
    let bp = bytes.as_ptr();
    if unsafe { *bp.add(n - 4) } != b'.' {
        return false;
    }
    let (e0, e1, e2) = unsafe { (*bp.add(n - 3), *bp.add(n - 2), *bp.add(n - 1)) };
    let pp = PKG_EXTS.as_ptr();
    let mut i = 0;
    while i + 3 <= PKG_EXTS.len() {
        if unsafe { e0 == *pp.add(i) && e1 == *pp.add(i + 1) && e2 == *pp.add(i + 2) } {
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
pub const MAX_CATALOG_ACTIONS: usize = 2;

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

    /// Perform a zero-allocation discovery pass across `/apps/` in storage.
    #[inline(always)]
    #[cfg(feature = "filesystem")]
    pub fn scan(&mut self, filesystem: &mut SystemStorage) {
        if self.scanned {
            return;
        }
        self.scan_internal(filesystem);
    }

    #[cfg(feature = "filesystem")]
    fn scan_internal(&mut self, filesystem: &mut SystemStorage) {
        self.entries.clear();
        self.actions.clear();

        let _ = filesystem.for_each_entry("/apps", |name, size, _perm, is_dir| {
            let base_name = extract_filename(name);
            let bytes = base_name.as_bytes();
            let len = self.entries.len();
            if is_dir
                || base_name.is_empty()
                || len >= MAX_CATALOG_PACKAGES
                || bytes.ends_with(b".mod")
            {
                return;
            }
            let entry = unsafe {
                let ptr = self.entries.as_mut_ptr().add(len);
                core::ptr::write_bytes(ptr, 0, 1);
                self.entries.set_len(len + 1);
                &mut *ptr
            };
            entry.exec_type = KrxExecType::App;
            entry.icon_16_off = (size > 0) as u16;
            unsafe {
                core::ptr::copy_nonoverlapping(b"/apps/".as_ptr(), entry.path.buf.as_mut_ptr(), 6);
            }
            entry.path.len = utils::format::copy_trunc_utf8(&mut entry.path.buf, 6, base_name);
        });

        let mut write = 0usize;
        let total = self.entries.len();
        for read in 0..total {
            if self.inspect_entry_at(filesystem, read, write) {
                if write != read {
                    unsafe {
                        let ptr = self.entries.as_mut_ptr();
                        core::ptr::copy_nonoverlapping(ptr.add(read), ptr.add(write), 1);
                    }
                }
                write += 1;
            }
        }
        unsafe { self.entries.set_len(write) };

        self.scanned = true;
    }

    #[cfg(feature = "filesystem")]
    fn inspect_entry_at(
        &mut self,
        filesystem: &mut SystemStorage,
        idx: usize,
        dst_idx: usize,
    ) -> bool {
        let has_data = self.entries[idx].icon_16_off != 0;
        self.entries[idx].icon_16_off = 0;
        let path_copy = self.entries[idx].path;
        let base_name = extract_filename(path_copy.as_str());
        let bytes = base_name.as_bytes();
        let is_pkg = is_package_ext(bytes);
        let stem_len = if is_pkg { bytes.len() - 4 } else { bytes.len() };
        let stem = unsafe { core::str::from_utf8_unchecked(bytes.get_unchecked(..stem_len)) };

        if is_pkg && has_data {
            if let Ok((_hdr_buf, summary)) =
                crate::runtime::read_krx_summary_in_scratch(filesystem, path_copy.as_str())
            {
                let is_valid = summary.preamble.execution_class().is_some();
                if is_valid && summary.exec_type != KrxExecType::App {
                    return false;
                }
                let entry = &mut self.entries[idx];
                entry.is_valid = is_valid;
                entry.exec_type = summary.exec_type;
                entry.icon_id = summary.icon_id;
                let title = summary.title.filter(|t| !t.is_empty()).unwrap_or(stem);
                entry.title.set(title);
                if is_valid {
                    if let Some(assets) = summary.aux.assets
                        && let Some((_, _, ic8)) =
                            KrxPreamble::find_icon_in_assets(assets, kernel::runtime::ASSET_ICON_8)
                        && ic8.len() >= 8
                    {
                        entry.icon_8.copy_from_slice(&ic8[..8]);
                    }
                    let act_body = summary.aux.actions.unwrap_or(&[]);
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
                            (*dst).pkg_idx = dst_idx as u8;
                            self.actions.set_len(alen + 1);
                        }
                        off += kernel::runtime::KrxActionDecl::SIZE;
                    }
                }
                return true;
            }
        }
        self.entries[idx].title.set(stem);
        true
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

        assert_eq!(
            summary.preamble.execution_class(),
            Some(kernel::runtime::KrxClass::Paged)
        );
        assert_eq!(summary.exec_type, KrxExecType::Module);
        assert!(
            summary.aux.actions.is_none(),
            "panic.mod is a boot-time panic screen and should not register a Settings page"
        );
        assert_eq!(summary.title(), "Panic");
        assert_eq!(
            summary.abi,
            ::abi::VERSION,
            "rebuild packages: ./kern pkg all"
        );
        assert!(summary.exec().is_some());
        #[cfg(feature = "krx_native")]
        {
            let map = summary.aux.leaf_map.expect("panic.mod must carry leaf_map");
            let mut leaves = kernel::runtime::paged::StreamingLeafState::new(0, 1024);
            let tbl_off = (map.as_ptr() as usize - PANIC_MOD.as_ptr() as usize) as u32;
            let count = (map.len() / kernel::runtime::LeafEntry::SIZE) as u8;
            leaves.configure(0, 1024, tbl_off, count);
            assert_eq!(leaves.leaf_count, 10);
            let (entry, hit) = leaves
                .begin_leaf_call(10, |off| {
                    kernel::runtime::LeafEntry::parse(&PANIC_MOD[off as usize..])
                })
                .expect("leaf 10 must resolve via O(1) flash table seek");
            assert!(!hit);
            assert_eq!(entry.leaf_id, 10);
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

        let cat = PackageCatalog::new();
        let _ = cat;
        let mut entry = PackageCatalogEntry {
            exec_type: KrxExecType::App,
            is_valid: true,
            title: InlineStr::from_str("Hello Rust"),
            path: InlineStr::from_str("/apps/hello_rust.krx"),
            icon_16_off: 128,
            icon_id: summary.icon_id,
            icon_8: [0; 8],
        };
        entry.icon_8.copy_from_slice(bits8);
        assert_eq!(entry.resolved_icon_8(), bits8);
        assert_eq!(bits16.len(), 32);
    }

    #[test]
    fn core_settings_krx_package_validates_header_and_icons() {
        const SETTINGS_KRX: &[u8] =
            include_bytes!("../../../../../packages/core/settings/settings.krx");
        let hdr_len = KrxPreamble::MAX_HEADER_SIZE.min(SETTINGS_KRX.len());
        let summary = KrxPreamble::summarize(&SETTINGS_KRX[..hdr_len])
            .expect("settings.krx header must parse");
        assert_eq!(summary.title(), "Settings");
        assert_eq!(summary.abi, ::abi::VERSION);
        assert_eq!(
            summary.preamble.execution_class(),
            Some(kernel::runtime::KrxClass::Paged)
        );
        let (w8, h8, bits8) =
            KrxPreamble::find_icon_asset(SETTINGS_KRX, kernel::runtime::ASSET_ICON_8)
                .expect("settings.krx must carry its 8x8 icon in SECTION_ASSETS");
        assert_eq!((w8, h8, bits8.len()), (8, 8, 8));
        assert_eq!(bits8, gfx::icons::ICON_APP_SETTINGS);
        let (w16, h16, bits16) =
            KrxPreamble::find_icon_asset(SETTINGS_KRX, kernel::runtime::ASSET_ICON_16)
                .expect("settings.krx must carry its 16x16 icon in SECTION_ASSETS");
        assert_eq!((w16, h16, bits16.len()), (16, 16, 32));
        assert_eq!(bits16, gfx::icons::ICON_16_SETTINGS);
        #[cfg(feature = "krx_native")]
        {
            let map = summary
                .aux
                .leaf_map
                .expect("settings.krx must carry SECTION_LEAF_MAP");
            let count = map.len() / kernel::runtime::LeafEntry::SIZE;
            assert!(
                count >= 20,
                "settings.krx should carry its modular leaf set"
            );
            let tbl_off = (map.as_ptr() as usize - SETTINGS_KRX.as_ptr() as usize) as u32;
            let mut leaves = kernel::runtime::paged::StreamingLeafState::new(0, 1024);
            leaves.configure(0, 1024, tbl_off, count as u8);
            let (last, hit) = leaves
                .begin_leaf_call(count as u8, |off| {
                    kernel::runtime::LeafEntry::parse(&SETTINGS_KRX[off as usize..])
                })
                .expect("last leaf must resolve via O(1) flash table seek");
            assert!(!hit);
            assert_eq!(last.leaf_id, count as u8);
        }
    }

    #[test]
    fn core_vfo_krx_package_validates_header_and_icons() {
        const VFO_KRX: &[u8] = include_bytes!("../../../../../packages/core/vfo/vfo.krx");
        let hdr_len = KrxPreamble::MAX_HEADER_SIZE.min(VFO_KRX.len());
        let summary = KrxPreamble::summarize(&VFO_KRX[..hdr_len]).expect("vfo.krx header parses");
        assert_eq!(summary.title(), "Radio");
        assert_eq!(summary.abi, ::abi::VERSION);
        assert_eq!(
            summary.preamble.execution_class(),
            Some(kernel::runtime::KrxClass::Native)
        );
        let (w8, h8, bits8) = KrxPreamble::find_icon_asset(VFO_KRX, kernel::runtime::ASSET_ICON_8)
            .expect("vfo.krx must carry its 8x8 icon in SECTION_ASSETS");
        assert_eq!((w8, h8, bits8.len()), (8, 8, 8));
        assert_eq!(bits8, gfx::icons::ICON_APP_RADIO);
        let (w16, h16, bits16) =
            KrxPreamble::find_icon_asset(VFO_KRX, kernel::runtime::ASSET_ICON_16)
                .expect("vfo.krx must carry its 16x16 icon in SECTION_ASSETS");
        assert_eq!((w16, h16, bits16.len()), (16, 16, 32));
        assert_eq!(bits16, gfx::icons::ICON_16_RADIO);
    }
}
