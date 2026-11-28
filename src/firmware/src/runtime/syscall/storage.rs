//! Storage & Filesystem System Call Dispatcher (`Op::Fs*`, domain `0x3`).

use super::{parse_str, parse_user_path, ptr_ref, ptr_slice, ptr_slice_mut, str_or_default};
use kernel::proc::abi::{KrxDirEntry, KrxFsReq};

fn app_config_path_into<'a>(app_name: &str, buf: &'a mut [u8; 32]) -> &'a str {
    const PFX: &[u8] = b"/pref/app/";
    const SFX: &[u8] = b".cfg";
    let n = app_name.len().min(buf.len() - PFX.len() - SFX.len());
    buf[..PFX.len()].copy_from_slice(PFX);
    buf[PFX.len()..PFX.len() + n].copy_from_slice(&app_name.as_bytes()[..n]);
    buf[PFX.len() + n..PFX.len() + n + SFX.len()].copy_from_slice(SFX);
    unsafe { core::str::from_utf8_unchecked(&buf[..PFX.len() + n + SFX.len()]) }
}

pub fn load_app_config<D: kernfs::BlockDevice>(
    storage: &mut kernfs::KernFS<'_, D>,
    app_name: &str,
    buf: &mut [u8],
) -> Result<usize, ()> {
    let mut pbuf = [0u8; 32];
    storage
        .read_file(app_config_path_into(app_name, &mut pbuf), buf)
        .map_err(|_| ())
}

pub fn save_app_config(
    storage: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    app_name: &str,
    data: &[u8],
) -> Result<(), ()> {
    let mut pbuf = [0u8; 32];
    crate::security::write_classified_file(storage, app_config_path_into(app_name, &mut pbuf), data)
        .map_err(|_| ())
}

pub fn list_krx_dir_at<D: kernfs::BlockDevice>(
    storage: &mut kernfs::KernFS<'_, D>,
    dir_path: &str,
    skip: usize,
    out_entries: &mut [KrxDirEntry],
) -> Result<(usize, usize), ()> {
    let mut total = 0usize;
    let mut written = 0usize;
    storage
        .list_dir(dir_path, |path, size, _perm, is_dir| {
            if total >= skip {
                if let Some(dst) = out_entries.get_mut(written) {
                    unsafe { core::ptr::write_bytes(dst, 0, 1) };
                    let b = path.as_bytes();
                    let n = b.len().min(24);
                    dst.name[..n].copy_from_slice(&b[..n]);
                    dst.size = size as u32;
                    dst.is_dir = u8::from(is_dir);
                    written += 1;
                }
            }
            total += 1;
        })
        .map_err(|_| ())?;
    Ok((total, written))
}

#[inline(always)]
fn is_proc_boot_path(path: &str) -> bool {
    path == "/proc/boot"
}

#[inline(always)]
fn has_proc_read_cap() -> bool {
    super::has_cap(::abi::Cap::ServiceRegistry)
}

fn proc_read(path: &str, offset: usize, dst: &mut [u8]) -> Option<usize> {
    if !is_proc_boot_path(path) || !has_proc_read_cap() {
        return None;
    }
    let tag_bytes = crate::boot::backup::read().encode().to_le_bytes();
    let segs: [(*const u8, usize); 3] = [
        (
            crate::panic::BOOT_RECORD.get() as *const kernel::boot::BootRecord as *const u8,
            260,
        ),
        (core::ptr::addr_of!(crate::FAULT_EXTRA) as *const u8, 52),
        (tag_bytes.as_ptr(), 4),
    ];
    let mut written = 0usize;
    let mut rem_off = offset;
    for (ptr, len) in segs {
        if rem_off < len {
            let n = (dst.len() - written).min(len - rem_off);
            unsafe {
                core::ptr::copy_nonoverlapping(ptr.add(rem_off), dst.as_mut_ptr().add(written), n);
            }
            written += n;
            rem_off = 0;
        } else {
            rem_off -= len;
        }
    }
    Some(written)
}

fn proc_remove(path: &str) -> Option<u8> {
    if !is_proc_boot_path(path) {
        return None;
    }
    let caps = super::super::native_runner::active_native_caps();
    if !caps.contains(::abi::Cap::PowerManage) {
        return Some(1);
    }
    crate::boot::clear_crash_strikes();
    if caps.contains(::abi::Cap::StorageAdmin) {
        crate::panic::BOOT_RECORD.get_mut().reset_all();
    }
    Some(0)
}

fn parse_non_sys_user_path<'a>(path: *const u8, path_len: u8) -> Option<&'a str> {
    let p_str = unsafe { parse_user_path(path, path_len as usize) }?;
    (utils::path::root_bit(p_str.as_bytes()) != utils::path::ROOT_SYS).then_some(p_str)
}

fn file_stat_size(
    st: &mut kernfs::KernFS<'static, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    path: *const u8,
    path_len: u8,
) -> Option<u32> {
    let path_str = unsafe { parse_str(path, path_len as usize) }?;
    if is_proc_boot_path(path_str) && has_proc_read_cap() {
        return Some(316);
    }
    st.file_size(path_str).ok().map(|s| s as u32)
}

#[cfg(feature = "filesystem")]
unsafe fn fs_read_extents(
    st: &mut kernfs::KernFS<'static, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    path: *const u8,
    path_len: u8,
    dst: &mut [u8],
) -> usize {
    if dst.len() >= 24 && super::has_cap(::abi::Cap::StorageAdmin) {
        if let Some(path_str) = unsafe { parse_str(path, path_len as usize) } {
            let mut l1 = [0u16; 12];
            if let Ok((depth, sz)) = st.file_extents(path_str, &mut l1) {
                let roots_bytes = if dst.len() >= 32 { 24 } else { 16 };
                let p = dst.as_mut_ptr();
                unsafe {
                    core::ptr::write_bytes(p, 0, 4);
                    *p = depth;
                    core::ptr::copy_nonoverlapping(sz.to_le_bytes().as_ptr(), p.add(4), 4);
                    core::ptr::copy_nonoverlapping(l1.as_ptr() as *const u8, p.add(8), roots_bytes);
                }
                return 8 + roots_bytes;
            }
        }
    }
    0
}

#[cfg(feature = "filesystem")]
fn fs_read_stats(
    st: &kernfs::KernFS<'static, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    dst: &mut [u8],
) -> usize {
    if dst.len() >= 16 {
        let s = st.stats();
        #[repr(C)]
        struct WireStats(u16, u16, u16, u16, u16, u16, u32);
        let w = WireStats(
            s.total_sectors,
            s.used_sectors,
            s.free_sectors,
            s.deleted_sectors,
            s.active_files,
            0,
            s.wear_cycles,
        );
        unsafe {
            core::ptr::copy_nonoverlapping(&raw const w as *const u8, dst.as_mut_ptr(), 16);
        }
        return 16;
    }
    0
}

unsafe fn dispatch_fs_read(
    st: &mut kernfs::KernFS<'static, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    sub: u8,
    r: &KrxFsReq,
) -> usize {
    use ::abi::sub::fs_read;
    if sub == fs_read::LIST_DIR {
        let max = r.len as usize;
        let dir_str = unsafe { str_or_default(r.path, r.path_len as usize, "/") };
        let mut empty: [KrxDirEntry; 0] = [];
        let dst = if (r.buf as usize & 3) == 0
            && let Some(bytes) = unsafe {
                ptr_slice_mut(
                    r.buf,
                    max.saturating_mul(core::mem::size_of::<KrxDirEntry>()),
                )
            } {
            unsafe { core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut KrxDirEntry, max) }
        } else {
            &mut empty[..]
        };
        if let Ok((total, written)) = list_krx_dir_at(st, dir_str, r.offset as usize, dst) {
            return if r.extra != 0 { total } else { written };
        }
        return 0;
    }
    let Some(dst) = (unsafe { ptr_slice_mut(r.buf, r.len as usize) }) else {
        return 0;
    };
    match sub {
        fs_read::FILE_AT => {
            let Some(path_str) = (unsafe { parse_str(r.path, r.path_len as usize) }) else {
                return 0;
            };
            if let Some(n) = proc_read(path_str, r.offset as usize, dst) {
                return n;
            }
            st.read_file_at(path_str, r.offset as usize, dst)
                .unwrap_or(0)
        }
        fs_read::LOAD_CFG => {
            let name = super::super::native_runner::active_native_app_name();
            load_app_config(st, name, dst).unwrap_or(0)
        }
        fs_read::ASSET => {
            #[cfg(feature = "krx_native")]
            {
                crate::runtime::native_runner::read_active_pkg_asset(st, r.offset, dst).unwrap_or(0)
            }
            #[cfg(not(feature = "krx_native"))]
            {
                let _ = dst;
                0
            }
        }
        fs_read::EXTENTS => {
            #[cfg(feature = "filesystem")]
            {
                unsafe { fs_read_extents(st, r.path, r.path_len, dst) }
            }
            #[cfg(not(feature = "filesystem"))]
            {
                let _ = dst;
                0
            }
        }
        fs_read::STATS => {
            #[cfg(feature = "filesystem")]
            {
                fs_read_stats(st, dst)
            }
            #[cfg(not(feature = "filesystem"))]
            {
                let _ = dst;
                0
            }
        }
        _ => 0,
    }
}

unsafe fn dispatch_fs_write(
    st: &mut kernfs::KernFS<'static, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    sub: u8,
    r: &KrxFsReq,
) -> usize {
    use ::abi::sub::fs_write;
    let Some(src) = (unsafe { ptr_slice(r.buf, r.len as usize) }) else {
        return 1;
    };
    match sub {
        fs_write::FILE | fs_write::APPEND => {
            if sub == fs_write::FILE && src.len() == 512 {
                if let Some(raw_path) = unsafe { parse_str(r.path, r.path_len as usize) } {
                    if raw_path == crate::services::calibration::CALIB_FILE_PATH
                        && super::has_cap(::abi::Cap::FactoryCalib)
                    {
                        return usize::from(
                            crate::services::calibration::commit_calib_slice(
                                crate::calibration_mut(),
                                st,
                                src,
                            )
                            .is_err(),
                        );
                    }
                }
            }
            if let Some(path_str) = parse_non_sys_user_path(r.path, r.path_len) {
                let ok = if sub == fs_write::APPEND {
                    st.append_file(path_str, src).is_ok()
                } else {
                    crate::security::write_classified_file(st, path_str, src).is_ok()
                };
                return usize::from(!ok);
            }
            1
        }
        fs_write::SAVE_CFG => {
            let name = super::super::native_runner::active_native_app_name();
            usize::from(save_app_config(st, name, src).is_err())
        }
        _ => 1,
    }
}

pub unsafe fn op_fs_read(a0: usize, a1: usize, _a2: usize) -> usize {
    let Some(r) = (unsafe { ptr_ref(a1 as *const KrxFsReq) }) else {
        return 0;
    };
    unsafe { dispatch_fs_read(crate::services::storage_mut(), a0 as u8, r) }
}

pub unsafe fn op_fs_write(a0: usize, a1: usize, _a2: usize) -> usize {
    let Some(r) = (unsafe { ptr_ref(a1 as *const KrxFsReq) }) else {
        return 1;
    };
    unsafe { dispatch_fs_write(crate::services::storage_mut(), a0 as u8, r) }
}

pub unsafe fn op_fs_meta(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::fs_meta;
    let st = crate::services::storage_mut();
    let path = a1 as *const u8;
    let plen = a2 as u8;
    match a0 as u8 {
        fs_meta::SIZE | fs_meta::EXISTS => {
            let sz = file_stat_size(st, path, plen);
            if (a0 as u8) == fs_meta::SIZE {
                sz.unwrap_or(0) as usize
            } else {
                usize::from(sz.is_some())
            }
        }
        fs_meta::REMOVE | fs_meta::MKDIR => {
            let create_dir = (a0 as u8) == fs_meta::MKDIR;
            if !create_dir {
                if let Some(raw) = unsafe { parse_str(path, plen as usize) } {
                    if let Some(rc) = proc_remove(raw) {
                        return rc as usize;
                    }
                }
            }
            if let Some(p_str) = parse_non_sys_user_path(path, plen) {
                let res = if create_dir {
                    st.create_dir(p_str)
                } else {
                    st.remove(p_str)
                };
                return usize::from(res.is_err());
            }
            1
        }
        fs_meta::FORMAT => {
            let mode = a1 as u8;
            let caps = super::super::native_runner::active_native_caps();
            if !caps.contains(::abi::Cap::StorageAdmin)
                || (mode > 0 && !caps.contains(::abi::Cap::FactoryCalib))
            {
                return 1;
            }
            #[cfg(feature = "filesystem")]
            {
                let res = match mode {
                    0 | 1 => crate::services::calibration::reformat_storage(st, mode == 0),
                    2 => st.wipe_raw().map_err(|_| ()),
                    _ => Err(()),
                };
                usize::from(res.is_err())
            }
            #[cfg(not(feature = "filesystem"))]
            1
        }
        fs_meta::RENAME => {
            if !super::has_cap(::abi::Cap::StorageWrite) {
                return ::abi::Status::PermissionDenied.to_raw();
            }
            let Some(r) = (unsafe { ptr_ref(a1 as *const KrxFsReq) }) else {
                return 1;
            };
            if let (Some(o_str), Some(n_str)) = (
                parse_non_sys_user_path(r.path, r.path_len),
                parse_non_sys_user_path(r.buf as *const u8, r.len as u8),
            ) && st.rename(o_str, n_str).is_ok()
            {
                0
            } else {
                1
            }
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::abi::{Cap, CapabilitySet};

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
        crate::boot::backup::write(initial_tag);
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
        assert_eq!(crate::boot::backup::read().strikes, 0);
        unsafe {
            assert_eq!(crate::panic::BOOT_RECORD.get().slot_a.crash_count, 0);
            assert_eq!(crate::panic::BOOT_RECORD.get().slot_a.fault_pc, 0);
            crate::FAULT_EXTRA.valid = 0;
        }
        crate::boot::backup::write(kernel::boot::BackupCrashTag::empty());
        super::super::super::native_runner::init_native_slot();
    }
}
