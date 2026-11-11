//! KRX Application Package Loader & Binder (KRX-ABI 2.5.0 Appendix C).
//!
//! Provides end-to-end binary package verification, capability token checks,
//! `CapImports` GOT binding, `BlockMap` paged configuration, process table
//! registration, and execution class dispatch for installable packages.

use crate::SysContext;
use crate::apps::registry::{AppRegistry, DynamicAppRecord};
use kernel::app::AppMetadata;
#[cfg(feature = "krx_declare")]
use kernel::proc::declare::{ActionRecord, DeclareItem, DeclareNode, LAYOUT_MENU};
use kernel::proc::krx::{KrxClass, KrxPreamble, SectionExecution};

pub use kernel::proc::krx::KrxLaunchError;

static mut DYNAMIC_APP_NAME_BUF: [u8; 32] = [0u8; 32];

fn store_dynamic_app_name(name: &str) -> &'static str {
    unsafe {
        let len = name.len().min(31);
        DYNAMIC_APP_NAME_BUF[..len].copy_from_slice(&name.as_bytes()[..len]);
        DYNAMIC_APP_NAME_BUF[len] = 0;
        core::str::from_utf8_unchecked(&DYNAMIC_APP_NAME_BUF[..len])
    }
}

/// Parse a Declare 3.0 binary payload from the persistent upper slice of the shared arena (`[1024..5120]`).
#[inline(never)]
#[cfg(feature = "krx_declare")]
fn populate_declare_from_payload(
    walker: &mut kernel::proc::declare::DeclareWalker,
    payload: &'static [u8],
) {
    if payload.len() >= 4
        && payload[0] > 0
        && payload[0] <= 8
        && payload[1] <= 24
        && payload[2] <= 16
        && payload[3] <= 24
    {
        let n_cnt = payload[0] as usize;
        let i_cnt = payload[1] as usize;
        let a_cnt = payload[2] as usize;
        let l_cnt = payload[3] as usize;
        let min_bytes = 4 + n_cnt * 8 + i_cnt * 12 + a_cnt * 8;
        if payload.len() >= min_bytes {
            let mut off = 4usize;
            for _ in 0..n_cnt {
                let node: DeclareNode = unsafe {
                    core::ptr::read_unaligned(payload.as_ptr().add(off) as *const DeclareNode)
                };
                let _ = walker.nodes.push(node);
                off += 8;
            }
            for _ in 0..i_cnt {
                let item: DeclareItem = unsafe {
                    core::ptr::read_unaligned(payload.as_ptr().add(off) as *const DeclareItem)
                };
                let _ = walker.items.push(item);
                off += 12;
            }
            for _ in 0..a_cnt {
                let act: ActionRecord = unsafe {
                    core::ptr::read_unaligned(payload.as_ptr().add(off) as *const ActionRecord)
                };
                let _ = walker.actions.push(act);
                off += 8;
            }
            // Length-prefixed string pool (u8 len + UTF-8, KRX-ABI 2.5.0 Section 10.2)
            for _ in 0..l_cnt {
                if off >= payload.len() {
                    break;
                }
                let slen = payload[off] as usize;
                off += 1;
                if off + slen > payload.len() {
                    break;
                }
                if let Ok(s) = core::str::from_utf8(&payload[off..off + slen]) {
                    let _ = walker.labels.push(s);
                }
                off += slen;
            }
            return;
        }
    }

    // Fallback for raw DeclareItem slices: synthesize root DeclareNode
    let item_count = (payload.len() / core::mem::size_of::<DeclareItem>()).min(16);
    if item_count > 0 {
        for i in 0..item_count {
            let item: DeclareItem = unsafe {
                core::ptr::read_unaligned(payload.as_ptr().add(i * 12) as *const DeclareItem)
            };
            let _ = walker.items.push(item);
        }
        let _ = walker.nodes.push(DeclareNode {
            node_id: 0,
            layout: LAYOUT_MENU,
            item_count: item_count as u8,
            title_offset: 0,
            first_item_offset: 0,
        });
    }
}

/// Load and launch a verified KRX binary application package.
#[inline(never)]
pub fn launch_krx_package(ctx: &mut SysContext, path: &str) -> Result<usize, KrxLaunchError> {
    let mut header_buf = [0u8; 128];
    let header_read = ctx
        .storage
        .read_file_at(path, 0, &mut header_buf)
        .map_err(|e| match e {
            kernel::service::StorageError::NotFound => KrxLaunchError::FileNotFound,
            _ => KrxLaunchError::ReadFailed,
        })?;

    if header_read < KrxPreamble::SIZE {
        return Err(KrxLaunchError::TruncatedPackage);
    }

    let summary = KrxPreamble::summarize(&header_buf[..header_read])?;
    let preamble = summary.preamble;
    let header_len = preamble.header_len as usize;
    if header_len > header_read {
        return Err(KrxLaunchError::TruncatedPackage);
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
        let cap_msg = missing.first_cap().map_or("Missing Cap", |c| c.required_message());
        return Err(KrxLaunchError::CapabilityDenied(cap_msg));
    }

    // 3. Verify RAM and stack headroom requirements against Execution Section
    let mut exec_sec: Option<SectionExecution> = None;
    if let Some(eb) = summary.exec_body {
        if let Some(exec) = SectionExecution::parse(eb) {
            let avail = kernel::sys::SystemDiagnostics::ram_available_bytes();
            let margin = kernel::sys::SystemDiagnostics::stack_margin_bytes();
            let min_stack = (exec.min_stack_bytes as usize).min(128);
            if margin < min_stack.min(96) {
                return Err(KrxLaunchError::InsufficientStack);
            }
            if !exec.can_execute(avail, margin.min(128)) {
                return Err(KrxLaunchError::InsufficientRam);
            }
            exec_sec = Some(exec);
        }
    }

    #[cfg(feature = "krx_native")]
    let aux = super::native_runner::KrxAuxSections {
        cap_imports: summary.cap_imports,
        block_map: summary.block_map,
        leaf_map: summary.leaf_map,
        service_exports: summary.service_exports,
    };

    let app_name = store_dynamic_app_name(raw_name);

    // 5. Execution Class Dispatch
    let exec_class = preamble.execution_class().unwrap_or(KrxClass::Declare);
    let (vtable, instance) = match exec_class {
        #[cfg(feature = "krx_declare")]
        KrxClass::Declare => {
            let walker = crate::apps::runtime::get_global_walker();
            walker.reset(app_name);

            // Store Declare payload in [1024..] of the shared transient arena
            // (DeclareWalker occupies [0..988], so [1024..] persists for the app's lifetime!)
            let arena_tail = &mut crate::runtime::transient_arena_bytes_mut()[1024..];
            let read_total = ctx
                .storage
                .read_file_at(path, header_len, arena_tail)
                .map_err(|_| KrxLaunchError::ReadFailed)?;
            if read_total == 0 {
                return Err(KrxLaunchError::TruncatedPackage);
            }
            let payload: &'static [u8] = unsafe {
                core::slice::from_raw_parts(arena_tail.as_ptr(), read_total)
            };
            populate_declare_from_payload(walker, payload);

            (
                crate::apps::runtime::DECLARE_WALKER_VTABLE,
                walker as *mut _ as *mut (),
            )
        }
        #[cfg(not(feature = "krx_declare"))]
        KrxClass::Declare => {
            let _ = (ctx, path, preamble, app_name);
            return Err(KrxLaunchError::VmDisabled);
        }
        #[cfg(feature = "krx_vm")]
        KrxClass::VMachine => {
            let mut code_buf = [0u8; 256];
            let read_total = ctx
                .storage
                .read_file_at(path, header_len, &mut code_buf)
                .map_err(|_| KrxLaunchError::ReadFailed)?;
            let loaded = if read_total > 0 {
                let payload = &code_buf[..read_total];
                let mut words = [0u16; 128];
                let word_count = (payload.len() / 2).min(128);
                for i in 0..word_count {
                    words[i] = u16::from_le_bytes([payload[i * 2], payload[i * 2 + 1]]);
                }
                super::vm_runner::load_vm_program(app_name, &words[..word_count])
            } else {
                static DEFAULT_BYTECODE: [u16; 4] = [
                    0x0400, // MOV_IMM v0, 0
                    0x0400, // MOV_IMM v0, 0
                    0x4000, // HALT
                    0x0000, // NOP
                ];
                super::vm_runner::load_vm_program(app_name, &DEFAULT_BYTECODE)
            };
            if !loaded {
                return Err(KrxLaunchError::ExecutionFailed);
            }

            (super::vm_runner::VM_RUNNER_VTABLE, core::ptr::null_mut())
        }
        #[cfg(not(feature = "krx_vm"))]
        KrxClass::VMachine => {
            let _ = (ctx, path, preamble, app_name);
            return Err(KrxLaunchError::VmDisabled);
        }
        #[cfg(feature = "krx_native")]
        KrxClass::Native | KrxClass::Paged => {
            let exec_sec = exec_sec.ok_or(KrxLaunchError::MissingExecution)?;
            let arena_buf = super::native_runner::transient_arena_bytes_mut();
            let payload_read = ctx
                .storage
                .read_file_at(path, header_len, arena_buf)
                .map_err(|_| KrxLaunchError::ReadFailed)?;

            super::native_runner::load_native_from_storage(
                app_name,
                path,
                payload_read,
                &preamble,
                &exec_sec,
                &aux,
                required_caps,
            )?;

            (
                super::native_runner::NATIVE_RUNNER_VTABLE,
                core::ptr::null_mut(),
            )
        }
        #[cfg(not(feature = "krx_native"))]
        KrxClass::Native | KrxClass::Paged => {
            let _ = (ctx, path, preamble, app_name, required_caps);
            return Err(KrxLaunchError::ExecutionFailed);
        }
    };

    let dyn_id = AppRegistry::register_dynamic(DynamicAppRecord {
        metadata: AppMetadata {
            name: app_name,
            icon: &gfx::icons::ICON_16_DIAGNOSTICS,
            icon_8: &gfx::icons::ICON_APP_DIAG,
        },
        vtable,
        instance,
        required_capabilities: required_caps,
    });
    ctx.post_event(crate::apps::Shortcut::LaunchApp(dyn_id as u8).into());
    Ok(dyn_id)
}
