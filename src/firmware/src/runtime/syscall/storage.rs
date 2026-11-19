//! Storage & Filesystem System Call Dispatcher (`Op::Fs*`, domain `0x3`).

#[cfg(test)]
use super::active_ctx_opt;
use super::{parse_str, parse_user_path, ptr_slice, ptr_slice_mut, str_or_default};
use kernel::proc::abi::{KrxDirEntry, KrxFsReq};

#[inline(always)]
fn active_storage_mut() -> Option<&'static mut crate::services::SystemStorage<'static>> {
    #[cfg(not(test))]
    {
        Some(crate::kern_fs())
    }
    #[cfg(test)]
    {
        active_ctx_opt().map(|ctx| unsafe {
            &mut *(ctx.storage as *mut crate::services::SystemStorage<'_> as *mut _)
        })
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

#[inline(never)]
fn proc_read(path: &str, offset: usize, dst: &mut [u8]) -> Option<usize> {
    if !is_proc_boot_path(path) || !has_proc_read_cap() {
        return None;
    }
    let tag_bytes = crate::boot::read_backup_tag().encode().to_le_bytes();
    let mut written = 0usize;
    let mut seg_start = 0usize;
    for (ptr, len) in [
        (
            crate::panic::BOOT_RECORD.get() as *const kernel::boot::BootRecord as *const u8,
            260usize,
        ),
        (core::ptr::addr_of!(crate::FAULT_EXTRA) as *const u8, 52),
        (tag_bytes.as_ptr(), 4),
    ] {
        let seg_end = seg_start + len;
        let cur = offset + written;
        if cur >= seg_start && cur < seg_end && written < dst.len() {
            let s_off = cur - seg_start;
            let n = (dst.len() - written).min(len - s_off);
            unsafe {
                core::ptr::copy_nonoverlapping(ptr.add(s_off), dst.as_mut_ptr().add(written), n);
            }
            written += n;
        }
        seg_start = seg_end;
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
fn user_storage<'a>(
    path: *const u8,
    path_len: u8,
) -> Option<(&'a str, &'a mut crate::services::SystemStorage<'static>)> {
    let p_str = unsafe { parse_user_path(path, path_len as usize) }?;
    if utils::path::root_bit(p_str.as_bytes()) == utils::path::ROOT_SYS {
        return None;
    }
    Some((p_str, active_storage_mut()?))
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

#[inline(never)]
pub unsafe fn dispatch_fs(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    match idx {
        // FsRead: a0 = mode (0=file_at, 1=load_cfg, 2=read_asset, 3=list_dir), a1 = *const KrxFsReq
        0 => {
            let req = a1 as *const KrxFsReq;
            if req.is_null() {
                return 0;
            }
            let r = unsafe { &*req };
            match a0 as u8 {
                0 => {
                    if let (Some(path_str), Some(dst)) =
                        (unsafe { parse_str(r.path, r.path_len as usize) }, unsafe {
                            ptr_slice_mut(r.buf, r.len as usize)
                        })
                    {
                        if let Some(n) = proc_read(path_str, r.offset as usize, dst) {
                            return n;
                        }
                        if let Some(st) = active_storage_mut() {
                            return st
                                .read_file_at(path_str, r.offset as usize, dst)
                                .unwrap_or(0);
                        }
                    }
                    0
                }
                1 => {
                    if let (Some(dst), Some(st)) = (
                        unsafe { ptr_slice_mut(r.buf, r.len as usize) },
                        active_storage_mut(),
                    ) {
                        let name = super::super::native_runner::active_native_app_name();
                        crate::services::primitives::load_app_config(st, name, dst).unwrap_or(0)
                    } else {
                        0
                    }
                }
                2 => {
                    if let (Some(slice), Some(st)) = (
                        unsafe { ptr_slice_mut(r.buf, r.len as usize) },
                        active_storage_mut(),
                    ) {
                        #[cfg(feature = "krx_native")]
                        {
                            return crate::runtime::native_runner::read_active_pkg_asset(
                                st, r.offset, slice,
                            )
                            .unwrap_or(0);
                        }
                        #[cfg(not(feature = "krx_native"))]
                        let _ = (st, slice);
                    }
                    0
                }
                3 => {
                    let out = r.buf as *mut KrxDirEntry;
                    let max = r.len as u8;
                    if !out.is_null() && max > 0 {
                        let dir_str = unsafe { str_or_default(r.path, r.path_len as usize, "/") };
                        let dst = unsafe { core::slice::from_raw_parts_mut(out, max as usize) };
                        if let Some(st) = active_storage_mut() {
                            return crate::services::primitives::list_krx_dir(st, dir_str, dst)
                                .unwrap_or(0);
                        }
                    }
                    0
                }
                _ => 0,
            }
        }
        // FsWrite: a0 = mode (0=write, 1=append, 2=save_cfg), a1 = *const KrxFsReq
        1 => {
            let req = a1 as *const KrxFsReq;
            if req.is_null() {
                return 1;
            }
            let r = unsafe { &*req };
            match a0 as u8 {
                0 | 1 => {
                    if !r.buf.is_null() {
                        if let Some((path_str, st)) = user_storage(r.path, r.path_len) {
                            let src = unsafe { core::slice::from_raw_parts(r.buf, r.len as usize) };
                            let ok = if (a0 as u8) == 1 {
                                st.fs.append_file(path_str, src).is_ok()
                            } else {
                                crate::security::write_classified_file(st, path_str, src).is_ok()
                            };
                            return usize::from(!ok);
                        }
                    }
                    1
                }
                2 => {
                    if let (Some(src), Some(st)) = (
                        unsafe { ptr_slice(r.buf, r.len as usize) },
                        active_storage_mut(),
                    ) {
                        let name = super::super::native_runner::active_native_app_name();
                        usize::from(
                            crate::services::primitives::save_app_config(st, name, src).is_err(),
                        )
                    } else {
                        1
                    }
                }
                _ => 1,
            }
        }
        // FsMeta: a0 = sub (0=size, 1=exists, 2=remove, 3=create_dir, 4=format)
        2 => {
            let path = a1 as *const u8;
            let plen = a2 as u8;
            match a0 as u8 {
                0 => file_stat_size(path, plen).unwrap_or(0) as usize,
                1 => usize::from(file_stat_size(path, plen).is_some()),
                2 | 3 => {
                    let create_dir = (a0 as u8) == 3;
                    if !create_dir {
                        if let Some(raw) = unsafe { parse_str(path, plen as usize) } {
                            if let Some(rc) = proc_remove(raw) {
                                return rc as usize;
                            }
                        }
                    }
                    if let Some((p_str, st)) = user_storage(path, plen) {
                        let res = if create_dir {
                            st.create_dir(p_str)
                        } else {
                            st.remove(p_str)
                        };
                        return usize::from(res.is_err());
                    }
                    1
                }
                4 => {
                    let mode = a1 as u8;
                    let caps = super::super::native_runner::active_native_caps();
                    if !caps.contains(kernel::caps::Cap::StorageAdmin)
                        || (mode > 0 && !caps.contains(kernel::caps::Cap::FactoryCalib))
                    {
                        return 1;
                    }
                    #[cfg(feature = "filesystem")]
                    if let Some(st) = active_storage_mut() {
                        let calib = unsafe { &*core::ptr::addr_of!(crate::CALIBRATION) };
                        let res = match mode {
                            0 | 1 => crate::services::calibration::reformat_storage(
                                st,
                                calib.raw_bytes(),
                                mode == 0,
                            ),
                            2 => st.fs.wipe_raw().map_err(|_| ()),
                            _ => Err(()),
                        };
                        return usize::from(res.is_err());
                    }
                    1
                }
                _ => 1,
            }
        }
        // FsRename: a0 = *const KrxFsReq (path/path_len = old, buf/len = new)
        3 => {
            let req = a0 as *const KrxFsReq;
            if !req.is_null() {
                let r = unsafe { &*req };
                if let (Some((o_str, st)), Some((n_str, _))) = (
                    user_storage(r.path, r.path_len),
                    user_storage(r.buf as *const u8, r.len as u8),
                ) {
                    if st.rename(o_str, n_str).is_ok() {
                        return 0;
                    }
                }
            }
            1
        }
        _ => 1,
    }
}

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
        assert!(proc_read("/proc/boot", 0, &mut buf).is_none());
        assert_eq!(proc_remove("/proc/boot"), Some(1));

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

        let mut tail = [0u8; 64];
        assert_eq!(proc_read("/proc/boot", 260, &mut tail), Some(56));
        assert_eq!(&tail[..56], &buf[260..316]);

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
