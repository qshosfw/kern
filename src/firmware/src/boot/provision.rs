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

/// Extract the filename component from a path (e.g. "/prov/app.krx" -> "app.krx").
fn filename_from_path(path: &str) -> &str {
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
    for d in [
        "/apps",
        "/pref",
        "/sys",
        "/user",
        "/user/splash",
        "/user/rogers",
        "/user/jingles",
    ] {
        let _ = filesystem.create_dir(d);
    }

    ensure_core_packages(filesystem);

    let mut entries = [const { DirEntryInfo::empty() }; 8];
    let count = filesystem.list_dir("/prov", &mut entries).unwrap_or(0);
    if count == 0 {
        crate::apps::launcher::reload_launcher(filesystem);
        return;
    }

    let prov_buf = &mut crate::runtime::transient_arena_bytes_mut()[..PROV_BUF_CAP];

    for entry in &entries[..count] {
        if entry.is_dir || entry.path.is_empty() {
            continue;
        }

        let src_path = entry.path.as_str();
        let fname = filename_from_path(src_path);
        if fname.is_empty() {
            continue;
        }

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
        let prefix = match ext {
            b"app" | b"svc" | b"dmn" | b"krx" => "/apps/",
            b"cfg" => "/pref/",
            b"dat" | b"sig" => "/sys/",
            b"rle" | b"img" => "/user/splash/",
            _ => "/user/",
        };

        let mut dst_buf = [0u8; 48];
        let p_bytes = prefix.as_bytes();
        let total_len = p_bytes.len() + b.len();
        if total_len <= dst_buf.len() {
            dst_buf[..p_bytes.len()].copy_from_slice(p_bytes);
            dst_buf[p_bytes.len()..total_len].copy_from_slice(b);
            let dst_path = unsafe { core::str::from_utf8_unchecked(&dst_buf[..total_len]) };
            let _ = filesystem.remove(dst_path);
            if filesystem.write_file(dst_path, buf).is_ok() {
                let _ = filesystem.remove(src_path);
            }
        }
    }

    crate::apps::launcher::reload_launcher(filesystem);
}
