//! KRX Application Package Loader & Dispatcher.
//!
//! Provides end-to-end binary package verification, capability token checks,
//! process table registration, and execution class dispatch for installable apps.

use crate::SysContext;
use crate::apps::registry::{AppRegistry, DynamicAppRecord};
use kernel::app::AppMetadata;
use kernel::caps::CapabilitySet;
use kernel::proc::krx::{
    KrxClass, KrxPreamble, SECTION_EXECUTION, SECTION_IDENTITY, SECTION_TYPE_LIFECYCLE,
    SectionExecution, SectionIdentity, SectionType, find_section,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KrxLaunchError {
    FileNotFound,
    ReadFailed,
    InvalidPreamble,
    CrcMismatch,
    MissingIdentity,
    CapabilityDenied,
    ExecutionFailed,
}

static mut DYNAMIC_APP_NAME_BUF: [u8; 32] = [0u8; 32];

fn store_dynamic_app_name(name: &str) -> &'static str {
    unsafe {
        let len = name.len().min(31);
        DYNAMIC_APP_NAME_BUF[..len].copy_from_slice(&name.as_bytes()[..len]);
        DYNAMIC_APP_NAME_BUF[len] = 0;
        core::str::from_utf8_unchecked(&DYNAMIC_APP_NAME_BUF[..len])
    }
}

/// Load and launch a verified KRX binary application package.
#[inline(never)]
pub fn launch_krx_package(ctx: &mut SysContext, path: &str) -> Result<usize, KrxLaunchError> {
    let mut hdr_buf = [0u8; 160];
    let read_len = ctx
        .storage
        .read_file(path, &mut hdr_buf)
        .map_err(|_| KrxLaunchError::ReadFailed)?;

    if read_len < KrxPreamble::SIZE {
        return Err(KrxLaunchError::InvalidPreamble);
    }

    let preamble =
        KrxPreamble::parse(&hdr_buf[..read_len]).map_err(|_| KrxLaunchError::InvalidPreamble)?;

    let header_len = (preamble.header_len as usize).min(read_len);

    // 1. Inspect Identity Section
    let ident_body = find_section(&hdr_buf, header_len, SECTION_IDENTITY)
        .ok_or(KrxLaunchError::MissingIdentity)?;
    let ident = SectionIdentity::parse(ident_body).ok_or(KrxLaunchError::MissingIdentity)?;
    let raw_name = ident.name(&hdr_buf[..header_len]).unwrap_or("App");

    // 2. Inspect Type & Lifecycle Section
    let mut required_caps = CapabilitySet::NONE;
    if let Some(type_body) = find_section(&hdr_buf, header_len, SECTION_TYPE_LIFECYCLE) {
        if let Some(st) = SectionType::parse(type_body) {
            required_caps = st.capabilities();
        }
    }

    // 3. Verify Hardware Capabilities
    if !ctx.caps().contains_all(required_caps) {
        return Err(KrxLaunchError::CapabilityDenied);
    }

    // 3b. Verify RAM and stack headroom requirements against Execution Section
    if let Some(exec_body) = find_section(&hdr_buf, header_len, SECTION_EXECUTION) {
        if let Some(exec) = SectionExecution::parse(exec_body) {
            let avail = kernel::sys::SystemDiagnostics::ram_available_bytes();
            let margin = kernel::sys::SystemDiagnostics::stack_margin_bytes();
            if !exec.can_execute(avail, margin.min(256)) {
                return Err(KrxLaunchError::ExecutionFailed);
            }
        }
    }

    // 4. Special binding for provisioned Explorer package
    #[cfg(feature = "app_explorer")]
    if raw_name.eq_ignore_ascii_case("Explorer") || path.ends_with("explorer.krx") {
        crate::apps::explorer::launch_explorer(ctx);
        return Ok(AppRegistry::count());
    }

    let app_name = store_dynamic_app_name(raw_name);

    // 5. Execution Class Dispatch
    let exec_class = preamble.execution_class().unwrap_or(KrxClass::Declare);
    let (vtable, instance) = match exec_class {
        KrxClass::Declare => {
            let walker = crate::apps::runtime::get_global_walker();
            walker.reset(app_name);

            // Read payload bytes to populate dynamic Declare program
            let mut payload_buf = [0u8; 256];
            let read_total = ctx.storage.read_file(path, &mut payload_buf).unwrap_or(0);
            let h_len = preamble.header_len as usize;
            if read_total > h_len {
                let payload = &payload_buf[h_len..read_total];
                let item_count =
                    payload.len() / core::mem::size_of::<kernel::proc::declare::DeclareItem>();
                if item_count > 0 {
                    let items_slice = unsafe {
                        core::slice::from_raw_parts(
                            payload.as_ptr() as *const kernel::proc::declare::DeclareItem,
                            item_count.min(16),
                        )
                    };
                    walker.load_static_package(app_name, &[], items_slice, &[], &[]);
                }
            }

            (
                crate::apps::runtime::DECLARE_WALKER_VTABLE,
                walker as *mut _ as *mut (),
            )
        }
        KrxClass::VMachine => {
            let mut code_buf = [0u8; 256];
            let read_total = ctx.storage.read_file(path, &mut code_buf).unwrap_or(0);
            let h_len = preamble.header_len as usize;
            if read_total > h_len {
                let payload = &code_buf[h_len..read_total];
                let mut words = [0u16; 128];
                let word_count = (payload.len() / 2).min(128);
                for i in 0..word_count {
                    words[i] = u16::from_le_bytes([payload[i * 2], payload[i * 2 + 1]]);
                }
                super::vm_runner::load_vm_program(app_name, &words[..word_count]);
            } else {
                static DEFAULT_BYTECODE: [u16; 4] = [
                    0x0400, // NOP
                    0x0400, // NOP
                    0x4000, // HALT
                    0x0000,
                ];
                super::vm_runner::load_vm_program(app_name, &DEFAULT_BYTECODE);
            }

            (super::vm_runner::VM_RUNNER_VTABLE, core::ptr::null_mut())
        }
        KrxClass::Native | KrxClass::Paged => {
            let exec_sec = find_section(&hdr_buf, header_len, SECTION_EXECUTION)
                .and_then(SectionExecution::parse)
                .ok_or(KrxLaunchError::ExecutionFailed)?;

            if !super::native_runner::load_native_from_storage(
                app_name,
                ctx.storage,
                path,
                &preamble,
                &exec_sec,
                required_caps,
            ) {
                return Err(KrxLaunchError::ExecutionFailed);
            }

            (
                super::native_runner::NATIVE_RUNNER_VTABLE,
                core::ptr::null_mut(),
            )
        }
    };

    let dyn_id = AppRegistry::register_dynamic(DynamicAppRecord {
        metadata: AppMetadata {
            name: app_name,
            icon: &crate::apps::launcher::assets::ICON_DIAG_16,
            icon_8: &crate::apps::launcher::assets::ICON_DIAG_8,
        },
        vtable,
        instance,
        required_capabilities: required_caps,
    });
    ctx.post_event(crate::apps::Shortcut::LaunchApp(dyn_id as u8).into());
    Ok(dyn_id)
}
