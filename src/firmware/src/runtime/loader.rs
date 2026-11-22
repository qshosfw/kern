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
static mut DYNAMIC_APP_NAME_BUF: [u8; DYNAMIC_APP_NAME_CAP] = [0u8; DYNAMIC_APP_NAME_CAP];
static mut DYNAMIC_APP_NAME_LEN: u8 = 0;

/// Diagnostic detail sub-codes for `LaunchDiag`.
pub(crate) const DIAG_NONE: u8 = 0;
pub(crate) const DIAG_ABI_MISMATCH: u8 = 1;
pub(crate) const DIAG_GOT_INVALID: u8 = 2;
pub(crate) const DIAG_TOO_MANY_LEAVES: u8 = 3;
pub(crate) const DIAG_PROFILE_UNSUPPORTED: u8 = 4;

/// Structured diagnostics captured at the exact point a KRX launch check fails.
#[derive(Clone, Copy)]
pub(crate) struct LaunchDiag {
    pub(crate) a: u32,
    pub(crate) b: u32,
    pub(crate) code: u8,
}

impl LaunchDiag {
    const EMPTY: Self = Self {
        a: 0,
        b: 0,
        code: DIAG_NONE,
    };
}

static mut LAST_LAUNCH_DIAG: LaunchDiag = LaunchDiag::EMPTY;

#[inline(always)]
pub(crate) fn clear_launch_diag() {
    unsafe {
        LAST_LAUNCH_DIAG = LaunchDiag::EMPTY;
    }
}

#[inline(always)]
pub(crate) fn record_launch_diag(a: u32, b: u32, _app_ver: [u8; 3], code: u8) {
    unsafe {
        LAST_LAUNCH_DIAG = LaunchDiag {
            a,
            b,
            code,
        };
    }
}

#[inline(always)]
fn last_launch_diag() -> LaunchDiag {
    unsafe { LAST_LAUNCH_DIAG }
}

pub(crate) fn dynamic_app_name() -> &'static str {
    unsafe {
        let len = (DYNAMIC_APP_NAME_LEN as usize).min(DYNAMIC_APP_NAME_CAP - 1);
        let bytes = core::slice::from_raw_parts(
            core::ptr::addr_of!(DYNAMIC_APP_NAME_BUF).cast::<u8>(),
            len,
        );
        core::str::from_utf8_unchecked(bytes)
    }
}

fn store_dynamic_app_name(name: &str) -> &'static str {
    let bytes = name.as_bytes();
    let mut len = bytes.len().min(DYNAMIC_APP_NAME_CAP - 1);
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
    let tail = crate::data::arena::scratch_tail_bytes_mut();
    let tail_len = tail.len();
    let (pre_hdr, hdr_buf) = tail.split_at_mut(tail_len - HDR_RESERVE);
    let summary_offset = pre_hdr.len() - SUMMARY_RESERVE;
    let summary_slot = unsafe {
        &mut *(pre_hdr.as_mut_ptr().add(summary_offset)
            as *mut core::mem::MaybeUninit<kernel::runtime::KrxSummary<'_>>)
    };
    clear_launch_diag();
    let summary = super::read_krx_summary(storage, path, hdr_buf, summary_slot)?;
    let header_len = summary.preamble.header_len as usize;
    let app_ver = summary.version;

    let Some(exec_class) = summary.preamble.execution_class() else {
        record_launch_diag(
            summary.preamble.app_type as u32,
            0,
            app_ver,
            DIAG_PROFILE_UNSUPPORTED,
        );
        return Err(KrxLaunchError::UnsupportedProfile);
    };
    if !matches!(exec_class, KrxClass::Native | KrxClass::Paged) {
        record_launch_diag(
            exec_class as u32,
            summary.preamble.app_type as u32,
            app_ver,
            DIAG_PROFILE_UNSUPPORTED,
        );
        return Err(KrxLaunchError::UnsupportedProfile);
    }

    // Operation numbers are only meaningful for the ABI the package was built against.
    if summary.abi != ::abi::VERSION {
        record_launch_diag(
            summary.abi as u32,
            ::abi::VERSION as u32,
            app_ver,
            DIAG_ABI_MISMATCH,
        );
        return Err(KrxLaunchError::ImportBindingFailed);
    }

    let raw_name = summary.title.unwrap_or("App");

    let required_caps = summary.caps;
    if let Some(granted) = sys_caps {
        if !granted.contains_all(required_caps) {
            let cap_msg = required_caps
                .without(granted)
                .first_cap()
                .map_or("", kernel::caps::name);
            return Err(KrxLaunchError::CapabilityDenied(cap_msg));
        }
    }

    let exec = summary.exec().ok_or(KrxLaunchError::MissingExecution)?;
    let profile = kernel::runtime::KrxProfile::detect(
        &summary.preamble,
        exec,
        summary.aux.block_map.is_some(),
        summary.aux.leaf_map.is_some(),
    );
    #[cfg(not(feature = "krx_paged"))]
    if profile == kernel::runtime::KrxProfile::P1TransparentPaged {
        record_launch_diag(
            exec_class as u32,
            profile as u32,
            app_ver,
            DIAG_PROFILE_UNSUPPORTED,
        );
        return Err(KrxLaunchError::UnsupportedProfile);
    }
    if profile.is_structured_paged()
        && let Some(map) = summary.aux.leaf_map
        && map.len()
            > super::native_runner::MAX_STRUCTURED_LEAVES * kernel::runtime::LeafEntry::SIZE
    {
        record_launch_diag(
            (map.len() / kernel::runtime::LeafEntry::SIZE) as u32,
            super::native_runner::MAX_STRUCTURED_LEAVES as u32,
            app_ver,
            DIAG_TOO_MANY_LEAVES,
        );
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
            let min_stack = (exec.min_stack_bytes as usize).min(96);
            let needed = min_stack.saturating_add(32);
            if margin < needed {
                record_launch_diag(needed as u32, margin as u32, app_ver, DIAG_NONE);
                return Err(KrxLaunchError::InsufficientStack);
            }
        }
        let needed_ram = (exec.min_stack_bytes as usize)
            .saturating_add(exec.min_heap_bytes as usize)
            .saturating_add(64);
        if !exec.can_execute(avail, 64) {
            record_launch_diag(needed_ram as u32, avail as u32, app_ver, DIAG_NONE);
            return Err(KrxLaunchError::InsufficientRam);
        }
        let code_limit = if profile.is_structured_paged() {
            avail - 1024
        } else {
            avail
        };
        if !matches!(exec_class, KrxClass::Paged) && (exec.payload_size as usize) > avail {
            record_launch_diag(exec.payload_size, avail as u32, app_ver, DIAG_NONE);
            return Err(KrxLaunchError::PayloadTooLarge);
        }
        if (exec.got_offset & 3) != 0 || (exec.got_offset as usize) >= avail {
            record_launch_diag(
                exec.got_offset as u32,
                avail as u32,
                app_ver,
                DIAG_GOT_INVALID,
            );
            return Err(KrxLaunchError::ImportBindingFailed);
        }
        if exec.payload_size > 0 {
            let code_size = if exec.got_offset > 0 {
                (exec.got_offset as u32).min(code_limit as u32)
            } else {
                exec.payload_size.min(code_limit as u32)
            };
            if (exec.entry_point & !1) >= code_size {
                record_launch_diag(exec.entry_point, code_size, app_ver, DIAG_NONE);
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
    let payload_read = if let Some(ctx) = active_ctx.as_deref_mut() {
        ctx.storage
            .read_file_at(path, header_len, &mut arena_buf[..to_read])
            .map_err(|_| KrxLaunchError::ReadFailed)?
    } else {
        storage
            .read_file_at(path, header_len, &mut arena_buf[..to_read])
            .map_err(|_| KrxLaunchError::ReadFailed)?
    };

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
    let (required_caps, app_name) =
        load_krx_into_slot(unsafe { &mut *storage_ptr }, path, Some(caps), true, Some(ctx))?;
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

/// Zero-stack, bounded ASCII/UTF-8 message builder into a caller-supplied staging slice.
struct MsgWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> MsgWriter<'a> {
    #[inline(always)]
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline(never)]
    fn push(&mut self, s: &str) {
        let src = s.as_bytes();
        let n = src.len().min(self.buf.len() - self.pos);
        self.buf[self.pos..self.pos + n].copy_from_slice(&src[..n]);
        self.pos += n;
    }

    #[inline(never)]
    fn push_u32(&mut self, val: u32) {
        let mut tmp = [0u8; 10];
        self.push(gfx::text::format_u32(val, &mut tmp));
    }

    #[inline(never)]
    fn push_pair(&mut self, pfx: &str, a: u32, sep: &str, b: u32, sfx: &str) {
        self.push(pfx);
        self.push_u32(a);
        self.push(sep);
        self.push_u32(b);
        self.push(sfx);
    }

    #[inline(always)]
    fn finish(self) -> &'a str {
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.pos]) }
    }
}

/// Format a detailed, version- and RAM-aware diagnostic body for a KRX launch error.
#[inline(never)]
fn format_launch_error<'a>(
    err: KrxLaunchError,
    diag: LaunchDiag,
    buf: &'a mut [u8],
) -> &'a str {
    let mut w = MsgWriter::new(buf);
    match err {
        KrxLaunchError::CapabilityDenied(cap) if !cap.is_empty() => {
            w.push("Missing cap:\n");
            w.push(cap);
            w.finish()
        }
        KrxLaunchError::ImportBindingFailed => {
            let os_abi = if diag.b != 0 { diag.b } else { ::abi::VERSION as u32 };
            if diag.code == DIAG_GOT_INVALID {
                w.push_pair("Bad GOT ", diag.a, "/", diag.b, "B.\nRebuild.");
            } else {
                w.push_pair("ABI mismatch\nApp v", diag.a, " != OS v", os_abi, ".\nUpdate.");
            }
            w.finish()
        }
        KrxLaunchError::InsufficientStack => {
            let free = if diag.b != 0 {
                diag.b
            } else {
                kernel::sys::SystemDiagnostics::stack_margin_bytes() as u32
            };
            w.push_pair("Low stack RAM\nNeed ", diag.a, "B, free ", free, "B.");
            w.finish()
        }
        KrxLaunchError::InsufficientRam | KrxLaunchError::PayloadTooLarge => {
            w.push_pair("Too large\nNeed ", diag.a, "B, max ", diag.b, "B.\nUse P0.");
            w.finish()
        }
        KrxLaunchError::UnsupportedProfile => {
            if diag.code == DIAG_TOO_MANY_LEAVES {
                w.push_pair("Leaves ", diag.a, " > max ", diag.b, ".\nRebuild.");
            } else {
                w.push_pair("Unsupported prof\nClass ", diag.a, " prof ", diag.b, ".");
            }
            w.finish()
        }
        KrxLaunchError::TruncatedPackage if diag.b > 0 => {
            w.push_pair("Truncated\nRead ", diag.a, "B < ", diag.b, "B.");
            w.finish()
        }
        KrxLaunchError::InvalidEntryPoint if diag.b > 0 => {
            w.push_pair("Bad entry ", diag.a, " >= ", diag.b, "B.");
            w.finish()
        }
        _ => err.describe(),
    }
}

/// Display a detailed, actionable error popup for KRX package launch failures.
#[inline(never)]
pub fn show_launch_error_dialog(ctx: &mut SysContext, path: &str, err: KrxLaunchError) {
    let fname = crate::apps::launcher::catalog::extract_filename(path);
    let mut buf = [0u8; 88];
    let msg = format_launch_error(err, last_launch_diag(), &mut buf);

    ctx.show_confirm(
        crate::orchestrator::compositor::TAG_SYSTEM_ALERT,
        fname,
        msg,
        "OK",
        "",
    );
}
