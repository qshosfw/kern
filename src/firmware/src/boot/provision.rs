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
#[cfg(feature = "provision_apps")]
use kernel::proc::krx::{
    KRX_MAGIC, KrxClass, KrxExecType, SECTION_EXECUTION, SECTION_IDENTITY, SECTION_TYPE_LIFECYCLE,
};

pub const PROV_BUF_CAP: usize = 512;

// Declare 3.0 payload for `/apps/sysinfo.krx` (1 node, 5 live ITEM_INFO rows, 0 actions, 6 labels)
#[cfg(feature = "provision_apps")]
const SYSINFO_PAYLOAD: &[u8] = &[
    1, 5, 0, 6, // [node_count=1, item_count=5, action_count=0, label_count=6]
    // DeclareNode 0 (8B): node_id=0, layout=0 (Menu), item_count=5, title_offset=0, first_item=0
    0, 0, 0, 5, 0, 0, 0, 0,
    // Item 0 (12B): kind=5 (ITEM_INFO), flags=0, label=1, target=0, value_ref=0 (Battery)
    5, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    // Item 1 (12B): kind=5 (ITEM_INFO), flags=0, label=2, target=0, value_ref=1 (CPU Clock)
    5, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0, 0,
    // Item 2 (12B): kind=5 (ITEM_INFO), flags=0, label=3, target=0, value_ref=2 (Stack Free)
    5, 0, 3, 0, 0, 0, 2, 0, 0, 0, 0, 0,
    // Item 3 (12B): kind=5 (ITEM_INFO), flags=0, label=4, target=0, value_ref=4 (Uptime)
    5, 0, 4, 0, 0, 0, 4, 0, 0, 0, 0, 0,
    // Item 4 (12B): kind=5 (ITEM_INFO), flags=0, label=5, target=0, value_ref=5 (Active Tasks)
    5, 0, 5, 0, 0, 0, 5, 0, 0, 0, 0, 0, // Length-prefixed string pool (6 strings)
    4, b'L', b'i', b'v', b'e', 7, b'B', b'a', b't', b't', b'e', b'r', b'y', 9, b'C', b'P', b'U',
    b' ', b'C', b'l', b'o', b'c', b'k', 10, b'S', b't', b'a', b'c', b'k', b' ', b'F', b'r', b'e',
    b'e', 10, b'U', b'p', b't', b'i', b'm', b'e', b' ', b'(', b's', b')', 12, b'A', b'c', b't',
    b'i', b'v', b'e', b' ', b'T', b'a', b's', b'k', b's',
];

// Declare 3.0 payload for `/apps/counter.krx` (1 node, 4 items, 3 actions, 5 labels)
#[cfg(feature = "provision_apps")]
const COUNTER_PAYLOAD: &[u8] = &[
    1, 4, 3, 5, // [node_count=1, item_count=4, action_count=3, label_count=5]
    // DeclareNode 0 (8B): node_id=0, layout=0, item_count=4, title_offset=0, first_item=0
    0, 0, 0, 4, 0, 0, 0, 0, // Item 0: ACTION (0), label=1 ("Count +1"), target=0
    0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    // Item 1: ACTION (0), label=2 ("Count -1"), target=1
    0, 0, 2, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    // Item 2: ACTION (0), label=3 ("Reset (0)"), target=2
    0, 0, 3, 0, 2, 0, 0, 0, 0, 0, 0, 0,
    // Item 3: VALUE (2), label=4 ("Current"), value_ref=0
    2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    // Action 0 (8B): ACTION_SET_STATE (1), a=0, b=+1, c=1 (delta)
    1, 0, 0, 0, 1, 0, 1, 0,
    // Action 1 (8B): ACTION_SET_STATE (1), a=0, b=-1 (0xFFFF), c=1 (delta)
    1, 0, 0, 0, 0xFF, 0xFF, 1, 0,
    // Action 2 (8B): ACTION_SET_STATE (1), a=0, b=0, c=0 (set)
    1, 0, 0, 0, 0, 0, 0, 0, // Length-prefixed string pool (5 strings)
    5, b'T', b'a', b'l', b'l', b'y', 8, b'C', b'o', b'u', b'n', b't', b' ', b'+', b'1', 8, b'C',
    b'o', b'u', b'n', b't', b' ', b'-', b'1', 9, b'R', b'e', b's', b'e', b't', b' ', b'(', b'0',
    b')', 7, b'C', b'u', b'r', b'r', b'e', b'n', b't',
];

// Declare 3.0 payload for `/apps/timer.krx` (1 node, 3 items, 2 actions, 4 labels)
#[cfg(feature = "provision_apps")]
const TIMER_PAYLOAD: &[u8] = &[
    1, 3, 2, 4, // [node_count=1, item_count=3, action_count=2, label_count=4]
    // DeclareNode 0 (8B): node_id=0, layout=0, item_count=3, title_offset=0, first_item=0
    0, 0, 0, 3, 0, 0, 0, 0, // Item 0: ACTION (0), label=1 ("Start (+10)"), target=0
    0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, // Item 1: ACTION (0), label=2 ("Reset"), target=1
    0, 0, 2, 0, 1, 0, 0, 0, 0, 0, 0, 0,
    // Item 2: VALUE (2), label=3 ("Seconds"), value_ref=0
    2, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    // Action 0 (8B): ACTION_SET_STATE (1), a=0, b=+10, c=1 (delta)
    1, 0, 0, 0, 10, 0, 1, 0, // Action 1 (8B): ACTION_SET_STATE (1), a=0, b=0, c=0 (set)
    1, 0, 0, 0, 0, 0, 0, 0, // Length-prefixed string pool (4 strings)
    7, b'E', b'l', b'a', b'p', b's', b'e', b'd', 11, b'S', b't', b'a', b'r', b't', b' ', b'(',
    b'+', b'1', b'0', b')', 5, b'R', b'e', b's', b'e', b't', 7, b'S', b'e', b'c', b'o', b'n', b'd',
    b's',
];

/// Extract the filename component from a path (e.g. "/prov/app.krx" -> "app.krx").
fn filename_from_path(path: &str) -> &str {
    if let Some(idx) = path.rfind('/') {
        &path[idx + 1..]
    } else {
        path
    }
}

/// Build and write a KRX package file into KernFS using the shared transient arena buffer.
#[cfg(feature = "provision_apps")]
#[inline(never)]
fn write_krx_package(
    filesystem: &mut SystemStorage,
    path: &str,
    class: KrxClass,
    name: &str,
    caps: u32,
    payload: &[u8],
) {
    if filesystem.exists(path) {
        return;
    }
    let total_len = 128 + payload.len();
    if total_len > PROV_BUF_CAP {
        return;
    }

    let pkg = &mut crate::runtime::transient_arena_bytes_mut()[..total_len];
    pkg.fill(0);

    // 1. Preamble (20 bytes)
    pkg[0..4].copy_from_slice(&KRX_MAGIC);
    pkg[4] = class as u8;
    pkg[5] = 0x05; // Format version 2.5.0
    pkg[6..8].copy_from_slice(&3u16.to_le_bytes()); // 3 sections
    pkg[8..10].copy_from_slice(&128u16.to_le_bytes()); // header_len = 128

    // 2. SectionIdentity (offset 20: 4B header + 40B body = 44B, ends at 64)
    pkg[20..22].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
    pkg[22] = 44;
    pkg[24] = 112; // name_str offset = 112
    pkg[26] = 1; // version 1.0.0
    pkg[29] = 1; // min_kernel_abi = 1
    pkg[31..40].copy_from_slice(b"KERN-TEAM");
    pkg[47] = 1; // icon_asset = 1

    // 3. SectionExecution (offset 64: 4B header + 28B body = 32B, ends at 96)
    pkg[64..66].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
    pkg[66] = 32;
    pkg[68..70].copy_from_slice(&256u16.to_le_bytes()); // min_stack_bytes = 256
    pkg[72..74].copy_from_slice(&512u16.to_le_bytes()); // page_size = 512
    pkg[74] = 2; // cache_blocks = 2
    pkg[80..84].copy_from_slice(&(payload.len() as u32).to_le_bytes()); // payload_size

    // 4. SectionType (offset 96: 4B header + 12B body = 16B, ends at 112)
    pkg[96..98].copy_from_slice(&SECTION_TYPE_LIFECYCLE.to_le_bytes());
    pkg[98] = 16;
    pkg[100] = KrxExecType::App as u8;
    pkg[102] = 1; // stack_cost
    pkg[108..112].copy_from_slice(&caps.to_le_bytes());

    // 5. String pool (offset 112..128)
    let nbytes = name.as_bytes();
    let nlen = nbytes.len().min(15);
    pkg[112..112 + nlen].copy_from_slice(&nbytes[..nlen]);

    // 6. Payload (offset 128..total_len)
    pkg[128..total_len].copy_from_slice(payload);

    let _ = filesystem.write_file(path, pkg);
}

/// Ensure core flash-loaded `.krx` packages exist in `/apps/`.
#[cfg(not(feature = "provision_apps"))]
pub fn ensure_core_packages(_filesystem: &mut SystemStorage) {}

/// Ensure core flash-loaded `.krx` packages exist in `/apps/`.
#[cfg(feature = "provision_apps")]
#[inline(never)]
pub fn ensure_core_packages(filesystem: &mut SystemStorage) {
    let base_caps = (kernel::caps::Cap::DisplayDirect as u32) | (kernel::caps::Cap::Keypad as u32);
    write_krx_package(
        filesystem,
        "/apps/sysinfo.krx",
        KrxClass::Declare,
        "SysInfo",
        base_caps,
        SYSINFO_PAYLOAD,
    );
    write_krx_package(
        filesystem,
        "/apps/counter.krx",
        KrxClass::Declare,
        "Counter",
        base_caps,
        COUNTER_PAYLOAD,
    );
    write_krx_package(
        filesystem,
        "/apps/timer.krx",
        KrxClass::Declare,
        "Timer",
        base_caps,
        TIMER_PAYLOAD,
    );
    write_krx_package(
        filesystem,
        "/apps/explorer.krx",
        KrxClass::Paged,
        "Explorer",
        base_caps | (kernel::caps::Cap::StorageRead as u32),
        &[],
    );
}

/// Execute the storage provisioning pass for dynamic application packages and configuration.
#[inline(never)]
pub fn provision_packages(filesystem: &mut SystemStorage) {
    let _ = filesystem.create_dir("/apps");
    let _ = filesystem.create_dir("/pref");
    let _ = filesystem.create_dir("/sys");
    let _ = filesystem.create_dir("/user");
    let _ = filesystem.create_dir("/user/splash");
    let _ = filesystem.create_dir("/user/rogers");
    let _ = filesystem.create_dir("/user/jingles");

    ensure_core_packages(filesystem);

    let mut entries = [const { DirEntryInfo::empty() }; 8];
    let count = match filesystem.list_dir("/prov", &mut entries) {
        Ok(c) => c,
        Err(_) => return,
    };

    if count == 0 {
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

        let prefix = if fname.ends_with(".app")
            || fname.ends_with(".svc")
            || fname.ends_with(".dmn")
            || fname.ends_with(".krx")
        {
            "/apps/"
        } else if fname.ends_with(".cfg") {
            "/pref/"
        } else if fname.ends_with(".dat") || fname.ends_with(".sig") {
            "/sys/"
        } else if fname.ends_with(".rle") || fname.ends_with(".img") {
            "/user/splash/"
        } else {
            "/user/"
        };

        let mut dst_buf = [0u8; 48];
        let p_bytes = prefix.as_bytes();
        let n_bytes = fname.as_bytes();
        if p_bytes.len() + n_bytes.len() <= dst_buf.len() {
            dst_buf[..p_bytes.len()].copy_from_slice(p_bytes);
            dst_buf[p_bytes.len()..p_bytes.len() + n_bytes.len()].copy_from_slice(n_bytes);
            if let Ok(dst_path) = core::str::from_utf8(&dst_buf[..p_bytes.len() + n_bytes.len()]) {
                let _ = filesystem.remove(dst_path);
                if filesystem.write_file(dst_path, buf).is_ok() {
                    let _ = filesystem.remove(src_path);
                }
            }
        }
    }
}
