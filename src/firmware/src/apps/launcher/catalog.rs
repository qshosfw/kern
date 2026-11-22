//! Dynamic Application Package Catalog & Discovery.
//!
//! Scans KernFS (`/apps/`) for installable packages (`.app` and `.krx`),
//! extracts identity metadata, flags invalid/corrupted files at the top,
//! and caches entries for the OS launcher and Settings manager.

use crate::services::SystemStorage;
use kernel::runtime::KrxExecType;
#[cfg(feature = "filesystem")]
use kernel::runtime::KrxPreamble;

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

/// Cached catalog entry for an installed package (`48 B` fixed stride, `4 B` aligned;
/// all `u8`/`bool` fields sit in offsets `0..31` for 1-insn Cortex-M0+ `ldrb`).
#[derive(Debug, Clone)]
#[repr(C, align(4))]
pub struct PackageCatalogEntry {
    pub exec_type: KrxExecType,
    pub is_valid: bool,
    pub icon_id: u16,
    /// File offset of embedded 16x16 bitmap inside `SECTION_ASSETS` (`0` when absent).
    pub icon_16_off: u16,
    /// Self-contained 8x8 monochrome icon bitmap from `SECTION_ASSETS` (`[0; 8]` when absent).
    pub icon_8: [u8; 8],
    pub title: SmallStr<12>,
    pub path: SmallStr<20>,
}

const _: () = assert!(core::mem::size_of::<PackageCatalogEntry>() == 48);

impl PackageCatalogEntry {
    #[inline(always)]
    pub fn filename(&self) -> &str {
        extract_filename(self.path.as_str())
    }

    #[inline(always)]
    pub fn has_custom_icon_8(&self) -> bool {
        self.icon_8.iter().any(|&b| b != 0)
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

    /// Resolve the package's fallback 16x16 ROM icon from `icon_id`.
    #[inline(always)]
    pub fn resolved_icon_16(&self) -> &'static [u8; 32] {
        gfx::icons::icon_16(u8::try_from(self.icon_id).unwrap_or(::abi::icon::APPS))
    }

    /// Resolve both 8x8 and 16x16 icons for this entry, preferring embedded `SECTION_ASSETS`
    /// bitmaps (`ASSET_ICON_8`, and `custom_icon_16` read on demand for `Launcher`) over ROM glyphs.
    #[inline(always)]
    pub fn resolved_icons(
        &self,
        custom_icon_16: Option<&'static [u8; 32]>,
    ) -> (&'static [u8; 8], &'static [u8; 32]) {
        let icon_16 = custom_icon_16.unwrap_or_else(|| self.resolved_icon_16());
        (self.resolved_icon_8(), icon_16)
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
pub const MAX_CATALOG_ACTIONS: usize = 4;

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

    #[inline(always)]
    pub fn resolved_icons(
        &self,
        entry: &PackageCatalogEntry,
        custom_icon_16: Option<&'static [u8; 32]>,
    ) -> (&'static [u8; 8], &'static [u8; 32]) {
        entry.resolved_icons(custom_icon_16)
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

    #[inline(never)]
    #[cfg(feature = "filesystem")]
    fn scan_internal(&mut self, filesystem: &mut SystemStorage) {
        self.entries.clear();
        self.actions.clear();

        let mut dir_entry = kernel::proc::abi::KrxDirEntry {
            name: [0; 24],
            size: 0,
            is_dir: 0,
            reserved: [0; 3],
        };
        let mut off = 0usize;
        while crate::services::primitives::list_krx_dir_at(
            filesystem,
            "/apps",
            off,
            core::slice::from_mut(&mut dir_entry),
        )
        .is_ok_and(|(_, written)| written > 0)
        {
            off += 1;
            if dir_entry.is_dir == 0 && self.entries.len() < MAX_CATALOG_PACKAGES {
                self.inspect_package(filesystem, &dir_entry);
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
        let bytes = base_name.as_bytes();
        let n = bytes.len();
        if base_name.is_empty() || len >= MAX_CATALOG_PACKAGES || bytes.ends_with(b".mod") {
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
        entry.icon_16_off = 0;

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
            if let Ok((hdr_buf, summary)) =
                crate::runtime::read_krx_summary_in_scratch(filesystem, entry.path.as_str())
            {
                if summary.preamble.execution_class().is_some() {
                    entry.is_valid = true;
                }
                entry.exec_type = summary.exec_type;
                if entry.is_valid && entry.exec_type != KrxExecType::App {
                    unsafe {
                        self.entries.set_len(len);
                    }
                    return;
                }
                entry.icon_id = summary.icon_id;
                title = summary.title;
                if entry.is_valid {
                    let act_body = summary.aux.actions.unwrap_or(&[]);
                    let mut off = 0usize;
                    while off + kernel::runtime::KrxActionDecl::SIZE <= act_body.len() {
                        let alen = self.actions.len();
                        if alen < MAX_CATALOG_ACTIONS {
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
                    if let Some((_, _, ic8)) =
                        KrxPreamble::find_icon_asset(hdr_buf, kernel::runtime::ASSET_ICON_8)
                        && ic8.len() >= 8
                    {
                        entry.icon_8.copy_from_slice(&ic8[..8]);
                    }
                    if let Some((_, _, ic16)) =
                        KrxPreamble::find_icon_asset(hdr_buf, kernel::runtime::ASSET_ICON_16)
                        && ic16.len() >= 32
                    {
                        let off = (ic16.as_ptr() as usize) - (hdr_buf.as_ptr() as usize);
                        if off <= (u16::MAX as usize) {
                            entry.icon_16_off = off as u16;
                        }
                    }
                }
            }
            if let Some(t) = title {
                entry.title.len = write_trunc_utf8(&mut entry.title.buf, 0, t);
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
            let mut leaves = kernel::runtime::paged::StructuredLeafManager::<
                { crate::runtime::native_runner::MAX_STRUCTURED_LEAVES },
            >::new(0, 1024);
            leaves.configure(0, 1024, summary.aux.leaf_map);
            assert_eq!(leaves.leaf_count(), 10);
            assert_eq!(leaves.leaf_at(9).map(|entry| entry.leaf_id), Some(10));
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
        let mut entry = PackageCatalogEntry {
            exec_type: KrxExecType::App,
            is_valid: true,
            title: SmallStr::from_str("Hello Rust"),
            path: SmallStr::from_str("/apps/hello_rust.krx"),
            icon_16_off: 128,
            icon_id: summary.icon_id,
            icon_8: [0; 8],
        };
        entry.icon_8.copy_from_slice(bits8);
        let mut icon16_buf = [0u8; 32];
        icon16_buf.copy_from_slice(bits16);
        let icon16_static: &'static [u8; 32] = unsafe { &*(&icon16_buf as *const [u8; 32]) };

        let (res8, res16) = cat.resolved_icons(&entry, Some(icon16_static));
        assert_eq!(res8, bits8);
        assert_eq!(res16, bits16);
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
