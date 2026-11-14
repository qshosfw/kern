//! KRX Application Package Loader & Binder (KRX-ABI 2.5.0 Appendix C).
//!
//! Provides end-to-end binary package verification, capability token checks,
//! `CapImports` GOT binding, `BlockMap` paged configuration, process table
//! registration, and execution class dispatch for installable packages.

use crate::SysContext;
use crate::apps::registry::{AppRegistry, DynamicAppRecord};
use kernel::app::AppMetadata;
use kernel::proc::krx::{KrxClass, KrxPreamble};

pub use kernel::proc::krx::KrxLaunchError;

static mut DYNAMIC_APP_NAME_BUF: [u8; 32] = [0u8; 32];

fn store_dynamic_app_name(name: &str) -> &'static str {
    let bytes = name.as_bytes();
    let mut len = bytes.len().min(31);
    while len > 0 && len < bytes.len() && (bytes[len] & 0xC0) == 0x80 {
        len -= 1;
    }
    unsafe {
        DYNAMIC_APP_NAME_BUF[..len].copy_from_slice(&bytes[..len]);
        DYNAMIC_APP_NAME_BUF[len] = 0;
        // Safety: `name` is valid `&str` and `len` lands on a UTF-8 codepoint boundary.
        core::str::from_utf8_unchecked(&DYNAMIC_APP_NAME_BUF[..len])
    }
}

/// Load and launch a verified KRX binary application package.
#[inline(never)]
pub fn launch_krx_package(ctx: &mut SysContext, path: &str) -> Result<usize, KrxLaunchError> {
    let header_buf = &mut crate::data::arena::scratch_tail_bytes_mut()[1792..2048];
    let header_read = ctx
        .storage
        .read_file_at(path, 0, header_buf)
        .map_err(|e| match e {
            kernel::service::StorageError::NotFound => KrxLaunchError::FileNotFound,
            _ => KrxLaunchError::ReadFailed,
        })?;

    let mut summary_uninit = core::mem::MaybeUninit::uninit();
    let summary = KrxPreamble::summarize_into(&header_buf[..header_read], &mut summary_uninit)?;
    let header_len = summary.preamble.header_len as usize;
    if header_len > header_read {
        return Err(KrxLaunchError::TruncatedPackage);
    }

    let exec_class = summary
        .preamble
        .execution_class()
        .unwrap_or(KrxClass::Declare);
    #[cfg(feature = "krx_native")]
    let is_native_or_paged = matches!(exec_class, KrxClass::Native | KrxClass::Paged);
    #[cfg(not(feature = "krx_native"))]
    let is_native_or_paged = false;
    if !is_native_or_paged {
        let _ = (ctx, path);
        return Err(match exec_class {
            KrxClass::Declare | KrxClass::VMachine => KrxLaunchError::VmDisabled,
            _ => KrxLaunchError::ExecutionFailed,
        });
    }

    let raw_name = if !summary.title.is_empty() {
        summary.title
    } else {
        "App"
    };

    // 2. Verify Hardware Capabilities
    let required_caps = summary.caps;
    if !ctx.caps().contains_all(required_caps) {
        let missing = required_caps.without(ctx.caps());
        let cap_msg = missing
            .first_cap()
            .map_or("Missing Cap", |c| c.required_message());
        return Err(KrxLaunchError::CapabilityDenied(cap_msg));
    }

    // 3. Verify RAM and stack headroom requirements against Execution Section
    if let Some(ref exec) = summary.exec {
        let avail = kernel::sys::SystemDiagnostics::ram_available_bytes();
        let margin = kernel::sys::SystemDiagnostics::stack_margin_bytes();
        let min_stack = (exec.min_stack_bytes as usize).min(128);
        if margin < min_stack.min(96) {
            return Err(KrxLaunchError::InsufficientStack);
        }
        if !exec.can_execute(avail, margin.min(128)) {
            return Err(KrxLaunchError::InsufficientRam);
        }
    }

    let app_name = store_dynamic_app_name(raw_name);
    let dyn_id = AppRegistry::count();
    let dyn_pid = crate::orchestrator::proc_table_mut()
        .spawn_or_recycle(
            app_name,
            dyn_id,
            kernel::sched::Priority::Ui,
            kernel::proc::ExecType::RelocatableNative,
            required_caps,
        )
        .unwrap_or(kernel::proc::Pid(dyn_id as u8));

    #[cfg(feature = "krx_native")]
    let (vtable, instance) = {
        let exec_sec = summary
            .exec
            .as_ref()
            .ok_or(KrxLaunchError::MissingExecution)?;
        let arena_buf = super::native_runner::transient_arena_bytes_mut();
        let max_payload = arena_buf.len() - 256;
        let to_read = if exec_sec.payload_size > 0 {
            (exec_sec.payload_size as usize).min(max_payload)
        } else {
            max_payload
        };
        let payload_read = ctx
            .storage
            .read_file_at(path, header_len, &mut arena_buf[..to_read])
            .map_err(|_| KrxLaunchError::ReadFailed)?;

        super::native_runner::load_native_from_storage(
            app_name,
            path,
            payload_read,
            &summary.preamble,
            exec_sec,
            &summary.aux,
            required_caps,
            dyn_pid,
        )?;

        (
            super::native_runner::NATIVE_RUNNER_VTABLE,
            core::ptr::null_mut(),
        )
    };
    #[cfg(not(feature = "krx_native"))]
    let (vtable, instance): (
        &'static kernel::app::AppVTable<SysContext<'static, 'static>>,
        *mut (),
    ) = {
        let _ = (ctx, path, app_name, dyn_pid, required_caps);
        return Err(KrxLaunchError::ExecutionFailed);
    };

    let dyn_id = AppRegistry::register_dynamic(DynamicAppRecord {
        metadata: AppMetadata {
            name: app_name,
            icon: &gfx::icons::ICON_16_DIAGNOSTICS,
            icon_8: gfx::icons::ICON_APP_DIAG,
        },
        vtable,
        instance,
        required_capabilities: required_caps,
    });
    ctx.post_event(crate::apps::Shortcut::LaunchApp(dyn_id as u8).into());
    Ok(dyn_id)
}
