//! KRX Application Package Loader & Binder (KRX-ABI 2.5.0 Appendix C).
//!
//! Provides end-to-end binary package verification, capability token checks,
//! `CapImports` GOT binding, `BlockMap` paged configuration, process table
//! registration, and execution class dispatch for installable packages.

use crate::SysContext;
use crate::apps::registry::{AppRegistry, DynamicAppRecord};
use kernel::app::AppMetadata;
use kernel::runtime::KrxClass;

pub use kernel::runtime::KrxLaunchError;

const DYNAMIC_APP_NAME_CAP: usize = 20;

#[repr(C)]
struct DynamicAppName {
    len: u8,
    buf: [u8; DYNAMIC_APP_NAME_CAP],
}

static mut DYNAMIC_APP_NAME: DynamicAppName = DynamicAppName {
    len: 0,
    buf: [0u8; DYNAMIC_APP_NAME_CAP],
};

pub(crate) fn dynamic_app_name() -> &'static str {
    unsafe {
        let slot = &*core::ptr::addr_of!(DYNAMIC_APP_NAME);
        let len = (slot.len as usize).min(DYNAMIC_APP_NAME_CAP - 1);
        core::str::from_utf8_unchecked(&slot.buf[..len])
    }
}

fn store_dynamic_app_name(name: &str) -> &'static str {
    let bytes = name.as_bytes();
    let mut len = bytes.len().min(DYNAMIC_APP_NAME_CAP - 1);
    while len > 0 && len < bytes.len() && (bytes[len] & 0xC0) == 0x80 {
        len -= 1;
    }
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(DYNAMIC_APP_NAME);
        slot.buf[..len].copy_from_slice(&bytes[..len]);
        slot.buf[len] = 0;
        slot.len = len as u8;
        // Safety: `name` is valid `&str` and `len` lands on a UTF-8 codepoint boundary.
        core::str::from_utf8_unchecked(&slot.buf[..len])
    }
}

/// Shared header validation, capability/RAM check, and native/paged payload loader.
///
/// With `spawn`, the PCB is allocated before the slot is bound, so the slot, its leases,
/// service exports and radio client all carry the PCB's own `Pid` (never an app index).
/// Transient modules (`spawn == false`) run as `Pid(0)`.
#[inline(never)]
fn load_krx_into_slot(
    storage: &mut crate::services::SystemStorage<'_>,
    path: &str,
    sys_caps: Option<::abi::CapabilitySet>,
    spawn: bool,
    mut active_ctx: Option<&mut SysContext<'_, '_>>,
) -> Result<(::abi::CapabilitySet, &'static str), KrxLaunchError> {
    const HDR_RESERVE: usize = kernel::runtime::KrxPreamble::MAX_HEADER_SIZE;
    const SUMMARY_RESERVE: usize =
        (core::mem::size_of::<kernel::runtime::KrxSummary<'static>>() + 7) & !7;
    let (_, summary) = super::read_krx_summary_in_scratch(storage, path)?;
    let header_len = summary.preamble.header_len as usize;

    let Some(exec_class) = summary.preamble.execution_class() else {
        return Err(KrxLaunchError::UnsupportedProfile);
    };

    // Operation numbers are only meaningful for the ABI the package was built against.
    if summary.abi != ::abi::VERSION {
        return Err(KrxLaunchError::ImportBindingFailed);
    }

    let raw_name = summary.title.unwrap_or("App");

    let required_caps = summary.caps;
    if let Some(granted) = sys_caps {
        if !granted.contains_all(required_caps) {
            return Err(KrxLaunchError::CapabilityDenied(
                required_caps.without(granted).first_cap(),
            ));
        }
    }

    let exec = summary.exec().ok_or(KrxLaunchError::MissingExecution)?;
    let profile = summary.profile(exec);
    #[cfg(not(feature = "krx_paged"))]
    if profile == kernel::runtime::KrxProfile::P1TransparentPaged {
        return Err(KrxLaunchError::UnsupportedProfile);
    }
    if profile.is_structured_paged()
        && let Some(map) = summary.aux.leaf_map
        && map.len()
            > super::native_runner::MAX_STRUCTURED_LEAVES * kernel::runtime::LeafEntry::SIZE
    {
        return Err(KrxLaunchError::UnsupportedProfile);
    }
    let load_base = super::native_runner::arena_base_ptr() as u32;
    if profile == kernel::runtime::KrxProfile::N0NativeFixed
        && summary.preamble.link_base != load_base
    {
        return Err(KrxLaunchError::RelocationFailed);
    }

    {
        let avail = crate::data::arena::TRANSIENT_ARENA_BYTES;
        if sys_caps.is_some() {
            let margin = kernel::sys::SystemDiagnostics::stack_margin_bytes();
            let needed = (exec.min_stack_bytes as usize).min(96).saturating_add(32);
            if margin < needed {
                return Err(KrxLaunchError::InsufficientStack);
            }
        }
        if !exec.can_execute(avail, 64) {
            return Err(KrxLaunchError::InsufficientRam);
        }
        let code_limit = if profile.is_structured_paged() {
            avail - 1024
        } else {
            avail
        };
        if !matches!(exec_class, KrxClass::Paged) && (exec.payload_size as usize) > avail {
            return Err(KrxLaunchError::PayloadTooLarge);
        }
        if (exec.got_offset & 3) != 0 || (exec.got_offset as usize) >= avail {
            return Err(KrxLaunchError::ImportBindingFailed);
        }
        if exec.payload_size > 0 {
            let code_size = if exec.got_offset > 0 {
                (exec.got_offset as u32).min(code_limit as u32)
            } else {
                exec.payload_size.min(code_limit as u32)
            };
            if (exec.entry_point & !1) >= code_size {
                return Err(KrxLaunchError::InvalidEntryPoint);
            }
        }
    }

    // All pre-flight checks succeeded: cleanly exit any currently loaded dynamic app
    // before reading the new payload into TRANSIENT_ARENA or mutating NATIVE_SLOT.
    if spawn && super::native_runner::slot_ref().is_loaded {
        let old_pid = super::native_runner::slot_ref().pid;
        if let Some(ctx) = active_ctx.as_deref_mut() {
            unsafe {
                let static_ctx =
                    &mut *(ctx as *mut SysContext<'_, '_> as *mut SysContext<'static, 'static>);
                super::native_runner::native_on_exit(static_ctx);
            }
        } else {
            super::native_runner::init_native_slot();
        }
        if old_pid.0 != 0 {
            crate::orchestrator::reap_pid_only(old_pid);
        }
    }

    let app_name = store_dynamic_app_name(raw_name);
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

    let pid = if spawn {
        crate::orchestrator::proc_table_mut()
            .spawn_or_recycle(
                app_name,
                AppRegistry::count(),
                kernel::sched::Priority::Ui,
                kernel::proc::ExecType::RelocatableNative,
                required_caps,
            )
            .map_err(|_| KrxLaunchError::InsufficientRam)?
    } else {
        kernel::proc::Pid(0)
    };
    super::native_runner::load_native_from_storage(path, payload_read, summary, pid)
        .inspect_err(|_| crate::orchestrator::reap_pid_only(pid))?;

    Ok((required_caps, app_name))
}

/// Load a verified KRX package into its own process and queue it for foreground launch.
///
/// `start_arg` reaches the package's `on_start` (shortcut action ID; `0` for a plain launch).
#[inline(never)]
pub fn launch_krx_package(
    ctx: &mut SysContext,
    path: &str,
    start_arg: u32,
) -> Result<usize, KrxLaunchError> {
    let caps = ctx.caps();
    let storage_ptr = ctx.storage as *mut crate::services::SystemStorage<'_>;
    let (required_caps, app_name) = load_krx_into_slot(
        unsafe { &mut *storage_ptr },
        path,
        Some(caps),
        true,
        Some(ctx),
    )?;
    #[cfg(feature = "filesystem")]
    crate::services::lang::bind_app(ctx.storage, path);
    super::native_runner::set_pending_start_arg(start_arg);
    let dyn_id = AppRegistry::register_dynamic(DynamicAppRecord {
        metadata: AppMetadata { name: app_name },
        required_capabilities: required_caps,
    });
    ctx.post_event(crate::apps::Shortcut::LaunchApp(dyn_id as u8).into());
    Ok(dyn_id)
}

/// Load and invoke an in-place system action module (`scope::SYSTEM_ACTION`) without switching apps.
pub fn invoke_action_krx_package<'a, 'hw>(
    ctx: &mut SysContext<'a, 'hw>,
    path: &str,
) -> Result<(), KrxLaunchError> {
    load_krx_into_slot(ctx.storage, path, Some(ctx.caps()), false, None)?;
    #[cfg(feature = "filesystem")]
    crate::services::lang::bind_app(ctx.storage, path);
    unsafe {
        let static_ctx =
            &mut *(ctx as *mut SysContext<'a, 'hw> as *mut SysContext<'static, 'static>);
        super::native_runner::native_on_start(static_ctx);
    }
    Ok(())
}

/// Forward a modal result to an in-place action module and unload its transient slot.
pub fn finish_action_krx_package<'a, 'hw>(
    ctx: &mut SysContext<'a, 'hw>,
    result: &kernel::app::ModalResult<'_>,
) -> kernel::app::EventResult {
    unsafe {
        let static_ctx =
            &mut *(ctx as *mut SysContext<'a, 'hw> as *mut SysContext<'static, 'static>);
        let res = super::native_runner::native_on_modal_result(static_ctx, result);
        super::native_runner::native_on_exit(static_ctx);
        #[cfg(feature = "filesystem")]
        crate::services::lang::unbind_app();
        res
    }
}

/// Load and execute a standalone KRX module directly during early boot (before `SysContext` exists).
pub fn try_launch_boot_module(
    _lcd: &mut drivers::st7565::St7565<bsp::LcdBus>,
    storage: &mut crate::services::SystemStorage<'_>,
    path: &str,
    param: usize,
) -> Result<u32, KrxLaunchError> {
    load_krx_into_slot(storage, path, None, false, None)?;

    let res = unsafe {
        let rc = super::native_runner::invoke_boot_start(param);
        super::native_runner::slot_mut().is_loaded = false;
        rc
    };
    Ok(res)
}

/// Display a detailed, actionable error popup for KRX package launch failures.
#[inline(never)]
pub fn show_launch_error_dialog(ctx: &mut SysContext, path: &str, err: KrxLaunchError) {
    let fname = crate::apps::launcher::catalog::extract_filename(path);
    let mut buf = [0u8; 40];
    let msg = if let KrxLaunchError::CapabilityDenied(Some(cap)) = err {
        let cap_name = kernel::caps::name(cap);
        const PFX: &[u8] = b"Missing cap:\n";
        buf[..PFX.len()].copy_from_slice(PFX);
        let n = cap_name.len().min(buf.len() - PFX.len());
        buf[PFX.len()..PFX.len() + n].copy_from_slice(&cap_name.as_bytes()[..n]);
        unsafe { core::str::from_utf8_unchecked(&buf[..PFX.len() + n]) }
    } else {
        err.describe()
    };

    ctx.show_confirm(
        crate::orchestrator::compositor::TAG_SYSTEM_ALERT,
        fname,
        msg,
        "OK",
        "",
    );
}
