//! KRX Application Package Loader & Binder (KRX-ABI 2.5.0 Appendix C).
//!
//! Provides end-to-end binary package verification, capability token checks,
//! `CapImports` GOT binding, `BlockMap` paged configuration, process table
//! registration, and execution class dispatch for installable packages.

use crate::SysContext;
use crate::apps::registry::{AppRegistry, DynamicAppRecord};
use kernel::app::AppMetadata;
use kernel::proc::krx::{KrxClass, KrxPreamble, KrxSummary};

pub use kernel::proc::krx::KrxLaunchError;

static mut DYNAMIC_APP_NAME_BUF: [u8; 32] = [0u8; 32];
static mut DYNAMIC_APP_NAME_LEN: u8 = 0;

pub(crate) fn dynamic_app_name() -> &'static str {
    unsafe {
        let len = (DYNAMIC_APP_NAME_LEN as usize).min(31);
        let bytes = core::slice::from_raw_parts(
            core::ptr::addr_of!(DYNAMIC_APP_NAME_BUF).cast::<u8>(),
            len,
        );
        core::str::from_utf8_unchecked(bytes)
    }
}

fn store_dynamic_app_name(name: &str) -> &'static str {
    let bytes = name.as_bytes();
    let mut len = bytes.len().min(31);
    while len > 0 && len < bytes.len() && (bytes[len] & 0xC0) == 0x80 {
        len -= 1;
    }
    unsafe {
        DYNAMIC_APP_NAME_BUF[..len].copy_from_slice(&bytes[..len]);
        DYNAMIC_APP_NAME_BUF[len] = 0;
        DYNAMIC_APP_NAME_LEN = len as u8;
        // Safety: `name` is valid `&str` and `len` lands on a UTF-8 codepoint boundary.
        core::str::from_utf8_unchecked(&DYNAMIC_APP_NAME_BUF[..len])
    }
}

/// Read and parse a KRX package header from storage into `out` (single shared read + summary path).
#[inline(never)]
pub fn read_krx_summary<'a, 'b>(
    storage: &mut crate::services::SystemStorage,
    path: &str,
    hdr_buf: &'a mut [u8],
    out: &'b mut core::mem::MaybeUninit<KrxSummary<'a>>,
) -> Result<&'b mut KrxSummary<'a>, KrxLaunchError> {
    let read_len = storage
        .read_file_at(path, 0, hdr_buf)
        .map_err(|e| match e {
            kernel::service::StorageError::NotFound => KrxLaunchError::FileNotFound,
            _ => KrxLaunchError::ReadFailed,
        })?;
    KrxPreamble::summarize_into(&hdr_buf[..read_len], out)
}

/// Shared header validation, capability/RAM check, and native/paged payload loader.
#[inline(never)]
fn load_krx_into_slot(
    storage: &mut crate::services::SystemStorage<'_>,
    path: &str,
    sys_caps: Option<kernel::caps::CapabilitySet>,
    pid: kernel::proc::Pid,
) -> Result<(kernel::caps::CapabilitySet, &'static str, u16), KrxLaunchError> {
    const HDR_RESERVE: usize = kernel::proc::krx::KrxPreamble::MAX_HEADER_SIZE;
    const SUMMARY_RESERVE: usize =
        (core::mem::size_of::<kernel::proc::krx::KrxSummary<'static>>() + 7) & !7;
    let tail = crate::data::arena::scratch_tail_bytes_mut();
    let tail_len = tail.len();
    let (pre_hdr, hdr_buf) = tail.split_at_mut(tail_len - HDR_RESERVE);
    let summary_offset = pre_hdr.len() - SUMMARY_RESERVE;
    let summary_slot = unsafe {
        &mut *(pre_hdr.as_mut_ptr().add(summary_offset)
            as *mut core::mem::MaybeUninit<kernel::proc::krx::KrxSummary<'_>>)
    };
    let summary = read_krx_summary(storage, path, hdr_buf, summary_slot)?;
    let header_len = summary.preamble.header_len as usize;

    let Some(exec_class) = summary.preamble.execution_class() else {
        return Err(KrxLaunchError::UnsupportedProfile);
    };
    #[cfg(feature = "krx_native")]
    let is_native_or_paged = matches!(exec_class, KrxClass::Native | KrxClass::Paged);
    #[cfg(not(feature = "krx_native"))]
    let is_native_or_paged = false;
    if !is_native_or_paged {
        return Err(KrxLaunchError::UnsupportedProfile);
    }

    let raw_name = if !summary.title.is_empty() {
        summary.title
    } else {
        "App"
    };

    let required_caps = summary.caps;
    let icon_id = summary.icon_id;
    if let Some(granted) = sys_caps {
        if !granted.contains_all(required_caps) {
            #[cfg(feature = "settings_debug")]
            let cap_msg = required_caps
                .without(granted)
                .first_cap()
                .map_or("", |c| c.required_message());
            #[cfg(not(feature = "settings_debug"))]
            let cap_msg = "";
            return Err(KrxLaunchError::CapabilityDenied(cap_msg));
        }
    }

    let exec = summary
        .exec
        .as_ref()
        .ok_or(KrxLaunchError::MissingExecution)?;
    {
        let avail = crate::data::arena::TRANSIENT_ARENA_BYTES;
        if sys_caps.is_some() {
            let margin = kernel::sys::SystemDiagnostics::stack_margin_bytes();
            let min_stack = (exec.min_stack_bytes as usize).min(96);
            if margin < min_stack.saturating_add(32) {
                return Err(KrxLaunchError::InsufficientStack);
            }
        }
        if !exec.can_execute(avail, 64) {
            return Err(KrxLaunchError::InsufficientRam);
        }
    }

    let app_name = store_dynamic_app_name(raw_name);
    #[cfg(feature = "krx_native")]
    {
        let arena_buf = super::native_runner::transient_arena_bytes_mut();
        let max_payload = arena_buf.len() - HDR_RESERVE - SUMMARY_RESERVE;
        let to_read = if exec.payload_size > 0 {
            (exec.payload_size as usize).min(max_payload)
        } else {
            max_payload
        };
        let payload_read = storage
            .read_file_at(path, header_len, &mut arena_buf[..to_read])
            .map_err(|_| KrxLaunchError::ReadFailed)?;

        super::native_runner::load_native_from_storage(path, payload_read, summary, pid)?;
    }
    #[cfg(not(feature = "krx_native"))]
    {
        let _ = (app_name, pid, header_len);
        return Err(KrxLaunchError::ExecutionFailed);
    }

    Ok((required_caps, app_name, icon_id))
}

/// Load and launch a verified KRX binary application package.
#[inline(never)]
pub fn launch_krx_package(ctx: &mut SysContext, path: &str) -> Result<usize, KrxLaunchError> {
    let dyn_id = AppRegistry::count();
    let dyn_pid = kernel::proc::Pid(dyn_id as u8);
    let (required_caps, app_name, icon_id) =
        load_krx_into_slot(ctx.storage, path, Some(ctx.caps()), dyn_pid)?;

    let _ = crate::orchestrator::proc_table_mut().spawn_or_recycle(
        app_name,
        dyn_id,
        kernel::sched::Priority::Ui,
        kernel::proc::ExecType::RelocatableNative,
        required_caps,
    );

    #[cfg(not(feature = "krx_native"))]
    return Err(KrxLaunchError::ExecutionFailed);

    #[cfg(feature = "krx_native")]
    {
        let (icon_8, icon_16) = crate::apps::settings::apps::package_icons(app_name, icon_id);
        let dyn_id = AppRegistry::register_dynamic(DynamicAppRecord {
            metadata: AppMetadata {
                name: app_name,
                icon: icon_16,
                icon_8,
            },
            required_capabilities: required_caps,
        });
        ctx.post_event(crate::apps::Shortcut::LaunchApp(dyn_id as u8).into());
        Ok(dyn_id)
    }
}

/// Load and launch a verified KRX binary application package with a specific action ID argument.
#[inline(never)]
pub fn launch_krx_package_with_arg(
    ctx: &mut SysContext,
    path: &str,
    arg: u32,
) -> Result<usize, KrxLaunchError> {
    #[cfg(feature = "krx_native")]
    super::native_runner::set_pending_start_arg(arg);
    let _ = arg;
    let res = launch_krx_package(ctx, path);
    if res.is_err() {
        #[cfg(feature = "krx_native")]
        super::native_runner::set_pending_start_arg(0);
    }
    res
}

/// Load and invoke an in-place system action module (`FLAG_SYSTEM_ACTION`) without switching apps.
#[cfg(all(feature = "filesystem", feature = "krx_native"))]
#[inline(never)]
pub fn invoke_action_krx_package<'a, 'hw>(
    ctx: &mut SysContext<'a, 'hw>,
    path: &str,
) -> Result<(), KrxLaunchError> {
    load_krx_into_slot(ctx.storage, path, Some(ctx.caps()), kernel::proc::Pid(0))?;
    unsafe {
        let static_ctx =
            &mut *(ctx as *mut SysContext<'a, 'hw> as *mut SysContext<'static, 'static>);
        super::native_runner::native_on_start(static_ctx);
    }
    Ok(())
}

/// Forward a modal result to an in-place action module and unload its transient slot.
#[cfg(all(feature = "filesystem", feature = "krx_native"))]
#[inline(never)]
pub fn finish_action_krx_package<'a, 'hw>(
    ctx: &mut SysContext<'a, 'hw>,
    result: &kernel::app::ModalResult<'_>,
) -> kernel::app::EventResult {
    unsafe {
        let static_ctx =
            &mut *(ctx as *mut SysContext<'a, 'hw> as *mut SysContext<'static, 'static>);
        let res = super::native_runner::native_on_modal_result(static_ctx, result);
        super::native_runner::native_on_exit(static_ctx);
        res
    }
}

/// Load and execute a standalone KRX module directly during early boot (before `SysContext` exists).
#[cfg(all(feature = "filesystem", feature = "krx_native"))]
#[inline(never)]
pub fn try_launch_boot_module(
    _lcd: &mut drivers::st7565::St7565<'_>,
    storage: &mut crate::services::SystemStorage<'_>,
    path: &str,
    param: usize,
) -> Result<u32, KrxLaunchError> {
    load_krx_into_slot(storage, path, None, kernel::proc::Pid(0))?;

    let res = unsafe {
        let rc = super::native_runner::invoke_boot_start(param);
        super::native_runner::slot_mut().is_loaded = false;
        rc
    };
    Ok(res)
}

/// Display a concise error popup for KRX package launch failures.
#[inline(never)]
pub fn show_launch_error_dialog(ctx: &mut SysContext, path: &str, err: KrxLaunchError) {
    let fname = crate::apps::launcher::catalog::extract_filename(path);

    const HUMAN_MSGS: &[u8] = b"\
App file was not found\non storage.\0\
Could not read app file\nfrom storage.\0\
App file is invalid or\ncorrupted.\0\
App checksum failed.\nPlease reinstall it.\0\
App info is missing.\nPlease reinstall it.\0\
App has no runnable code.\0\
Permission required to\nrun this app.\0\
Not enough free memory\nto open this app.\0\
Not enough free stack\nto open this app.\0\
Could not load app into\nmemory.\0\
App requires a newer\nfirmware version.\0\
App is too large for\navailable memory.\0\
App could not be started.\0\
App start point is\ninvalid.\0\
App file is incomplete.\nPlease reinstall it.\0\
App format is not\nsupported on this radio.\0";

    let msg = match err {
        #[cfg(any(test, feature = "settings_debug"))]
        KrxLaunchError::CapabilityDenied(cap) if !cap.is_empty() => {
            let buf = &mut crate::data::arena::staging_buf()[..64];
            let mut pos = 0usize;
            for part in [b"Needs ".as_slice(), cap.as_bytes(), b"\nto run this app."] {
                let n = part.len().min(buf.len() - pos);
                buf[pos..pos + n].copy_from_slice(&part[..n]);
                pos += n;
            }
            // SAFETY: all parts are valid UTF-8 (`cap` is `&str`, literals are ASCII).
            unsafe { core::str::from_utf8_unchecked(&buf[..pos]) }
        }
        _ => utils::format::nth_cstr(HUMAN_MSGS, err.index()),
    };

    ctx.show_confirm(
        crate::apps::settings::dialog::TAG_POPUP_INFO,
        fname,
        msg,
        "OK",
        "",
    );
}
