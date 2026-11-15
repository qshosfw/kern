//! Storage & Filesystem Subsystem System Call Handlers (`StorageApi`, `interface_id = 0x0004`).

use super::{active_ctx_opt, parse_str, parse_user_path};
use kernel::proc::abi::{KrxDirEntry, StorageApi};
use kernel::proc::primitives::*;

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
        if let (Some(path_str), Some(ctx)) =
            (unsafe { parse_user_path(path, path_len) }, active_ctx_opt())
        {
            let src = unsafe { core::slice::from_raw_parts(data, len as usize) };
            let res = if append {
                ctx.append_file(path_str, src)
            } else {
                ctx.write_file(path_str, src)
            };
            return u8::from(res.is_err());
        }
    }
    1
}

#[inline(never)]
fn user_path_unit_op(path: *const u8, path_len: u8, create_dir: bool) -> u8 {
    if let (Some(p_str), Some(ctx)) = (
        unsafe { parse_user_path(path, path_len as usize) },
        active_ctx_opt(),
    ) {
        let res = if create_dir {
            ctx.create_dir(p_str)
        } else {
            ctx.remove_file(p_str)
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
    let ctx = active_ctx_opt()?;
    ctx.file_size(path_str).ok().map(|s| s as u32)
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
    if let (Some(path_str), Some(dst), Some(ctx)) = (
        unsafe { parse_str(path, path_len as usize) },
        unsafe { super::ptr_slice_mut(buf, max_len as usize) },
        active_ctx_opt(),
    ) {
        return ctx
            .read_file_at(path_str, offset as usize, dst)
            .unwrap_or(0) as u16;
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
};
