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
    KRX_IDENT, KrxClass, KrxExecType, KrxProfile, SECTION_EXECUTION, SECTION_IDENTITY,
    SECTION_TYPE_LIFECYCLE,
};

pub const PROV_BUF_CAP: usize = 2048;

// Declare 3.0 payload for `/apps/sysinfo.krx` (1 node, 5 live ITEM_INFO rows, 0 actions, 6 labels)
#[cfg(all(feature = "provision_apps", feature = "krx_declare"))]
const SYSINFO_PAYLOAD: &[u8] = &[
    1, 5, 0, 6, // [node_count=1, item_count=5, action_count=0, label_count=6]
    // DeclareNode 0 (8B): node_id=0, layout=0 (Menu), item_count=5, title_offset=0, first_item=0
    0, 0, 0, 5, 0, 0, 0, 0,
    // Item 0 (12B): kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=1, target=0, value_ref=0x0100 (DYN_BATTERY_MV)
    5, 1, 1, 0, 0, 0, 0x00, 0x01, 0, 0, 0, 0,
    // Item 1 (12B): kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=2, target=0, value_ref=0x0102 (DYN_CPU_CLOCK)
    5, 1, 2, 0, 0, 0, 0x02, 0x01, 0, 0, 0, 0,
    // Item 2 (12B): kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=3, target=0, value_ref=0x0105 (DYN_STACK_MARGIN)
    5, 1, 3, 0, 0, 0, 0x05, 0x01, 0, 0, 0, 0,
    // Item 3 (12B): kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=4, target=0, value_ref=0x0107 (DYN_UPTIME)
    5, 1, 4, 0, 0, 0, 0x07, 0x01, 0, 0, 0, 0,
    // Item 4 (12B): kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=5, target=0, value_ref=0x0108 (DYN_ACTIVE_PROCS)
    5, 1, 5, 0, 0, 0, 0x08, 0x01, 0, 0, 0, 0, // Length-prefixed string pool (6 strings)
    4, b'L', b'i', b'v', b'e', 7, b'B', b'a', b't', b't', b'e', b'r', b'y', 9, b'C', b'P', b'U',
    b' ', b'C', b'l', b'o', b'c', b'k', 10, b'S', b't', b'a', b'c', b'k', b' ', b'F', b'r', b'e',
    b'e', 10, b'U', b'p', b't', b'i', b'm', b'e', b' ', b'(', b's', b')', 12, b'A', b'c', b't',
    b'i', b'v', b'e', b' ', b'T', b'a', b's', b'k', b's',
];

// Declare 3.0 payload for `/apps/radioinfo.krx` (1 node, 4 live telemetry rows, layout=3 InfoCard, 5 labels)
#[cfg(all(feature = "provision_apps", feature = "krx_declare"))]
const RADIOINFO_PAYLOAD: &[u8] = &[
    1, 4, 0, 5, // [node_count=1, item_count=4, action_count=0, label_count=5]
    // DeclareNode 0 (8B): node_id=0, layout=3 (InfoCard), item_count=4, title_offset=0, first_item=0
    0, 0, 3, 4, 0, 0, 0, 0,
    // Item 0: kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=1, target=0, value_ref=0x010B (DYN_RADIO_FREQ_A)
    5, 1, 1, 0, 0, 0, 0x0B, 0x01, 0, 0, 0, 0,
    // Item 1: kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=2, target=0, value_ref=0x010C (DYN_RADIO_FREQ_B)
    5, 1, 2, 0, 0, 0, 0x0C, 0x01, 0, 0, 0, 0,
    // Item 2: kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=3, target=0, value_ref=0x010A (DYN_RADIO_RSSI)
    5, 1, 3, 0, 0, 0, 0x0A, 0x01, 0, 0, 0, 0,
    // Item 3: kind=5 (ITEM_INFO), flags=1 (FLAG_DYNAMIC), label=4, target=0, value_ref=0x0101 (DYN_BATTERY_PCT)
    5, 1, 4, 0, 0, 0, 0x01, 0x01, 0, 0, 0, 0,
    // String pool: "Radio Info", "VFO A", "VFO B", "RSSI", "Battery"
    10, b'R', b'a', b'd', b'i', b'o', b' ', b'I', b'n', b'f', b'o', 5, b'V', b'F', b'O', b' ', b'A',
    5, b'V', b'F', b'O', b' ', b'B', 4, b'R', b'S', b'S', b'I', 7, b'B', b'a', b't', b't', b'e',
    b'r', b'y',
];

// Declare 3.0 payload for `/apps/counter.krx` (1 node, 4 items, 3 actions, 5 labels)
#[cfg(all(feature = "provision_apps", feature = "krx_declare"))]
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
#[cfg(all(feature = "provision_apps", feature = "krx_declare"))]
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

// Native ARM Thumb-1 binary payload for `/apps/beacon.krx`
// Implements standard KrxNativeEntryFn:
//   r0 = op (0=START, 1=EVENT, 2=RENDER, 3=EXIT)
//   r1 = api_table (*const KrxApiTable)
//   r2 = param (*const ())
#[cfg(all(feature = "provision_apps", feature = "krx_native"))]
const BEACON_NATIVE_PAYLOAD: &[u8] = &[
    0x70, 0xb5, // push {r4, r5, r6, lr}
    0x04, 0x46, // mov r4, r0
    0x0d, 0x46, // mov r5, r1
    0x16, 0x46, // mov r6, r2
    0x00, 0x2c, // cmp r4, #0
    0x07, 0xd0, // beq .Lstart
    0x01, 0x2c, // cmp r4, #1
    0x0e, 0xd0, // beq .Levent
    0x02, 0x2c, // cmp r4, #2
    0x18, 0xd0, // beq .Lrender
    0x03, 0x2c, // cmp r4, #3
    0x3c, 0xd0, // beq .Lexit
    0x00, 0x20, // movs r0, #0
    0x70, 0xbd, // pop {r4, r5, r6, pc}
    // .Lstart:
    0x6b, 0x6e, // ldr r3, [r5, #100] (rf_tune)
    0x00, 0x2b, // cmp r3, #0
    0x03, 0xd0, // beq .Lstart_done
    0x21, 0x48, // ldr r0, [pc, #132] (145.500 MHz)
    0x19, 0x21, // movs r1, #25 (bandwidth)
    0x21, 0x4a, // ldr r2, [pc, #132] (12.5 kHz step)
    0x98, 0x47, // blx r3
    // .Lstart_done:
    0x00, 0x20, // movs r0, #0
    0x70, 0xbd, // pop {r4, r5, r6, pc}
    // .Levent:
    0x00, 0x2e, // cmp r6, #0
    0x07, 0xd0, // beq .Levent_ignored
    0x31, 0x78, // ldrb r1, [r6, #0] (krx_ev.kind)
    0x01, 0x29, // cmp r1, #1 (KRX_EV_KEY)
    0x04, 0xd1, // bne .Levent_ignored
    0x70, 0x78, // ldrb r0, [r6, #1] (krx_ev.key)
    0x06, 0x28, // cmp r0, #6 (KEY_EXIT)
    0x01, 0xd0, // beq .Levent_ignored
    0x01, 0x20, // movs r0, #1 (consumed)
    0x70, 0xbd, // pop {r4, r5, r6, pc}
    // .Levent_ignored:
    0x00, 0x20, // movs r0, #0
    0x70, 0xbd, // pop {r4, r5, r6, pc}
    // .Lrender: Clean edge-to-edge rendering without useless borders
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0xc0, 0x46, // nop
    0x2c, 0x6d, // ldr r4, [r5, #80] (draw_str)
    0x00, 0x2c, // cmp r4, #0
    0x0a, 0xd0, // beq .Lrender_done
    0x00, 0x20, // movs r0, #0 (font = 0)
    0x01, 0x21, // movs r1, #1 (color = 1)
    0x82, 0xb0, // sub sp, #8
    0x00, 0x90, // str r0, [sp, #0]
    0x01, 0x91, // str r1, [sp, #4]
    0x10, 0x20, // movs r0, #16 (x = 16)
    0x1a, 0x21, // movs r1, #26 (y = 26)
    0x04, 0xa2, // add r2, pc, #16 (adr r2, .Lstr_beacon)
    0x0e, 0x23, // movs r3, #14 (len = 14)
    0xa0, 0x47, // blx r4
    0x02, 0xb0, // add sp, #8
    // .Lrender_done:
    0x00, 0x20, // movs r0, #0
    0x70, 0xbd, // pop {r4, r5, r6, pc}
    // .Lexit:
    0x00, 0x20, // movs r0, #0
    0x70, 0xbd, // pop {r4, r5, r6, pc}
    0xc0, 0x46, // nop
    // "BEACON 145.500"
    b'B', b'E', b'A', b'C', b'O', b'N', b' ', b'1', b'4', b'5', b'.', b'5', b'0', b'0', 0xc0,
    0x46, // nop
    0x60, 0x27, 0xac, 0x08, // .word 145500000 (0x08ac2760)
    0xd4, 0x30, 0x00, 0x00, // .word 12500 (0x000030d4)
];

// Standalone Native Thumb-1 binary payload for `/apps/bench.krx`.
// 100% self-contained PIC executable. Completely resides inside the KRX file on KernFS.
#[cfg(all(feature = "provision_apps", feature = "krx_native"))]
const BENCH_NATIVE_PAYLOAD: &[u8] = include_bytes!("../../../../packages/bench/bench.bin");

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

/// Build and write a KRX package file into KernFS using the shared transient arena buffer.
#[cfg(feature = "provision_apps")]
#[inline(never)]
fn write_krx_package(
    filesystem: &mut SystemStorage,
    path: &str,
    class: KrxClass,
    profile: KrxProfile,
    name: &str,
    caps: u32,
    payload: &[u8],
) {
    let total_len = 128 + payload.len();
    if total_len > PROV_BUF_CAP {
        return;
    }

    if filesystem.exists(path) {
        let buf = &mut crate::runtime::transient_arena_bytes_mut()[..total_len];
        if let Ok(rlen) = filesystem.read_file(path, buf) {
            let page_size: u16 = match class {
                KrxClass::Paged => 512,
                _ => 0,
            };
            if rlen == total_len
                && &buf[128..total_len] == payload
                && buf[94] == profile.raw_code()
                && buf[68..70] == 128u16.to_le_bytes()
                && buf[72..74] == page_size.to_le_bytes()
            {
                return;
            }
        }
        let _ = filesystem.remove(path);
    }

    let pkg = &mut crate::runtime::transient_arena_bytes_mut()[..total_len];
    pkg.fill(0);

    // 1. Preamble (20 bytes)
    pkg[0..4].copy_from_slice(&KRX_IDENT);
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
    pkg[68..70].copy_from_slice(&128u16.to_le_bytes()); // min_stack_bytes = 128
    let page_size: u16 = match class {
        KrxClass::Paged => 512,
        _ => 0,
    };
    pkg[72..74].copy_from_slice(&page_size.to_le_bytes()); // page_size: 0 for Native
    pkg[74] = if class == KrxClass::Paged { 2 } else { 0 }; // cache_blocks
    pkg[80..84].copy_from_slice(&(payload.len() as u32).to_le_bytes()); // payload_size
    let code_len = payload.len() as u16;
    let got_offset = match class {
        KrxClass::Native | KrxClass::Paged => (code_len + 3) & !3,
        _ => 0,
    };
    pkg[88..90].copy_from_slice(&got_offset.to_le_bytes()); // got_offset
    pkg[94] = profile.raw_code(); // explicit profile

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

/// Write a pre-built valid KRX package directly to KernFS storage with idempotency.
#[cfg(feature = "provision_apps")]
#[inline(never)]
fn write_raw_krx_package(filesystem: &mut SystemStorage, path: &str, pkg_bytes: &[u8]) {
    if filesystem.exists(path) {
        if let Ok(size) = filesystem.file_size(path) {
            if size == pkg_bytes.len() {
                let mut head = [0u8; 64];
                let hlen = head.len().min(pkg_bytes.len());
                if let Ok(rlen) = filesystem.read_file_at(path, 0, &mut head[..hlen]) {
                    if rlen == hlen && head[..hlen] == pkg_bytes[..hlen] {
                        return;
                    }
                }
            }
        }
        let _ = filesystem.remove(path);
    }
    let _ = filesystem.write_file(path, pkg_bytes);
}

/// Ensure core flash-loaded `.krx` packages exist in `/apps/`.
#[cfg(not(feature = "provision_apps"))]
pub fn ensure_core_packages(_filesystem: &mut SystemStorage) {}

/// Ensure core flash-loaded `.krx` packages exist in `/apps/`.
#[cfg(feature = "provision_apps")]
#[inline(never)]
pub fn ensure_core_packages(filesystem: &mut SystemStorage) {
    let base_caps = (kernel::caps::Cap::DisplayDirect as u32) | (kernel::caps::Cap::Keypad as u32);

    #[cfg(feature = "krx_native")]
    {
        let radio_caps = base_caps | (kernel::caps::Cap::RadioRx as u32);
        write_krx_package(
            filesystem,
            "/apps/beacon.krx",
            KrxClass::Native,
            KrxProfile::N1NativePic,
            "Beacon",
            radio_caps,
            BEACON_NATIVE_PAYLOAD,
        );

        let all_caps = base_caps
            | (kernel::caps::Cap::RadioRx as u32)
            | (kernel::caps::Cap::RadioTx as u32)
            | (kernel::caps::Cap::StorageRead as u32)
            | (kernel::caps::Cap::StorageWrite as u32)
            | (kernel::caps::Cap::AudioOutput as u32)
            | (kernel::caps::Cap::AudioCapture as u32)
            | (kernel::caps::Cap::SerialComm as u32)
            | (kernel::caps::Cap::ServiceRegistry as u32)
            | (kernel::caps::Cap::PowerManage as u32)
            | (kernel::caps::Cap::RealTimeClock as u32);

        write_krx_package(
            filesystem,
            "/apps/bench.krx",
            KrxClass::Native,
            KrxProfile::N1NativePic,
            "Bench",
            all_caps,
            BENCH_NATIVE_PAYLOAD,
        );

        write_raw_krx_package(
            filesystem,
            "/apps/leaf.krx",
            LEAF_APP_PACKAGE,
        );

        write_raw_krx_package(
            filesystem,
            "/apps/paged.krx",
            PAGED_APP_PACKAGE,
        );
    }

    #[cfg(feature = "krx_declare")]
    {
        write_krx_package(
            filesystem,
            "/apps/sysinfo.krx",
            KrxClass::Declare,
            KrxProfile::D0DeclareGraph,
            "SysInfo",
            base_caps,
            SYSINFO_PAYLOAD,
        );
        write_krx_package(
            filesystem,
            "/apps/counter.krx",
            KrxClass::Declare,
            KrxProfile::D0DeclareGraph,
            "Counter",
            base_caps,
            COUNTER_PAYLOAD,
        );
        write_krx_package(
            filesystem,
            "/apps/timer.krx",
            KrxClass::Declare,
            KrxProfile::D0DeclareGraph,
            "Timer",
            base_caps,
            TIMER_PAYLOAD,
        );
        write_krx_package(
            filesystem,
            "/apps/radioinfo.krx",
            KrxClass::Declare,
            KrxProfile::D0DeclareGraph,
            "RadioInfo",
            base_caps,
            RADIOINFO_PAYLOAD,
        );
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
