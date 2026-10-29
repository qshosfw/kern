//! Native & Demand-Paged ARM Thumb-1 Execution Engine (§6, §7, §11.2 / KRX-ABI 2.5.0).
//!
//! Unifies Class 0 (`Native` single-base PIC) and Class 1 (`Paged` demand-paged
//! fixed-window overlay cache with LR-thunking shadow return stack) in the
//! shared transient SRAM execution arena (`NATIVE_SLOT`), linked against the
//! modular capability VTables (`abi_bridge`) and delta relocation engine (`apply_relocs`).

use super::abi_bridge::{ACTIVE_CANVAS, ACTIVE_CTX, NATIVE_API_TABLE, bind_cap_imports};
use crate::services::FirmwareContext;
use gfx::Canvas;
use kernel::app::{AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::caps::CapabilitySet;
use kernel::event::{Event, KeyState};
use kernel::proc::abi::*;
use kernel::proc::krx::{KrxClass, KrxPreamble, SectionExecution};
use kernel::proc::paged::{FixedWindowCache, PagedError, ReturnFrame, ShadowReturnStack};
use kernel::proc::reloc::apply_relocs;

pub const NATIVE_EXEC_BUFFER_WORDS: usize = 1536; // 6.0 KB (6144 bytes) shared transient execution arena
pub const PAGED_CACHE_MAX_BYTES: usize = 4096; // Up to 4 x 1024B fixed-window cache slots at arena base
pub const MICRO_DAEMON_ARENA_BYTES: usize = 1024; // Headroom reservation for micro-daemons / applets

#[repr(C, align(4))]
pub(crate) struct NativeExecSlot {
    pub(crate) buffer: [u32; NATIVE_EXEC_BUFFER_WORDS],
    pub(crate) entry_point: usize,
    pub(crate) got_base: u32,
    pub(crate) app_name: heapless::String<16>,
    pub(crate) pkg_path: heapless::String<32>,
    pub(crate) is_loaded: bool,
    pub(crate) is_paged: bool,
    pub(crate) current_block: u8,
    pub(crate) current_slot: u8,
    pub(crate) caps: CapabilitySet,
    pub(crate) cache: FixedWindowCache<16, 4>,
    pub(crate) shadow_stack: ShadowReturnStack<8>,
}

#[unsafe(link_section = ".uninit")]
static mut NATIVE_SLOT: core::mem::MaybeUninit<NativeExecSlot> = core::mem::MaybeUninit::uninit();

#[inline(always)]
pub fn arena_base_ptr() -> *mut u8 {
    core::ptr::addr_of_mut!(NATIVE_SLOT) as *mut u8
}

#[inline(always)]
pub fn transient_arena_bytes_mut() -> &'static mut [u8; NATIVE_EXEC_BUFFER_WORDS * 4] {
    unsafe { &mut *(arena_base_ptr() as *mut [u8; NATIVE_EXEC_BUFFER_WORDS * 4]) }
}

#[inline(always)]
fn slot_ref() -> &'static NativeExecSlot {
    unsafe { (*core::ptr::addr_of!(NATIVE_SLOT)).assume_init_ref() }
}

#[inline(always)]
fn slot_mut() -> &'static mut NativeExecSlot {
    unsafe { (*core::ptr::addr_of_mut!(NATIVE_SLOT)).assume_init_mut() }
}

#[inline(always)]
pub fn active_native_app_name() -> &'static str {
    slot_ref().app_name.as_str()
}

/// Mandatory ARMv6-M instruction/data barrier after code modification or page-in (KRX-ABI 2.5.0 Section 7.2 Step 4, F7).
#[inline(always)]
fn flush_instruction_pipeline() {
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// Normative Demand-Paged Block Fault Handler (`krx_page_fault`, KRX-ABI 2.5.0 Section 7.2).
///
/// Resolves `block_id` in the fixed-window cache (`slot.cache`). On a cache miss,
/// streams the block from `slot.pkg_path` in KernFS into the selected cache slot,
/// executes `DSB; ISB`, tallies the page fault in the process ledger, and returns
/// the executable Thumb-1 target address (`(slot_vma + func_offset) | 1`).
#[inline(never)]
pub fn krx_page_fault(block_id: u8, func_offset: u16) -> usize {
    let slot = slot_mut();
    if !slot.is_loaded || !slot.is_paged {
        return slot.entry_point;
    }

    let Ok((slot_idx, is_hit, block_entry)) = slot.cache.resolve_or_select_slot(block_id) else {
        return 0;
    };

    let slot_byte_off = slot.cache.slot_byte_offset(slot_idx);
    let slot_vma = (slot.buffer.as_ptr() as usize) + slot_byte_off;

    if !is_hit {
        // Stream block from SPI Flash via active context storage
        let read_len = (block_entry.size as usize).min(slot.cache.page_size as usize);
        if slot_byte_off + read_len > PAGED_CACHE_MAX_BYTES {
            return 0;
        }
        unsafe {
            if !ACTIVE_CTX.is_null() {
                let path = slot.pkg_path.as_str();
                let dst = core::slice::from_raw_parts_mut(slot_vma as *mut u8, read_len);
                // Read block from file offset into cache slot
                let _ = read_file_region(
                    (*ACTIVE_CTX).storage,
                    path,
                    block_entry.flash_offset as usize,
                    dst,
                );
            }
        }
        flush_instruction_pipeline();

        // Record page fault in active process ledger (Section 13.11)
        let active_id = crate::AppRegistry::count();
        if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut_by_app(active_id) {
            pcb.ledger.record_page_fault();
        }
    }

    slot.current_block = block_id;
    slot.current_slot = slot_idx as u8;
    (slot_vma + (func_offset as usize)) | 1
}

/// Cross-block PLT call helper with LR-thunking shadow stack push (KRX-ABI 2.5.0 Section 7.2).
pub fn krx_call_paged(
    target_block_id: u8,
    target_offset: u16,
    return_offset: u16,
) -> Result<usize, PagedError> {
    let slot = slot_mut();
    slot.shadow_stack.push(ReturnFrame {
        caller_block_id: slot.current_block,
        caller_slot: slot.current_slot,
        return_offset,
    })?;
    let target_addr = krx_page_fault(target_block_id, target_offset);
    if target_addr == 0 {
        let _ = slot.shadow_stack.pop();
        return Err(PagedError::StorageReadFailed);
    }
    Ok(target_addr)
}

/// Normative OS Return-Thunk (`krx_return_thunk`, KRX-ABI 2.5.0 Section 7.2).
/// Restores caller's block into the fixed-window cache if it was evicted while the callee ran.
pub fn krx_return_thunk() -> Result<usize, PagedError> {
    let frame = slot_mut().shadow_stack.pop()?;
    let resume_addr = krx_page_fault(frame.caller_block_id, frame.return_offset);
    if resume_addr == 0 {
        return Err(PagedError::StorageReadFailed);
    }
    Ok(resume_addr)
}

/// Read an asset slice from the currently loaded KRX package on KernFS.
pub fn read_active_pkg_asset(
    storage: &mut crate::services::SystemStorage,
    offset: u32,
    dst: &mut [u8],
) -> Result<usize, ()> {
    let slot = slot_ref();
    if !slot.is_loaded || slot.pkg_path.is_empty() || dst.is_empty() {
        return Err(());
    }
    if read_file_region(storage, slot.pkg_path.as_str(), offset as usize, dst) {
        Ok(dst.len())
    } else {
        Err(())
    }
}

fn read_file_region(
    storage: &mut crate::services::SystemStorage,
    path: &str,
    offset: usize,
    dst: &mut [u8],
) -> bool {
    storage.read_file_at(path, offset, dst).is_ok()
}

/// Loads a Native (`KrxClass::Native`) or Demand-Paged (`KrxClass::Paged`) binary from flash
/// into the shared execution arena, applies delta relocations, and binds `CapImports` into the GOT.
pub fn load_native_from_storage(
    name: &str,
    storage: &mut crate::services::SystemStorage,
    path: &str,
    preamble: &KrxPreamble,
    exec: &SectionExecution,
    cap_imports_body: Option<&[u8]>,
    block_map_body: Option<&[u8]>,
    caps: CapabilitySet,
) -> bool {
    let slot = slot_mut();
    slot.cache.reset();
    slot.shadow_stack.clear();
    slot.current_block = 0;
    slot.current_slot = 0;

    let buffer_bytes = unsafe {
        core::slice::from_raw_parts_mut(
            slot.buffer.as_mut_ptr() as *mut u8,
            NATIVE_EXEC_BUFFER_WORDS * 4,
        )
    };

    let total_read = storage.read_file(path, buffer_bytes).unwrap_or(0);
    let h_len = preamble.header_len as usize;
    if total_read <= h_len {
        return false;
    }

    let payload_len = total_read - h_len;
    // Shift payload to the beginning of the aligned buffer
    buffer_bytes.copy_within(h_len..total_read, 0);

    let load_base = slot.buffer.as_ptr() as u32;
    let load_delta = load_base.wrapping_sub(preamble.link_base);

    // 1. Apply escape-capable delta relocations v2 (Section 6.2)
    if exec.reloc_count > 0 && exec.reloc_offset > 0 {
        let r_off = exec.reloc_offset as usize;
        if r_off < payload_len {
            let reloc_stream = &buffer_bytes[r_off..payload_len];
            let code_len = r_off;
            let res = unsafe {
                apply_relocs(
                    slot.buffer.as_mut_ptr() as *mut u8,
                    code_len,
                    reloc_stream,
                    load_delta,
                    exec.reloc_count,
                )
            };
            if res.is_err() {
                return false;
            }
        }
    }

    // 2. Bind CapImports (0x0003) into the package GOT (Section 6.3 & Chapter 8)
    slot.got_base = load_base.wrapping_add(exec.got_offset as u32);
    if let Some(cap_body) = cap_imports_body {
        let got_word_off = (exec.got_offset as usize) / 4;
        let got_slice = if got_word_off < NATIVE_EXEC_BUFFER_WORDS {
            &mut slot.buffer[got_word_off..]
        } else {
            &mut [][..]
        };
        if !bind_cap_imports(cap_body, got_slice, caps) {
            return false;
        }
    }

    // 3. Configure Paged Fixed-Window Cache if KrxClass::Paged or page_size > 0
    let is_paged = preamble.execution_class() == Some(KrxClass::Paged)
        || exec.page_size > 0
        || block_map_body.is_some();
    slot.is_paged = is_paged;
    slot.pkg_path.clear();
    let _ = slot.pkg_path.push_str(path);

    if is_paged {
        let pay_size = if exec.payload_size > 0 {
            exec.payload_size
        } else {
            payload_len as u32
        };
        slot.cache.configure(
            exec.page_size,
            exec.cache_blocks,
            h_len as u32,
            pay_size,
            block_map_body,
        );
        // Mark block 0 as already resident in slot 0 from initial load
        let _ = slot.cache.resolve_or_select_slot(0);
    }

    // 4. Flush instruction pipeline before branching into SRAM arena
    flush_instruction_pipeline();

    slot.entry_point = ((load_base + exec.entry_point) | 1) as usize;
    slot.app_name.clear();
    let _ = slot.app_name.push_str(name);
    slot.caps = caps;
    slot.is_loaded = true;
    true
}

/// Invokes a Native or Paged Thumb-1 entry point with the guest GOT base register (r9) bound.
#[inline(never)]
pub unsafe fn invoke_krx_native(
    entry_point: usize,
    got_base: u32,
    op: u32,
    api_table: *const KrxApiTable,
    param: *const (),
) -> u32 {
    #[cfg(target_arch = "arm")]
    {
        let res: u32;
        unsafe {
            core::arch::asm!(
                "mov r4, r9",       // save host GOT base register
                "mov r9, {got}",    // load guest app's GOT base into r9 (Static Base)
                "blx {entry}",      // call native entry point
                "mov r9, r4",       // restore host GOT base register
                got = in(reg) got_base,
                entry = in(reg) entry_point,
                in("r0") op,
                in("r1") api_table,
                in("r2") param,
                lateout("r0") res,
                out("r4") _,
                clobber_abi("C"),
            );
        }
        res
    }
    #[cfg(not(target_arch = "arm"))]
    {
        let _ = got_base;
        let entry: KrxNativeEntryFn = core::mem::transmute(entry_point);
        entry(op, api_table, param)
    }
}

pub unsafe fn native_on_start(_ptr: *mut (), ctx: &mut FirmwareContext<'static, 'static>) {
    let slot = slot_ref();
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *mut _;
            let _ = invoke_krx_native(
                slot.entry_point,
                slot.got_base,
                KRX_OP_START,
                &raw const NATIVE_API_TABLE,
                core::ptr::null(),
            );
            ACTIVE_CTX = core::ptr::null_mut();
        }
    }
}

pub unsafe fn native_on_event(
    _ptr: *mut (),
    ctx: &mut FirmwareContext<'static, 'static>,
    ev: &Event,
) -> EventResult {
    let slot = slot_ref();
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *mut _;
            let (key_val, payload_val) = match ev {
                Event::Key { code, state } => {
                    let s_val = match state {
                        KeyState::Pressed => 0u32,
                        KeyState::Released => 1u32,
                        KeyState::Held => 2u32,
                    };
                    (*code as u8, s_val)
                }
                Event::Encoder(dir) => (0u8, *dir as u32),
                _ => (0u8, 0u32),
            };
            let krx_ev = KrxEvent {
                kind: match ev {
                    Event::Key { .. } => 1,
                    Event::Encoder(_) => 2,
                    Event::Tick => 3,
                    _ => 0,
                },
                key: key_val,
                flags: 0,
                reserved: 0,
                payload: payload_val,
            };
            let res = invoke_krx_native(
                slot.entry_point,
                slot.got_base,
                KRX_OP_EVENT,
                &raw const NATIVE_API_TABLE,
                &krx_ev as *const _ as *const (),
            );
            ACTIVE_CTX = core::ptr::null_mut();
            if res != 0 {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn native_on_modal_result(
    _ptr: *mut (),
    _ctx: &mut FirmwareContext<'static, 'static>,
    _r: &ModalResult<'_>,
) -> EventResult {
    EventResult::Ignored
}

pub unsafe fn native_on_render(
    _ptr: *const (),
    canvas: &mut Canvas,
    ctx: &FirmwareContext<'static, 'static>,
    status_bar: &mut StatusBarConfig<'_>,
) {
    let slot = slot_ref();
    *status_bar = StatusBarConfig::new(slot.app_name.as_str());
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *const _ as *mut _;
            ACTIVE_CANVAS = canvas as *mut _;
            let _ = invoke_krx_native(
                slot.entry_point,
                slot.got_base,
                KRX_OP_RENDER,
                &raw const NATIVE_API_TABLE,
                core::ptr::null(),
            );
            ACTIVE_CANVAS = core::ptr::null_mut();
            ACTIVE_CTX = core::ptr::null_mut();
        }
    }
}

pub unsafe fn native_on_exit(_ptr: *mut (), ctx: &mut FirmwareContext<'static, 'static>) {
    let slot = slot_ref();
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *mut _;
            let _ = invoke_krx_native(
                slot.entry_point,
                slot.got_base,
                KRX_OP_EXIT,
                &raw const NATIVE_API_TABLE,
                core::ptr::null(),
            );
            ACTIVE_CTX = core::ptr::null_mut();
        }
    }
}

pub static NATIVE_RUNNER_VTABLE: AppVTable<FirmwareContext<'static, 'static>> = AppVTable {
    on_start: native_on_start,
    on_event: native_on_event,
    on_modal_result: native_on_modal_result,
    render: native_on_render,
    on_exit: native_on_exit,
};
