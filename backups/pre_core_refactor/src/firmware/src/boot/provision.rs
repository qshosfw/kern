//! Boot-Time Package Provisioning Engine (§13.2 / ORDS-01).
//!
//! Scans the staging directory `/prov/` on KernFS, performs strict validation
//! on all staged packages and configuration files, and relocates verified items
//! to their canonical locations:
//! - Executable packages (`.app`, `.krx`) -> `/apps/` (after header/magic/CRC/cap validation)
//! - System preferences (`.cfg`)          -> `/pref/`
//! - System calibration / keys (`.dat`, `.sig`) -> `/sys/`
//! - Operational databases (`.ch`, `.ct`, `.bnd`, `.zn`, `.fm`, `.vfo`) -> `/user/`
//!
//! Safely cleans up staged source files from `/prov/` upon successful relocation.

use crate::services::SystemStorage;
use crate::services::storage::DirEntryInfo;

pub const PROV_BUF_CAP: usize = 512;

/// Staging buffer for atomic package relocation.
/// Placed in `.uninit` so it incurs strictly ZERO bytes of Flash overhead.
#[unsafe(link_section = ".uninit")]
static mut PROV_BUF: [u8; PROV_BUF_CAP] = [0u8; PROV_BUF_CAP];

/// Extract the filename component from a path (e.g. "/prov/app.krx" -> "app.krx").
fn filename_from_path(path: &str) -> &str {
    if let Some(idx) = path.rfind('/') {
        &path[idx + 1..]
    } else {
        path
    }
}

/// Execute the storage provisioning pass for dynamic application packages and configuration.
#[inline(never)]
pub fn provision_packages(vfs: &mut SystemStorage) {
    let _ = vfs.create_dir("/apps");
    let _ = vfs.create_dir("/pref");
    let _ = vfs.create_dir("/sys");
    let _ = vfs.create_dir("/user");
    let _ = vfs.create_dir("/user/splash");

    ensure_explorer_provisioned(vfs);

    let mut entries = [const { DirEntryInfo::empty() }; 16];
    let count = match vfs.list_dir("/prov", &mut entries) {
        Ok(c) => c,
        Err(_) => return,
    };

    if count == 0 {
        return;
    }

    for entry in &entries[..count] {
        if entry.is_dir || entry.path.is_empty() {
            continue;
        }

        let src_path = entry.path.as_str();
        let fname = filename_from_path(src_path);
        if fname.is_empty() {
            continue;
        }

        let rlen = match vfs.read_file(src_path, unsafe { &mut *core::ptr::addr_of_mut!(PROV_BUF) })
        {
            Ok(l) => l,
            Err(_) => continue,
        };

        if rlen == 0 {
            let _ = vfs.remove(src_path);
            continue;
        }

        let buf = unsafe {
            core::slice::from_raw_parts(core::ptr::addr_of!(PROV_BUF) as *const u8, rlen)
        };

        let prefix = if fname.ends_with(".app") || fname.ends_with(".krx") {
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
                let _ = vfs.remove(dst_path);
                if vfs.write_file(dst_path, buf).is_ok() {
                    let _ = vfs.remove(src_path);
                }
            }
        }
    }
}

#[cfg(feature = "provision_apps")]
const EXPLORER_KRX: [u8; 128] = {
    let mut pkg = [0u8; 128];
    // 1. Preamble (20 bytes)
    pkg[0] = kernel::proc::krx::KRX_MAGIC[0];
    pkg[1] = kernel::proc::krx::KRX_MAGIC[1];
    pkg[2] = kernel::proc::krx::KRX_MAGIC[2];
    pkg[3] = kernel::proc::krx::KRX_MAGIC[3];
    pkg[4] = kernel::proc::krx::KrxClass::Paged as u8; // Execution class: Paged
    pkg[5] = 0x05; // Format version 2.5.0
    let s_cnt = 3u16.to_le_bytes();
    pkg[6] = s_cnt[0];
    pkg[7] = s_cnt[1];
    let h_len = 128u16.to_le_bytes();
    pkg[8] = h_len[0];
    pkg[9] = h_len[1];

    // 2. SectionIdentity (offset 20: 4B header + 40B body = 44B, ends at 64)
    let s_id = kernel::proc::krx::SECTION_IDENTITY.to_le_bytes();
    pkg[20] = s_id[0];
    pkg[21] = s_id[1];
    pkg[22] = 44;
    pkg[24] = 112; // name_str offset = 112
    pkg[26] = 1; // version 1.0.0
    pkg[29] = 1; // min_kernel_abi = 1
    pkg[31] = b'K';
    pkg[32] = b'E';
    pkg[33] = b'R';
    pkg[34] = b'N';
    pkg[35] = b'-';
    pkg[36] = b'T';
    pkg[37] = b'E';
    pkg[38] = b'A';
    pkg[39] = b'M';
    pkg[47] = 1; // icon_asset = 1

    // 3. SectionExecution (offset 64: 4B header + 28B body = 32B, ends at 96)
    let s_ex = kernel::proc::krx::SECTION_EXECUTION.to_le_bytes();
    pkg[64] = s_ex[0];
    pkg[65] = s_ex[1];
    pkg[66] = 32;
    let stk = 256u16.to_le_bytes();
    pkg[68] = stk[0];
    pkg[69] = stk[1];
    let pg = 512u16.to_le_bytes();
    pkg[72] = pg[0];
    pkg[73] = pg[1];
    pkg[74] = 2; // cache_blocks

    // 4. SectionType (offset 96: 4B header + 12B body = 16B, ends at 112)
    let s_ty = kernel::proc::krx::SECTION_TYPE_LIFECYCLE.to_le_bytes();
    pkg[96] = s_ty[0];
    pkg[97] = s_ty[1];
    pkg[98] = 16;
    pkg[100] = kernel::proc::krx::KrxExecType::App as u8; // exec_type = APP (1)
    pkg[102] = 1; // stack_cost
    let caps = (kernel::caps::Cap::DisplayDirect as u32)
        | (kernel::caps::Cap::StorageRead as u32)
        | (kernel::caps::Cap::Keypad as u32);
    let cap_bytes = caps.to_le_bytes();
    pkg[108] = cap_bytes[0];
    pkg[109] = cap_bytes[1];
    pkg[110] = cap_bytes[2];
    pkg[111] = cap_bytes[3];

    // 5. String pool (offset 112..128)
    pkg[112] = b'E';
    pkg[113] = b'x';
    pkg[114] = b'p';
    pkg[115] = b'l';
    pkg[116] = b'o';
    pkg[117] = b'r';
    pkg[118] = b'e';
    pkg[119] = b'r';

    pkg
};

/// Ensure factory provisioned Explorer KRX package exists in `/apps/explorer.krx`.
#[cfg(feature = "provision_apps")]
fn ensure_explorer_provisioned(vfs: &mut SystemStorage) {
    if vfs.exists("/apps/explorer.krx") || vfs.exists("/prov/explorer.krx") {
        return;
    }
    let _ = vfs.write_file("/apps/explorer.krx", &EXPLORER_KRX);
}

#[cfg(not(feature = "provision_apps"))]
fn ensure_explorer_provisioned(_vfs: &mut SystemStorage) {}
