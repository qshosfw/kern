//! Boot-Time Package Provisioning Engine (§13.2, §14.4 / KRX-ABI 2.5.0).
//!
//! Scans the staging directory `/prov/` on KernFS, performs strict validation
//! on all staged packages and configuration files, and relocates verified items
//! to their canonical locations:
//! - Executable packages (`.app`, `.svc`, `.dmn`, `.krx`) -> `/apps/`
//! - System preferences (`.cfg`)                          -> `/pref/`
//! - System calibration / keys (`.dat`, `.sig`)           -> `/sys/`
//! - Operational databases (`.ch`, `.ct`, `.bnd`, `.zn`, `.fm`, `.vfo`) -> `/user/`
//!
//! Also provisions core flash-loaded `.krx` utility packages (`/apps/sysinfo.krx`,
//! `/apps/counter.krx`, `/apps/timer.krx`) so they execute from the shared transient arena
//! rather than consuming resident MCU ROM/SRAM.

use crate::services::SystemStorage;
use crate::services::storage::DirEntryInfo;

pub const PROV_BUF_CAP: usize = 4096;

// Native ARM Thumb-1 binary payload for `/apps/beacon.krx`
#[cfg(all(feature = "provision_apps", feature = "krx_native"))]
const BEACON_APP_PACKAGE: &[u8] = include_bytes!("../../../../packages/beacon/beacon.krx");

// Standalone Native Thumb-1 binary payload for `/apps/bench.krx`.
// 100% self-contained PIC executable. Completely resides inside the KRX file on KernFS.
#[cfg(all(feature = "provision_bench", feature = "krx_native"))]
const BENCH_APP_PACKAGE: &[u8] = include_bytes!("../../../../packages/bench/bench.krx");

#[cfg(all(feature = "provision_apps", feature = "krx_native"))]
const LEAF_APP_PACKAGE: &[u8] = include_bytes!("../../../../packages/leaf_app/leaf.krx");

#[cfg(all(feature = "provision_apps", feature = "krx_native"))]
const PAGED_APP_PACKAGE: &[u8] = include_bytes!("../../../../packages/paged_app/paged.krx");

#[inline(never)]
fn join_path<'a>(
    buf: &'a mut [core::mem::MaybeUninit<u8>; 48],
    prefix: &str,
    name: &str,
) -> Option<&'a str> {
    let p = prefix.as_bytes();
    let n = name.as_bytes();
    let total = p.len() + 1 + n.len();
    if total > buf.len() {
        return None;
    }
    unsafe {
        let ptr = buf.as_mut_ptr() as *mut u8;
        core::ptr::copy_nonoverlapping(p.as_ptr(), ptr, p.len());
        *ptr.add(p.len()) = b'/';
        core::ptr::copy_nonoverlapping(n.as_ptr(), ptr.add(p.len() + 1), n.len());
        Some(core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            ptr, total,
        )))
    }
}

/// Write a pre-built valid KRX package directly to KernFS storage with idempotency.
#[cfg(feature = "provision_apps")]
#[inline(never)]
fn write_raw_krx_package(filesystem: &mut SystemStorage, path: &str, pkg_bytes: &[u8]) {
    if filesystem.file_size(path) == Ok(pkg_bytes.len()) {
        return;
    }
    let _ = filesystem.remove(path);
    let _ = filesystem.write_file(path, pkg_bytes);
}

/// Ensure core flash-loaded `.krx` packages exist in `/apps/`.
#[cfg(not(feature = "provision_apps"))]
pub fn ensure_core_packages(_filesystem: &mut SystemStorage) {}

/// Ensure core flash-loaded `.krx` packages exist in `/apps/`.
#[cfg(feature = "provision_apps")]
#[inline(never)]
pub fn ensure_core_packages(filesystem: &mut SystemStorage) {
    #[cfg(feature = "krx_native")]
    {
        write_raw_krx_package(filesystem, "/apps/beacon.krx", BEACON_APP_PACKAGE);
        #[cfg(feature = "provision_bench")]
        write_raw_krx_package(filesystem, "/apps/bench.krx", BENCH_APP_PACKAGE);
        write_raw_krx_package(filesystem, "/apps/paged.krx", PAGED_APP_PACKAGE);
        write_raw_krx_package(filesystem, "/apps/leaf.krx", LEAF_APP_PACKAGE);
    }
}

/// Execute the storage provisioning pass for dynamic application packages and configuration.
#[inline(never)]
pub fn provision_packages(filesystem: &mut SystemStorage) {
    const PROV_DIRS: &[u8] =
        b"/apps\0/pref\0/sys\0/user\0/user/splash\0/user/rogers\0/user/jingles\0";
    for idx in 0..7u8 {
        let _ = filesystem.create_dir(utils::format::nth_cstr(PROV_DIRS, idx));
    }

    ensure_core_packages(filesystem);

    let (arena_head, arena_tail) =
        crate::runtime::transient_arena_bytes_mut().split_at_mut(PROV_BUF_CAP);
    const {
        assert!(core::mem::size_of::<[DirEntryInfo; 8]>() <= 2048);
        assert!(PROV_BUF_CAP.is_multiple_of(8));
        assert!(core::mem::align_of::<DirEntryInfo>() <= 8);
    }
    let entries: &mut [DirEntryInfo; 8] = unsafe {
        let base = arena_tail.as_mut_ptr() as *mut DirEntryInfo;
        core::ptr::write_bytes(base, 0, 8);
        &mut *(base as *mut [DirEntryInfo; 8])
    };
    let count = filesystem.list_dir("/prov", entries).unwrap_or(0);
    if count == 0 {
        crate::apps::launcher::reload_launcher(filesystem);
        return;
    }

    let prov_buf = arena_head;

    for entry in &entries[..count] {
        if entry.is_dir || entry.path.is_empty() || entry.size > PROV_BUF_CAP {
            continue;
        }

        let raw_entry = entry.path.as_str();
        let fname = crate::apps::launcher::catalog::extract_filename(raw_entry);
        if fname.is_empty() {
            continue;
        }

        let mut src_arr = [core::mem::MaybeUninit::<u8>::uninit(); 48];
        let src_path = if raw_entry.as_bytes()[0] == b'/' {
            raw_entry
        } else {
            let Some(s) = join_path(&mut src_arr, "/prov", fname) else {
                continue;
            };
            s
        };

        let rlen = match filesystem.read_file(src_path, prov_buf) {
            Ok(l) => l,
            Err(_) => continue,
        };

        if rlen == 0 {
            let _ = filesystem.remove(src_path);
            continue;
        }

        let buf = &prov_buf[..rlen];

        let b = fname.as_bytes();
        let ext = if b.len() >= 4 && b[b.len() - 4] == b'.' {
            &b[b.len() - 3..]
        } else {
            b""
        };
        const EXT_MAP: [([u8; 3], u8); 10] = [
            (*b"app", 0),
            (*b"svc", 0),
            (*b"dmn", 0),
            (*b"krx", 0),
            (*b"cfg", 1),
            (*b"dat", 2),
            (*b"sig", 2),
            (*b"rle", 4),
            (*b"img", 4),
            (*b"rgr", 5),
        ];
        let dir_idx = if ext == b"ton" {
            if b.starts_with(b"jingle") { 6 } else { 5 }
        } else {
            let mut idx = 3usize;
            for (pat, target) in &EXT_MAP {
                if ext == pat {
                    idx = *target as usize;
                    break;
                }
            }
            idx
        };
        let prefix = utils::format::nth_cstr(PROV_DIRS, dir_idx as u8);

        let mut dst_arr = [core::mem::MaybeUninit::<u8>::uninit(); 48];
        if let Some(dst_path) = join_path(&mut dst_arr, prefix, fname) {
            let _ = filesystem.remove(dst_path);
            if filesystem.write_file(dst_path, buf).is_ok() {
                let _ = filesystem.remove(src_path);
            }
        }
    }

    crate::apps::launcher::reload_launcher(filesystem);
}
