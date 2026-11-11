//! Native & Demand-Paged ARM Thumb-1 Execution Engine (§6, §7, §11.2 / KRX-ABI 2.5.0).
//!
//! Unifies Class 0 (`Native` single-base PIC) and Class 1 (`Paged` demand-paged
//! fixed-window overlay cache with LR-thunking shadow return stack) in the
//! shared transient SRAM execution arena (`NATIVE_SLOT`), linked against the
//! modular capability VTables (`syscall`) and delta relocation engine (`apply_relocs`).

use super::syscall::{ACTIVE_CANVAS, ACTIVE_CTX, NATIVE_API_TABLE, bind_cap_imports};
use crate::services::SysContext;
use gfx::Canvas;
use kernel::app::{AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::caps::CapabilitySet;
use kernel::event::{Event, KeyState};
use kernel::proc::abi::*;
use kernel::proc::krx::{KrxClass, KrxLaunchError, KrxPreamble, KrxProfile, SectionExecution};
use kernel::proc::memory::MemoryMapLayout;
use kernel::proc::paged::{
    FixedWindowCache, PagedError, ReturnFrame, ShadowReturnStack, StructuredLeafManager,
};
use kernel::proc::reloc::apply_relocs;

pub const NATIVE_EXEC_BUFFER_WORDS: usize = crate::data::arena::TRANSIENT_ARENA_WORDS;
pub const PAGED_CACHE_MAX_BYTES: usize = 4096; // Up to 4 x 1024B fixed-window cache slots at arena base
pub const MICRO_DAEMON_ARENA_BYTES: usize = 2048; // 2.0 KB headroom reservation for background micro-daemons / applets
pub const TRANSIENT_SCRATCH_OFFSET: usize = 2048; // Scratch storage offset for asset chooser, staging, etc.

#[repr(C, align(4))]
pub(crate) struct NativeExecSlot {
    pub(crate) entry_point: usize,
    pub(crate) got_base: u32,
    pub(crate) pid: kernel::proc::Pid,
    pub(crate) app_name: heapless::String<16>,
    pub(crate) pkg_path: heapless::String<32>,
    pub(crate) is_loaded: bool,
    pub(crate) is_paged: bool,
    pub(crate) profile: KrxProfile,
    pub(crate) mem_layout: MemoryMapLayout<4>,
    pub(crate) current_block: u8,
    pub(crate) current_slot: u8,
    pub(crate) caps: CapabilitySet,
    pub(crate) cache: FixedWindowCache<16, 4>,
    pub(crate) shadow_stack: ShadowReturnStack<8>,
    pub(crate) leaf_mgr: StructuredLeafManager<16>,
    pub(crate) status_bar: Option<kernel::proc::abi::KrxStatusBar>,
    pub(crate) status_bar_left_icons: [kernel::canvas::Icon; 4],
    pub(crate) status_bar_left_count: usize,
    pub(crate) status_bar_center_icons: [kernel::canvas::Icon; 4],
    pub(crate) status_bar_center_count: usize,
}

#[unsafe(link_section = ".uninit")]
static mut NATIVE_SLOT: core::mem::MaybeUninit<NativeExecSlot> = core::mem::MaybeUninit::uninit();

#[inline(always)]
pub fn arena_base_ptr() -> *mut u8 {
    crate::data::arena::arena_base_ptr()
}

#[inline(always)]
pub fn transient_arena_bytes_mut() -> &'static mut [u8; NATIVE_EXEC_BUFFER_WORDS * 4] {
    crate::data::arena::transient_arena_bytes_mut()
}

#[inline(always)]
pub(crate) fn slot_ref() -> &'static NativeExecSlot {
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
    let slot_vma = (arena_base_ptr() as usize) + slot_byte_off;

    if !is_hit {
        // Bounds check func_offset against declared block size
        if func_offset >= block_entry.size {
            return 0;
        }
        // Stream block from SPI Flash via active context storage
        let read_len = (block_entry.size as usize).min(slot.cache.page_size as usize);
        if slot_byte_off + read_len > PAGED_CACHE_MAX_BYTES {
            return 0;
        }
        let read_ok = unsafe {
            if !ACTIVE_CTX.is_null() {
                let path = slot.pkg_path.as_str();
                let dst = core::slice::from_raw_parts_mut(slot_vma as *mut u8, read_len);
                read_file_region(
                    (*ACTIVE_CTX).storage,
                    path,
                    block_entry.flash_offset as usize,
                    dst,
                )
            } else {
                false
            }
        };
        if !read_ok {
            return 0;
        }
        flush_instruction_pipeline();

        // Record page fault in active process ledger (Section 13.11)
        let active_id = crate::AppRegistry::count();
        if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut_by_app(active_id) {
            pcb.ledger.record_page_fault();
        }
    } else if func_offset >= block_entry.size {
        return 0;
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

/// Invokes a P0 Structured Leaf Overlay from the resident root actor (§5.1 / KRX 3.2).
pub fn krx_leaf_call(leaf_id: u8, arg: u32) -> u32 {
    let slot = slot_mut();
    if !slot.is_loaded {
        return 0xFFFF_FFFF;
    }

    let overlay_vma = slot.leaf_mgr.overlay_vma;
    let window_size = slot.leaf_mgr.window_size;
    let got_base = slot.got_base;

    let res = slot
        .leaf_mgr
        .dispatch_leaf_trampoline(leaf_id, arg, 8, |leaf_entry, is_hit, ctx| {
            if !is_hit {
                let read_len = (leaf_entry.size as usize).min(window_size as usize);
                let read_ok = unsafe {
                    if !ACTIVE_CTX.is_null() {
                        let path = slot_ref().pkg_path.as_str();
                        let dst = core::slice::from_raw_parts_mut(overlay_vma as *mut u8, read_len);
                        read_file_region(
                            (*ACTIVE_CTX).storage,
                            path,
                            leaf_entry.flash_offset as usize,
                            dst,
                        )
                    } else {
                        false
                    }
                };
                if !read_ok {
                    return Err(PagedError::StorageReadFailed);
                }
                flush_instruction_pipeline();
            }

            if leaf_entry.entry_offset >= leaf_entry.size {
                return Err(PagedError::InvalidLeafId);
            }

            let entry_addr = ((overlay_vma + leaf_entry.entry_offset as u32) | 1) as usize;
            let raw_res = unsafe {
                invoke_krx_native(
                    entry_addr,
                    got_base,
                    KRX_OP_START,
                    &raw const NATIVE_API_TABLE,
                    ctx as *mut _ as *const (),
                )
            };
            Ok(raw_res)
        });

    res.unwrap_or(0xFFFF_FFFF)
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

#[inline(never)]
fn apply_package_relocs(
    buffer_bytes: &[u8],
    payload_len: usize,
    exec: &SectionExecution,
    load_delta: u32,
) -> Result<(), KrxLaunchError> {
    if exec.reloc_count > 0 && exec.reloc_offset > 0 {
        let r_off = exec.reloc_offset as usize;
        if r_off < payload_len {
            let reloc_stream = &buffer_bytes[r_off..payload_len];
            let code_len = r_off;
            let res = unsafe {
                apply_relocs(
                    arena_base_ptr(),
                    code_len,
                    reloc_stream,
                    load_delta,
                    exec.reloc_count,
                )
            };
            if res.is_err() {
                return Err(KrxLaunchError::RelocationFailed);
            }
        }
    }
    Ok(())
}

#[derive(Default, Clone, Copy)]
pub struct KrxAuxSections<'a> {
    pub cap_imports: Option<&'a [u8]>,
    pub block_map: Option<&'a [u8]>,
    pub leaf_map: Option<&'a [u8]>,
    pub service_exports: Option<&'a [u8]>,
}

/// Loads a Native (`KrxClass::Native`) or Demand-Paged (`KrxClass::Paged`) binary from flash
/// into the shared execution arena, applies delta relocations, and binds `CapImports` into the GOT.
#[inline(never)]
pub fn load_native_from_storage(
    name: &str,
    path: &str,
    total_read: usize,
    preamble: &KrxPreamble,
    exec: &SectionExecution,
    aux: &KrxAuxSections,
    caps: CapabilitySet,
) -> Result<(), KrxLaunchError> {
    let slot = slot_mut();
    slot.cache.reset();
    slot.shadow_stack.clear();
    slot.leaf_mgr.reset();
    slot.current_block = 0;
    slot.current_slot = 0;
    slot.pid = kernel::proc::Pid(1);

    let buffer_bytes =
        unsafe { core::slice::from_raw_parts_mut(arena_base_ptr(), NATIVE_EXEC_BUFFER_WORDS * 4) };

    let h_len = preamble.header_len as usize;
    if total_read <= h_len {
        return Err(KrxLaunchError::TruncatedPackage);
    }

    let payload_len = total_read - h_len;
    if exec.payload_size > 0 && payload_len < (exec.payload_size as usize) {
        return Err(KrxLaunchError::TruncatedPackage);
    }
    if exec.payload_size > 0 && (exec.payload_size as usize) > (NATIVE_EXEC_BUFFER_WORDS * 4) {
        return Err(KrxLaunchError::PayloadTooLarge);
    }

    let load_base = arena_base_ptr() as u32;
    let load_delta = load_base.wrapping_sub(preamble.link_base);

    let profile = KrxProfile::detect(
        preamble,
        exec,
        aux.block_map.is_some(),
        aux.leaf_map.is_some(),
    );

    // Hardened check for N0 (NativeFixed): link_base must strictly match load_base!
    if profile == KrxProfile::N0NativeFixed && preamble.link_base != load_base {
        return Err(KrxLaunchError::RelocationFailed);
    }

    // 1. Configure Paged Fixed-Window Cache before shifting payload (aux.block_map borrows header buffer!)
    let is_paged = preamble.execution_class() == Some(KrxClass::Paged)
        || exec.page_size > 0
        || aux.block_map.is_some();
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
            aux.block_map,
        );
        // Mark block 0 as already resident in slot 0 from initial load
        let _ = slot.cache.resolve_or_select_slot(0);
    }

    // 2. Configure P0 Structured Leaf Manager before shifting payload (aux.leaf_map borrows header buffer!)
    let overlay_size = 1024u32;
    let overlay_vma =
        (load_base + (NATIVE_EXEC_BUFFER_WORDS as u32 * 4)).saturating_sub(overlay_size);
    slot.leaf_mgr
        .configure(overlay_vma, overlay_size as u16, aux.leaf_map);

    // 3. Preserve CapImports body before shifting payload
    let mut cap_buf = [0u8; 64];
    let cap_len = if let Some(cap_body) = aux.cap_imports {
        let len = cap_body.len().min(64);
        cap_buf[..len].copy_from_slice(&cap_body[..len]);
        len
    } else {
        0
    };

    // 4. Shift payload to the beginning of the aligned buffer without pulling in memmove
    let copy_len = total_read - h_len;
    for i in 0..copy_len {
        buffer_bytes[i] = buffer_bytes[h_len + i];
    }

    // 5. Apply escape-capable delta relocations v2 (Section 6.2)
    apply_package_relocs(buffer_bytes, payload_len, exec, load_delta)?;

    // 6. Bind CapImports (0x0003) into the package GOT (Section 6.3 & Chapter 8)
    slot.got_base = load_base.wrapping_add(exec.got_offset as u32);
    if cap_len > 0 {
        let got_word_off = (exec.got_offset as usize) / 4;
        let got_slice = if got_word_off < NATIVE_EXEC_BUFFER_WORDS {
            unsafe {
                core::slice::from_raw_parts_mut(
                    (arena_base_ptr() as *mut u32).add(got_word_off),
                    NATIVE_EXEC_BUFFER_WORDS - got_word_off,
                )
            }
        } else {
            &mut [][..]
        };
        if !bind_cap_imports(&cap_buf[..cap_len], got_slice, caps) {
            return Err(KrxLaunchError::ImportBindingFailed);
        }
    }

    // 4b. Configure normative memory regions for the detected execution profile
    slot.profile = profile;
    let is_p0 = profile.is_structured_paged();
    let max_arena_bytes = NATIVE_EXEC_BUFFER_WORDS as u32 * 4;
    let limit_vma = if is_p0 {
        overlay_vma
    } else {
        load_base + max_arena_bytes
    };
    let code_limit = limit_vma.saturating_sub(load_base);

    let code_size = if slot.is_paged && !is_p0 {
        (slot.cache.active_slots as u32 * slot.cache.page_size as u32)
            .min(PAGED_CACHE_MAX_BYTES as u32)
    } else if exec.got_offset > 0 {
        (exec.got_offset as u32).min(code_limit)
    } else {
        (payload_len as u32).min(code_limit)
    };

    let ram_vma = if !slot.is_paged && exec.got_offset > 0 {
        slot.got_base
    } else {
        load_base.saturating_add(code_size)
    };
    let ram_size = limit_vma.saturating_sub(ram_vma);
    let overlay = if is_p0 {
        Some((overlay_vma, overlay_size))
    } else {
        None
    };
    slot.mem_layout
        .init_regions(load_base, code_size, overlay, ram_vma, ram_size);

    // 5. Flush instruction pipeline before branching into SRAM arena
    flush_instruction_pipeline();

    let raw_entry = load_base.wrapping_add(exec.entry_point);
    let entry_thumb = (raw_entry | 1) as usize;
    let slot_start = load_base as usize;
    let slot_end = slot_start + (NATIVE_EXEC_BUFFER_WORDS * 4);
    if entry_thumb < slot_start || entry_thumb >= slot_end {
        return Err(KrxLaunchError::InvalidEntryPoint);
    }

    slot.entry_point = entry_thumb;
    slot.app_name.clear();
    let _ = slot.app_name.push_str(name);
    slot.caps = caps;
    slot.is_loaded = true;

    // 6. Register dynamic service exports if SECTION_SERVICE_EXPORTS is present (§8.1 / KRX-ABI 2.5.0)
    if let Some(exports_body) = aux.service_exports {
        for chunk in exports_body.chunks_exact(kernel::proc::ServiceExport::SIZE) {
            if let Some(export) = kernel::proc::ServiceExport::parse(chunk) {
                let vtable_ptr = (load_base + export.vtable_offset) as usize;
                super::syscall::service_registry_mut().register(
                    export.interface_id,
                    export.abi_version,
                    slot.pid,
                    vtable_ptr,
                    export.is_pinned(),
                );
            }
        }
    }

    Ok(())
}

/// Invokes a Native or Paged Thumb-1 entry point with the guest GOT base register (r9) bound.
/// Validates Thumb alignment, arena memory bounds, and stack margin before branching.
/// Returns `0xFFFF_FFFF` on pre-flight validation failure.
#[inline(never)]
pub unsafe fn invoke_krx_native(
    entry_point: usize,
    got_base: u32,
    op: u32,
    api_table: *const KrxApiTable,
    param: *const (),
) -> u32 {
    let slot = slot_ref();
    let slot_start = arena_base_ptr() as usize;
    let slot_end = slot_start + (NATIVE_EXEC_BUFFER_WORDS * 4);

    // 1. Validate Thumb-1 mode bit and code bounds
    let code_addr = entry_point & !1;
    if (entry_point & 1) == 0 || code_addr < slot_start || code_addr >= slot_end {
        return 0xFFFF_FFFF;
    }

    // 1b. Validate memory region execution permissions if layout is active
    if slot.mem_layout.region_count > 0
        && !slot
            .mem_layout
            .validate_access(code_addr as u32, 2, false, true)
    {
        return 0xFFFF_FFFF;
    }

    // 2. Validate GOT base alignment and memory bounds
    let got_addr = got_base as usize;
    if (got_addr & 3) != 0 || got_addr < slot_start || got_addr >= slot_end {
        return 0xFFFF_FFFF;
    }

    // 3. Ensure minimum stack headroom exists (prevent Cortex-M0+ stack exhaustion)
    if kernel::sys::SystemDiagnostics::stack_margin_bytes() < 96 {
        return 0xFFFF_FFFF;
    }

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
        let entry: KrxNativeEntryFn = unsafe {
            let ptr = (entry_point & !1) as *const u8;
            if *ptr == 0x00 && *ptr.add(1) == 0x4b && *ptr.add(2) == 0x18 && *ptr.add(3) == 0x47 {
                let target_fn_ptr = *(ptr.add(4) as *const usize);
                core::mem::transmute(target_fn_ptr)
            } else {
                core::mem::transmute(entry_point)
            }
        };
        unsafe { entry(op, api_table, param) }
    }
}

#[inline(never)]
unsafe fn invoke_guest(
    op: u32,
    ctx: *mut SysContext<'static, 'static>,
    canvas: *mut Canvas,
    param: *const (),
) -> u32 {
    let slot = slot_ref();
    if !slot.is_loaded || slot.entry_point == 0 {
        return 0xFFFF_FFFF;
    }
    unsafe {
        ACTIVE_CTX = ctx;
        ACTIVE_CANVAS = canvas;
        let res = invoke_krx_native(
            slot.entry_point,
            slot.got_base,
            op,
            &raw const NATIVE_API_TABLE,
            param,
        );
        ACTIVE_CANVAS = core::ptr::null_mut();
        ACTIVE_CTX = core::ptr::null_mut();
        res
    }
}

pub unsafe fn native_on_start(_ptr: *mut (), ctx: &mut SysContext<'static, 'static>) {
    let res = unsafe {
        invoke_guest(
            KRX_OP_START,
            ctx as *mut _,
            core::ptr::null_mut(),
            core::ptr::null(),
        )
    };
    if res == 0xFFFF_FFFF {
        slot_mut().is_loaded = false;
    }
}

pub unsafe fn native_on_event(
    _ptr: *mut (),
    ctx: &mut SysContext<'static, 'static>,
    ev: &Event,
) -> EventResult {
    let krx_ev = match ev {
        Event::Key { code, state } => {
            let s_val = match state {
                KeyState::Pressed => 0u32,
                KeyState::Released => 1u32,
                KeyState::Held => 2u32,
            };
            Some(KrxEvent {
                kind: KRX_EV_KEY,
                key: *code as u8,
                flags: 0,
                reserved: 0,
                payload: s_val,
            })
        }
        Event::Encoder(dir) => Some(KrxEvent {
            kind: KRX_EV_ENCODER,
            key: 0,
            flags: 0,
            reserved: 0,
            payload: *dir as u32,
        }),
        Event::Tick => Some(KrxEvent {
            kind: KRX_EV_TICK,
            key: 0,
            flags: 0,
            reserved: 0,
            payload: 0,
        }),
        Event::Bus(bus_ev) => Some(KrxEvent {
            kind: KRX_EV_BUS,
            key: bus_ev.event_id,
            flags: (bus_ev.topic.0 & 0xFF) as u8,
            reserved: 0,
            payload: bus_ev.data,
        }),
        Event::Usb(usb_ev) => Some(KrxEvent {
            kind: KRX_EV_USB,
            key: *usb_ev as u8,
            flags: 0,
            reserved: 0,
            payload: 0,
        }),
        Event::Charging(chg) => Some(KrxEvent {
            kind: KRX_EV_POWER,
            key: *chg as u8,
            flags: 0,
            reserved: 0,
            payload: 0,
        }),
        Event::Sys(sys_ev) => Some(KrxEvent {
            kind: kernel::proc::abi::KRX_EV_IPC,
            key: sys_ev.target,
            flags: sys_ev.command,
            reserved: 0,
            payload: sys_ev.arg as u32,
        }),
        _ => None,
    };
    if let Some(krx_ev) = krx_ev {
        let res = unsafe {
            invoke_guest(
                KRX_OP_EVENT,
                ctx as *mut _,
                core::ptr::null_mut(),
                &krx_ev as *const _ as *const (),
            )
        };
        if res != 0 && res != 0xFFFF_FFFF {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn native_on_modal_result(
    _ptr: *mut (),
    ctx: &mut SysContext<'static, 'static>,
    r: &ModalResult<'_>,
) -> EventResult {
    let (tag, flags, payload) = match r {
        ModalResult::Dismissed(tag) => (*tag, kernel::proc::abi::MODAL_RESULT_DISMISSED, 0),
        ModalResult::Confirmed(tag) => (*tag, kernel::proc::abi::MODAL_RESULT_CONFIRMED, 0),
        ModalResult::TextInputDone { tag, text } => {
            super::syscall::store_modal_text(text);
            (
                *tag,
                kernel::proc::abi::MODAL_RESULT_TEXT_DONE,
                text.len() as u32,
            )
        }
        ModalResult::SheetSelected { tag, index } => (
            *tag,
            kernel::proc::abi::MODAL_RESULT_SHEET_SELECTED,
            *index as u32,
        ),
        ModalResult::Canceled(tag) => (*tag, kernel::proc::abi::MODAL_RESULT_CANCELED, 0),
    };
    let krx_ev = KrxEvent {
        kind: kernel::proc::abi::KRX_EV_MODAL,
        key: tag,
        flags,
        reserved: 0,
        payload,
    };
    let res = unsafe {
        invoke_guest(
            KRX_OP_EVENT,
            ctx as *mut _,
            core::ptr::null_mut(),
            &krx_ev as *const _ as *const (),
        )
    };
    if res != 0 && res != 0xFFFF_FFFF {
        EventResult::Consumed
    } else {
        EventResult::Ignored
    }
}

fn c_str_to_str(buf: &[u8]) -> Option<&str> {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    if len == 0 {
        None
    } else {
        core::str::from_utf8(&buf[..len]).ok()
    }
}

fn apply_slot_status_bar<'a>(slot: &'a NativeExecSlot, status_bar: &mut StatusBarConfig<'a>) {
    if let Some(sb) = &slot.status_bar {
        let visible = (sb.flags & kernel::proc::abi::KRX_STATUS_VISIBLE) != 0;
        let show_divider = (sb.flags & kernel::proc::abi::KRX_STATUS_DIVIDER) != 0;
        let signal_level = if (sb.flags & kernel::proc::abi::KRX_STATUS_HAS_SIGNAL) != 0 {
            Some(sb.signal_level)
        } else {
            None
        };
        let title = if (sb.flags & kernel::proc::abi::KRX_STATUS_CUSTOM_TITLE) != 0 {
            Some(c_str_to_str(&sb.title).unwrap_or(""))
        } else {
            Some(slot.app_name.as_str())
        };
        let left_badge = c_str_to_str(&sb.badge);
        let center_text = c_str_to_str(&sb.center);
        let left_icons = &slot.status_bar_left_icons[..slot.status_bar_left_count];
        let center_icons = &slot.status_bar_center_icons[..slot.status_bar_center_count];

        *status_bar = StatusBarConfig {
            visible,
            title,
            center_text,
            signal_level,
            left_badge,
            left_icons,
            center_icons,
            show_divider,
        };
    } else {
        *status_bar = StatusBarConfig::new(slot.app_name.as_str());
    }
}

pub fn set_slot_status_bar(sb: *const kernel::proc::abi::KrxStatusBar) {
    let slot = slot_mut();
    if sb.is_null() {
        slot.status_bar = None;
        slot.status_bar_left_count = 0;
        slot.status_bar_center_count = 0;
    } else {
        let s = unsafe { *sb };
        slot.status_bar_left_count = 0;
        let l_cnt = (s.left_icon_count as usize).min(4);
        for &id in &s.left_icons[..l_cnt] {
            if let Some(ic) = kernel::canvas::Icon::from_u8(id) {
                slot.status_bar_left_icons[slot.status_bar_left_count] = ic;
                slot.status_bar_left_count += 1;
            }
        }
        slot.status_bar_center_count = 0;
        let c_cnt = (s.center_icon_count as usize).min(4);
        for &id in &s.center_icons[..c_cnt] {
            if let Some(ic) = kernel::canvas::Icon::from_u8(id) {
                slot.status_bar_center_icons[slot.status_bar_center_count] = ic;
                slot.status_bar_center_count += 1;
            }
        }
        slot.status_bar = Some(s);
    }
}

pub fn get_slot_status_bar(out_sb: *mut kernel::proc::abi::KrxStatusBar) {
    if !out_sb.is_null() {
        let slot = slot_ref();
        unsafe {
            *out_sb = slot
                .status_bar
                .unwrap_or_else(kernel::proc::abi::KrxStatusBar::empty);
        }
    }
}

pub unsafe fn native_on_render(
    _ptr: *const (),
    canvas: &mut Canvas,
    ctx: &SysContext<'static, 'static>,
    status_bar: &mut StatusBarConfig<'_>,
) {
    apply_slot_status_bar(slot_ref(), status_bar);
    unsafe {
        invoke_guest(
            KRX_OP_RENDER,
            ctx as *const _ as *mut _,
            canvas as *mut _,
            core::ptr::null(),
        );
    }
    apply_slot_status_bar(slot_ref(), status_bar);
}

pub unsafe fn native_on_exit(_ptr: *mut (), ctx: &mut SysContext<'static, 'static>) {
    let slot = slot_mut();
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            invoke_guest(
                KRX_OP_EXIT,
                ctx as *mut _,
                core::ptr::null_mut(),
                core::ptr::null(),
            );
        }
        // Unbind any service interfaces exported by this package
        super::syscall::service_registry_mut().unbind_provider(slot.pid);
        // Cleanly purge any pending unread IPC messages for this process
        super::syscall::ipc_channel_mut().purge_pid(slot.pid.0);
        // Ensure continuous audio capture is stopped on exit
        crate::audio_capture_mut().stop_capture();
        slot.is_loaded = false;
        slot.entry_point = 0;
    }
    let slot = slot_mut();
    crate::orchestrator::status_tray_mut().clear_icon(slot.pid);
    slot.status_bar = None;
    slot.status_bar_left_count = 0;
    slot.status_bar_center_count = 0;
    // Safety cleanup: ensure radio TX is disengaged if guest left PTT active
    if ctx.state.coord.is_transmitting {
        ctx.state.coord.request_ptt_stop(ctx.radio);
    }
}

pub static NATIVE_RUNNER_VTABLE: AppVTable<SysContext<'static, 'static>> = AppVTable {
    on_start: native_on_start,
    on_event: native_on_event,
    on_modal_result: native_on_modal_result,
    render: native_on_render,
    on_exit: native_on_exit,
};
