//! Storage & Filesystem Subsystem System Call Handlers (`StorageApi`, `interface_id = 0x0004`).

use super::{active_ctx_opt, parse_str, parse_user_path};
use kernel::proc::abi::{KrxDirEntry, StorageApi};
use kernel::proc::primitives::*;

#[inline(never)]
fn active_storage_mut() -> Option<&'static mut crate::services::SystemStorage<'static>> {
    if let Some(ctx) = active_ctx_opt() {
        Some(unsafe { &mut *(ctx.storage as *mut crate::services::SystemStorage<'_> as *mut _) })
    } else {
        let ptr = unsafe { super::super::native_runner::BOOT_STORAGE };
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *(ptr as *mut crate::services::SystemStorage<'static>) })
        }
    }
}

#[inline(always)]
fn is_proc_boot_path(path: &str) -> bool {
    path == "/proc/boot"
}

#[inline(always)]
fn has_proc_read_cap() -> bool {
    super::super::native_runner::active_native_caps().contains(kernel::caps::Cap::ServiceRegistry)
}

fn copy_proc_seg(
    dst: &mut [u8],
    offset: usize,
    written: &mut usize,
    seg_start: &mut usize,
    src: &[u8],
) {
    let seg_end = *seg_start + src.len();
    let cur = offset + *written;
    if cur >= *seg_start && cur < seg_end && *written < dst.len() {
        let s_off = cur - *seg_start;
        let n = (dst.len() - *written).min(src.len() - s_off);
        dst[*written..*written + n].copy_from_slice(&src[s_off..s_off + n]);
        *written += n;
    }
    *seg_start = seg_end;
}

#[inline(never)]
fn proc_read(path: &str, offset: usize, dst: &mut [u8]) -> Option<usize> {
    if !is_proc_boot_path(path) || !has_proc_read_cap() {
        return None;
    }
    let tag_bytes = crate::boot::read_backup_tag().encode().to_le_bytes();
    let segs: [(*const u8, usize); 3] = [
        (
            crate::panic::BOOT_RECORD.get() as *const kernel::boot::BootRecord as *const u8,
            260,
        ),
        (core::ptr::addr_of!(crate::FAULT_EXTRA) as *const u8, 52),
        (tag_bytes.as_ptr(), 4),
    ];
    let mut written = 0usize;
    let mut seg_start = 0usize;
    for &(ptr, len) in &segs {
        let src = unsafe { core::slice::from_raw_parts(ptr, len) };
        copy_proc_seg(dst, offset, &mut written, &mut seg_start, src);
    }
    Some(written)
}

#[inline(never)]
fn proc_remove(path: &str) -> Option<u8> {
    if !is_proc_boot_path(path) {
        return None;
    }
    let caps = super::super::native_runner::active_native_caps();
    if !caps.contains(kernel::caps::Cap::PowerManage) {
        return Some(1);
    }
    crate::boot::clear_crash_strikes();
    if caps.contains(kernel::caps::Cap::StorageAdmin) {
        crate::panic::BOOT_RECORD.get_mut().reset_all();
    }
    Some(0)
}

#[inline(never)]
fn storage_config_op(buf: *const u8, len: u16, is_save: bool) -> u16 {
    if let (Some(src), Some(ctx)) = (
        unsafe { super::ptr_slice(buf, len as usize) },
        active_ctx_opt(),
    ) {
        let app_name = super::super::native_runner::active_native_app_name();
        if is_save {
            return u16::from(ctx.save_config(app_name, src).is_err());
        } else {
            let dst = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, src.len()) };
            return ctx.load_config(app_name, dst).unwrap_or(0) as u16;
        }
    }
    u16::from(is_save)
}

pub unsafe extern "C" fn api_storage_load_config(buf: *mut u8, max_len: u16) -> u16 {
    storage_config_op(buf, max_len, false)
}

pub unsafe extern "C" fn api_storage_save_config(buf: *const u8, len: u16) -> u8 {
    storage_config_op(buf, len, true) as u8
}

pub unsafe extern "C" fn api_storage_read_asset(
    asset_id: u16,
    offset: u32,
    buf: *mut u8,
    len: u16,
) -> u16 {
    if let (Some(slice), Some(ctx)) = (
        unsafe { super::ptr_slice_mut(buf, len as usize) },
        active_ctx_opt(),
    ) {
        ctx.read_asset(asset_id, offset, slice).unwrap_or(0) as u16
    } else {
        0
    }
}

pub unsafe extern "C" fn api_storage_read_file(
    path: *const u8,
    path_len: u8,
    buf: *mut u8,
    max_len: u16,
) -> u16 {
    unsafe { api_storage_read_file_at(path, path_len, 0, buf, max_len) }
}

#[inline(never)]
fn storage_write_op(path: *const u8, path_len_and_append: u16, data: *const u8, len: u16) -> u8 {
    let append = (path_len_and_append & 0x8000) != 0;
    let path_len = (path_len_and_append & 0xFF) as usize;
    if !data.is_null() {
        if let (Some(path_str), Some(st)) = (
            unsafe { parse_user_path(path, path_len) },
            active_storage_mut(),
        ) {
            if utils::path::root_bit(path_str.as_bytes()) == utils::path::ROOT_SYS {
                return 1;
            }
            let src = unsafe { core::slice::from_raw_parts(data, len as usize) };
            let res = if append {
                let buf = crate::data::arena::staging_buf();
                let cur_len = match st.read_file(path_str, buf) {
                    Ok(n) => n,
                    Err(kernel::service::StorageError::NotFound) => 0,
                    Err(_) => return 1,
                };
                if cur_len + src.len() > buf.len() {
                    return 1;
                }
                buf[cur_len..cur_len + src.len()].copy_from_slice(src);
                crate::security::write_classified_file(st, path_str, &buf[..cur_len + src.len()])
            } else {
                crate::security::write_classified_file(st, path_str, src)
            };
            return u8::from(res.is_err());
        }
    }
    1
}

#[inline(never)]
fn user_path_unit_op(path: *const u8, path_len: u8, create_dir: bool) -> u8 {
    if !create_dir {
        if let Some(raw) = unsafe { parse_str(path, path_len as usize) } {
            if let Some(rc) = proc_remove(raw) {
                return rc;
            }
        }
    }
    if let (Some(p_str), Some(st)) = (
        unsafe { parse_user_path(path, path_len as usize) },
        active_storage_mut(),
    ) {
        if utils::path::root_bit(p_str.as_bytes()) == utils::path::ROOT_SYS {
            return 1;
        }
        let res = if create_dir {
            st.create_dir(p_str)
        } else {
            st.remove(p_str)
        };
        return u8::from(res.is_err());
    }
    1
}

pub unsafe extern "C" fn api_storage_write_file(
    path: *const u8,
    path_len: u8,
    data: *const u8,
    len: u16,
) -> u8 {
    storage_write_op(path, path_len as u16, data, len)
}

pub unsafe extern "C" fn api_storage_remove_file(path: *const u8, path_len: u8) -> u8 {
    user_path_unit_op(path, path_len, false)
}

#[inline(never)]
fn file_stat_size(path: *const u8, path_len: u8) -> Option<u32> {
    let path_str = unsafe { parse_str(path, path_len as usize) }?;
    if is_proc_boot_path(path_str) && has_proc_read_cap() {
        return Some(316);
    }
    active_storage_mut()?
        .file_size(path_str)
        .ok()
        .map(|s| s as u32)
}

pub unsafe extern "C" fn api_storage_file_size(path: *const u8, path_len: u8) -> u32 {
    file_stat_size(path, path_len).unwrap_or(0)
}

pub unsafe extern "C" fn api_storage_list_dir(
    dir_path: *const u8,
    dir_len: u8,
    out_entries: *mut KrxDirEntry,
    max_entries: u8,
) -> u8 {
    if !out_entries.is_null() && max_entries > 0 {
        let dir_str = unsafe { super::str_or_default(dir_path, dir_len as usize, "/") };
        let dst = unsafe { core::slice::from_raw_parts_mut(out_entries, max_entries as usize) };
        if let Some(ctx) = active_ctx_opt() {
            return ctx.list_dir(dir_str, dst).unwrap_or(0) as u8;
        }
    }
    0
}

pub unsafe extern "C" fn api_storage_file_exists(path: *const u8, path_len: u8) -> u8 {
    u8::from(file_stat_size(path, path_len).is_some())
}

pub unsafe extern "C" fn api_storage_read_file_at(
    path: *const u8,
    path_len: u8,
    offset: u32,
    buf: *mut u8,
    max_len: u16,
) -> u16 {
    if let (Some(path_str), Some(dst)) = (unsafe { parse_str(path, path_len as usize) }, unsafe {
        super::ptr_slice_mut(buf, max_len as usize)
    }) {
        if let Some(n) = proc_read(path_str, offset as usize, dst) {
            return n as u16;
        }
        if let Some(st) = active_storage_mut() {
            return st.read_file_at(path_str, offset as usize, dst).unwrap_or(0) as u16;
        }
    }
    0
}

pub unsafe extern "C" fn api_storage_append_file(
    path: *const u8,
    path_len: u8,
    data: *const u8,
    len: u16,
) -> u8 {
    storage_write_op(path, (path_len as u16) | 0x8000, data, len)
}

pub unsafe extern "C" fn api_storage_rename_file(
    old_path: *const u8,
    old_len: u8,
    new_path: *const u8,
    new_len: u8,
) -> u8 {
    if let (Some(o_str), Some(n_str)) = (
        unsafe { parse_user_path(old_path, old_len as usize) },
        unsafe { parse_user_path(new_path, new_len as usize) },
    ) {
        if let Some(ctx) = active_ctx_opt() {
            if ctx.rename_file(o_str, n_str).is_ok() {
                return 0;
            }
        }
    }
    1
}

pub unsafe extern "C" fn api_storage_create_dir(path: *const u8, path_len: u8) -> u8 {
    user_path_unit_op(path, path_len, true)
}

pub unsafe extern "C" fn api_storage_format(mode: u8) -> u8 {
    let caps = super::super::native_runner::active_native_caps();
    if !caps.contains(kernel::caps::Cap::StorageAdmin) {
        return 1;
    }
    if mode > 0 && !caps.contains(kernel::caps::Cap::FactoryCalib) {
        return 1;
    }
    #[cfg(feature = "filesystem")]
    if let Some(ctx) = active_ctx_opt() {
        let res = match mode {
            0 | 1 => crate::services::calibration::reformat_storage(
                ctx.storage,
                ctx.calibration.raw_bytes(),
                mode == 0,
            ),
            2 => ctx.storage.fs.wipe_raw().map_err(|_| ()),
            _ => Err(()),
        };
        return u8::from(res.is_err());
    }
    1
}

pub static STORAGE_VTABLE: StorageApi = StorageApi {
    storage_load_config: api_storage_load_config,
    storage_save_config: api_storage_save_config,
    storage_read_asset: api_storage_read_asset,
    storage_read_file: api_storage_read_file,
    storage_write_file: api_storage_write_file,
    storage_remove_file: api_storage_remove_file,
    storage_file_size: api_storage_file_size,
    storage_list_dir: api_storage_list_dir,
    storage_file_exists: api_storage_file_exists,
    storage_read_file_at: api_storage_read_file_at,
    storage_append_file: api_storage_append_file,
    storage_rename_file: api_storage_rename_file,
    storage_create_dir: api_storage_create_dir,
    storage_format: api_storage_format,
};

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::caps::{Cap, CapabilitySet};

    #[test]
    fn proc_boot_snapshot_cap_and_reset_semantics() {
        super::super::super::native_runner::init_native_slot();
        {
            let slot = super::super::super::native_runner::slot_mut();
            slot.is_loaded = true;
            slot.caps = CapabilitySet::empty();
        }
        let mut buf = [0u8; 316];
        // Without ServiceRegistry, /proc/boot read is denied
        assert!(proc_read("/proc/boot", 0, &mut buf).is_none());
        // Without PowerManage, /proc/boot remove returns error (1)
        assert_eq!(proc_remove("/proc/boot"), Some(1));

        // Populate BackupCrashTag, FaultExtraRegs, and a BootSlot entry
        let initial_tag = kernel::boot::BackupCrashTag {
            strikes: 3,
            flags: kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH
                | kernel::boot::BackupCrashTag::FLAG_SAFE_MODE_REQ,
            reason_code: 1,
        };
        crate::boot::write_backup_tag(initial_tag);
        unsafe {
            crate::FAULT_EXTRA.valid = kernel::boot::FaultExtraRegs::FAULT_EXTRA_IDENT;
            crate::FAULT_EXTRA.msp = 0x2000_3A00;
            let rec = crate::panic::BOOT_RECORD.get_mut();
            rec.slot_a.ident = kernel::boot::BOOT_RECORD_IDENT;
            rec.slot_a.fault_pc = 0x0800_1235;
            rec.slot_a.crash_count = 2;
            rec.slot_a.crc32 = rec.slot_a.compute_crc();
        }

        // Grant ServiceRegistry: full 316-byte read succeeds
        super::super::super::native_runner::slot_mut().caps =
            CapabilitySet::empty().with(Cap::ServiceRegistry);
        assert_eq!(proc_read("/proc/boot", 0, &mut buf), Some(316));
        assert_eq!(&buf[0..4], &unsafe {
            *(crate::panic::BOOT_RECORD.get() as *const kernel::boot::BootRecord as *const [u8; 4])
        });
        assert_eq!(
            u32::from_le_bytes(buf[292..296].try_into().unwrap()),
            0x2000_3A00
        );
        assert_eq!(
            u32::from_le_bytes(buf[308..312].try_into().unwrap()),
            kernel::boot::FaultExtraRegs::FAULT_EXTRA_IDENT
        );
        assert_eq!(
            u32::from_le_bytes(buf[312..316].try_into().unwrap()),
            initial_tag.encode()
        );

        // Chunked offset read at offset 260 yields 56 bytes (52B extra + 4B tag)
        let mut tail = [0u8; 64];
        assert_eq!(proc_read("/proc/boot", 260, &mut tail), Some(56));
        assert_eq!(&tail[..56], &buf[260..316]);

        // Grant PowerManage + StorageAdmin and remove /proc/boot -> clears strikes & resets BOOT_RECORD
        super::super::super::native_runner::slot_mut().caps = CapabilitySet::empty()
            .with(Cap::PowerManage)
            .with(Cap::StorageAdmin);
        assert_eq!(proc_remove("/proc/boot"), Some(0));
        assert_eq!(crate::boot::read_backup_tag().strikes, 0);
        unsafe {
            assert_eq!(crate::panic::BOOT_RECORD.get().slot_a.crash_count, 0);
            assert_eq!(crate::panic::BOOT_RECORD.get().slot_a.fault_pc, 0);
            crate::FAULT_EXTRA.valid = 0;
        }
        crate::boot::write_backup_tag(kernel::boot::BackupCrashTag::empty());
        super::super::super::native_runner::init_native_slot();
    }
}
