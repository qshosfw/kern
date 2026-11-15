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
use kernel::proc::krx::{KrxClass, KrxLaunchError, KrxProfile, KrxSummary, SectionExecution};
use kernel::proc::memory::MemoryMapLayout;
use kernel::proc::paged::{
    FixedWindowCache, PagedError, ReturnFrame, ShadowReturnStack, StructuredLeafManager,
};
use kernel::proc::reloc::apply_relocs;

pub const NATIVE_EXEC_BUFFER_WORDS: usize = crate::data::arena::TRANSIENT_ARENA_WORDS;
pub const PAGED_CACHE_MAX_BYTES: usize = 4096; // Up to 4 x 1024B fixed-window cache slots at arena base
pub const MICRO_DAEMON_ARENA_BYTES: usize = 2048; // 2.0 KB headroom reservation for background micro-daemons / applets
pub const TRANSIENT_SCRATCH_OFFSET: usize = crate::data::arena::TRANSIENT_SCRATCH_OFFSET;

#[repr(C, align(4))]
pub(crate) struct NativeExecSlot {
    pub(crate) entry_point: usize,
    pub(crate) got_base: u32,
    pub(crate) caps: CapabilitySet,
    pub(crate) pid: kernel::proc::Pid,
    pub(crate) is_loaded: bool,
    pub(crate) is_paged: bool,
    pub(crate) profile: KrxProfile,
    pub(crate) current_block: u8,
    pub(crate) current_slot: u8,
    pub(crate) status_bar_has_custom: bool,
    pub(crate) status_bar_left_count: usize,
    pub(crate) status_bar_center_count: usize,
    pub(crate) app_name: heapless::String<16>,
    pub(crate) pkg_path: heapless::String<32>,
    pub(crate) mem_layout: MemoryMapLayout<4>,
    pub(crate) cache: FixedWindowCache<16, 4>,
    pub(crate) shadow_stack: ShadowReturnStack<8>,
    pub(crate) leaf_mgr: StructuredLeafManager<16>,
    pub(crate) status_bar: kernel::proc::abi::KrxStatusBar,
    pub(crate) status_bar_left_icons: [kernel::canvas::StatusGlyph; 4],
    pub(crate) status_bar_center_icons: [kernel::canvas::StatusGlyph; 4],
}

pub fn init_native_slot() {
    unsafe {
        core::ptr::write_bytes(
            core::ptr::addr_of_mut!(NATIVE_SLOT) as *mut u8,
            0,
            core::mem::size_of::<NativeExecSlot>(),
        );
    }
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

#[inline(always)]
pub fn active_native_caps() -> CapabilitySet {
    let slot = slot_ref();
    if slot.is_loaded {
        slot.caps
    } else {
        CapabilitySet::ALL
    }
}

/// Mandatory ARMv6-M instruction/data barrier after code modification or page-in (KRX-ABI 2.5.0 Section 7.2 Step 4, F7).
#[inline(always)]
fn flush_instruction_pipeline() {
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

#[inline(never)]
fn read_active_pkg_region(flash_offset: usize, dst_vma: usize, len: usize) -> bool {
    let ok = unsafe {
        if !ACTIVE_CTX.is_null() {
            let path = slot_ref().pkg_path.as_str();
            let dst = core::slice::from_raw_parts_mut(dst_vma as *mut u8, len);
            read_file_region((*ACTIVE_CTX).storage, path, flash_offset, dst)
        } else {
            false
        }
    };
    if ok {
        flush_instruction_pipeline();
    }
    ok
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
        if !read_active_pkg_region(block_entry.flash_offset as usize, slot_vma, read_len) {
            return 0;
        }

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
                if !read_active_pkg_region(
                    leaf_entry.flash_offset as usize,
                    overlay_vma as usize,
                    read_len,
                ) {
                    return Err(PagedError::StorageReadFailed);
                }
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
    if load_delta == 0 {
        return Ok(());
    }
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

pub use kernel::proc::krx::KrxAuxSections;

/// Loads a Native (`KrxClass::Native`) or Demand-Paged (`KrxClass::Paged`) binary from flash
/// into the shared execution arena, applies delta relocations, and binds `CapImports` into the GOT.
#[inline(never)]
pub fn load_native_from_storage(
    name: &str,
    path: &str,
    payload_len: usize,
    summary: &KrxSummary<'_>,
    pid: kernel::proc::Pid,
) -> Result<(), KrxLaunchError> {
    let preamble = &summary.preamble;
    let exec = summary
        .exec
        .as_ref()
        .ok_or(KrxLaunchError::MissingExecution)?;
    let aux = &summary.aux;
    let caps = summary.caps;

    init_native_slot();
    let slot = slot_mut();
    slot.pid = pid;

    let buffer_bytes =
        unsafe { core::slice::from_raw_parts_mut(arena_base_ptr(), NATIVE_EXEC_BUFFER_WORDS * 4) };

    let h_len = preamble.header_len as usize;
    if payload_len == 0 || payload_len < (exec.payload_size as usize) {
        return Err(KrxLaunchError::TruncatedPackage);
    }
    if (exec.payload_size as usize) > (NATIVE_EXEC_BUFFER_WORDS * 4) {
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

    // 1. Configure Paged Fixed-Window Cache
    let is_paged = preamble.execution_class() == Some(KrxClass::Paged)
        || exec.page_size > 0
        || aux.block_map.is_some();
    slot.is_paged = is_paged;

    if is_paged {
        let _ = slot.pkg_path.push_str(path);
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
        if slot.cache.block_count > 0 {
            slot.cache.access_clock = 1;
            slot.cache.slots[0] = kernel::proc::paged::CacheSlotMeta {
                resident_block: Some(0),
                pinned: slot.cache.blocks[0].is_pinned(),
                last_access_tick: 1,
            };
        }
    }

    // 2. Configure P0 Structured Leaf Manager
    let overlay_size = 1024u32;
    let overlay_vma =
        (load_base + (NATIVE_EXEC_BUFFER_WORDS as u32 * 4)).saturating_sub(overlay_size);
    slot.leaf_mgr
        .configure(overlay_vma, overlay_size as u16, aux.leaf_map);

    // 3. Apply escape-capable delta relocations v2 (Section 6.2)
    apply_package_relocs(buffer_bytes, payload_len, exec, load_delta)?;

    // 4. Bind CapImports (0x0003) into the package GOT (Section 6.3 & Chapter 8)
    slot.got_base = load_base.wrapping_add(exec.got_offset as u32);
    let slot_start = load_base as usize;
    let slot_end = slot_start + (NATIVE_EXEC_BUFFER_WORDS * 4);
    let got_addr = slot.got_base as usize;
    if (got_addr & 3) != 0 || got_addr < slot_start || got_addr >= slot_end {
        return Err(KrxLaunchError::ImportBindingFailed);
    }
    if let Some(cap_body) = aux.cap_imports {
        let got_word_off = (exec.got_offset as usize) / 4;
        let got_slice = unsafe {
            core::slice::from_raw_parts_mut(
                (arena_base_ptr() as *mut u32).add(got_word_off),
                NATIVE_EXEC_BUFFER_WORDS - got_word_off,
            )
        };
        if !bind_cap_imports(cap_body, got_slice, caps) {
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

    let raw_entry = load_base.wrapping_add(exec.entry_point);
    let entry_thumb = (raw_entry | 1) as usize;
    let code_addr = raw_entry & !1;
    if code_addr < load_base || code_addr >= load_base.saturating_add(code_size) {
        return Err(KrxLaunchError::InvalidEntryPoint);
    }

    // 6. Flush instruction pipeline before branching into SRAM arena
    flush_instruction_pipeline();

    slot.entry_point = entry_thumb;
    let _ = slot.app_name.push_str(name);
    slot.caps = caps;
    slot.is_loaded = true;

    // 7. Register dynamic service exports if SECTION_SERVICE_EXPORTS is present (§8.1 / KRX-ABI 2.5.0)
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
    let slot_start = arena_base_ptr() as usize;
    let slot_end = slot_start + (NATIVE_EXEC_BUFFER_WORDS * 4);

    // 1. Fast Thumb-1 mode bit and code bounds guard (prevent wild branches)
    let code_addr = entry_point & !1;
    if (entry_point & 1) == 0 || code_addr < slot_start || code_addr >= slot_end {
        return 0xFFFF_FFFF;
    }

    // 2. Fast stack margin guard (prevent Cortex-M0+ stack exhaustion)
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

#[inline(always)]
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
    let (kind, key, flags, payload) = match ev {
        Event::Key { code, state } => {
            let s_val = match state {
                KeyState::Pressed => 0u32,
                KeyState::Released => 1u32,
                KeyState::Held => 2u32,
            };
            (KRX_EV_KEY, *code as u8, 0, s_val)
        }
        Event::Encoder(dir) => (KRX_EV_ENCODER, 0, 0, *dir as u32),
        Event::Tick => (KRX_EV_TICK, 0, 0, 0),
        Event::Bus(bus_ev) => (
            KRX_EV_BUS,
            bus_ev.event_id,
            (bus_ev.topic.0 & 0xFF) as u8,
            bus_ev.data,
        ),
        Event::Usb(usb_ev) => (KRX_EV_USB, *usb_ev as u8, 0, 0),
        Event::Charging(chg) => (KRX_EV_POWER, *chg as u8, 0, 0),
        Event::Sys(sys_ev) => (
            kernel::proc::abi::KRX_EV_IPC,
            sys_ev.target,
            sys_ev.command,
            sys_ev.arg as u32,
        ),
        _ => return EventResult::Ignored,
    };
    dispatch_krx_event(ctx as *mut _, kind, key, flags, payload)
}

#[inline(never)]
fn dispatch_krx_event(
    ctx: *mut SysContext<'static, 'static>,
    kind: u8,
    key: u8,
    flags: u8,
    payload: u32,
) -> EventResult {
    let krx_ev = KrxEvent {
        kind,
        key,
        flags,
        reserved: 0,
        payload,
    };
    let res = unsafe {
        invoke_guest(
            KRX_OP_EVENT,
            ctx,
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
    dispatch_krx_event(
        ctx as *mut _,
        kernel::proc::abi::KRX_EV_MODAL,
        tag,
        flags,
        payload,
    )
}

fn c_str_to_str(buf: &[u8]) -> Option<&str> {
    let mut len = 0;
    while len < buf.len() && buf[len] != 0 {
        if buf[len] < 0x20 || buf[len] > 0x7E {
            return None;
        }
        len += 1;
    }
    if len == 0 {
        None
    } else {
        Some(unsafe { core::str::from_utf8_unchecked(&buf[..len]) })
    }
}

fn apply_slot_status_bar<'a>(slot: &'a NativeExecSlot, status_bar: &mut StatusBarConfig<'a>) {
    if slot.status_bar_has_custom {
        let sb = &slot.status_bar;
        let visible = (sb.flags & kernel::proc::abi::KRX_STATUS_VISIBLE) != 0;
        let mut partial = (sb.flags & kernel::proc::abi::KRX_STATUS_PARTIAL) != 0;
        let show_divider = (sb.flags & kernel::proc::abi::KRX_STATUS_DIVIDER) != 0;
        let signal_level = if (sb.flags & kernel::proc::abi::KRX_STATUS_HAS_SIGNAL) != 0 {
            Some(sb.signal_level)
        } else {
            None
        };
        let title = if (sb.flags & kernel::proc::abi::KRX_STATUS_CUSTOM_TITLE) != 0 {
            c_str_to_str(&sb.title)
        } else {
            Some(slot.app_name.as_str())
        };
        if visible && !show_divider && title.is_none() && signal_level.is_none() {
            partial = true;
        }
        let left_badge = c_str_to_str(&sb.badge);
        let center_text = c_str_to_str(&sb.center);
        let left_icons = &slot.status_bar_left_icons[..slot.status_bar_left_count];
        let center_icons = &slot.status_bar_center_icons[..slot.status_bar_center_count];

        *status_bar = StatusBarConfig {
            visible,
            partial,
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

pub(crate) fn decode_status_glyph(
    id: u8,
    c0: &[u8; 5],
    c1: &[u8; 5],
) -> Option<kernel::canvas::StatusGlyph> {
    if id == kernel::proc::abi::KRX_ICON_CUSTOM_0 {
        Some(kernel::canvas::StatusGlyph::Custom(*c0))
    } else if id == kernel::proc::abi::KRX_ICON_CUSTOM_1 {
        Some(kernel::canvas::StatusGlyph::Custom(*c1))
    } else {
        kernel::canvas::Icon::from_u8(id).map(kernel::canvas::StatusGlyph::Builtin)
    }
}

#[inline(never)]
fn decode_icon_list(
    icons: &[u8],
    count: u8,
    c0: &[u8; 5],
    c1: &[u8; 5],
    out: &mut [kernel::canvas::StatusGlyph; 4],
) -> usize {
    let mut n = 0;
    let limit = (count as usize).min(4);
    for &id in &icons[..limit] {
        if let Some(glyph) = decode_status_glyph(id, c0, c1) {
            out[n] = glyph;
            n += 1;
        }
    }
    n
}

pub fn set_slot_status_bar(sb: *const kernel::proc::abi::KrxStatusBar) {
    let slot = slot_mut();
    if sb.is_null() {
        slot.status_bar_has_custom = false;
        slot.status_bar_left_count = 0;
        slot.status_bar_center_count = 0;
    } else {
        unsafe {
            core::ptr::copy_nonoverlapping(
                sb as *const u32,
                &mut slot.status_bar as *mut kernel::proc::abi::KrxStatusBar as *mut u32,
                16,
            );
        }
        slot.status_bar_has_custom = true;
        let s = &slot.status_bar;

        slot.status_bar_left_count = decode_icon_list(
            &s.left_icons,
            s.left_icon_count,
            &s.custom_icon_0,
            &s.custom_icon_1,
            &mut slot.status_bar_left_icons,
        );

        slot.status_bar_center_count = decode_icon_list(
            &s.center_icons,
            s.center_icon_count,
            &s.custom_icon_0,
            &s.custom_icon_1,
            &mut slot.status_bar_center_icons,
        );

        // Auto-register or clear status tray icon requested by app
        if s.tray_icon_id != 0 {
            unsafe {
                super::syscall::ui::api_ui_set_tray_icon(s.tray_icon_id);
            }
        }
    }
}

pub fn get_slot_status_bar(out_sb: *mut kernel::proc::abi::KrxStatusBar) {
    if !out_sb.is_null() {
        let slot = slot_ref();
        let src = if slot.status_bar_has_custom {
            &slot.status_bar
        } else {
            &kernel::proc::abi::KrxStatusBar::EMPTY
        };
        unsafe {
            core::ptr::copy_nonoverlapping(
                src as *const kernel::proc::abi::KrxStatusBar as *const u32,
                out_sb as *mut u32,
                16,
            );
        }
    }
}

pub unsafe fn native_on_render(
    _ptr: *const (),
    canvas: &mut Canvas,
    ctx: &SysContext<'static, 'static>,
    status_bar: &mut StatusBarConfig<'_>,
) {
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
    let persist_tray = slot.status_bar_has_custom
        && (slot.status_bar.custom_flags & kernel::proc::abi::KRX_CUSTOM_TRAY_PERSIST) != 0;
    if !persist_tray {
        crate::orchestrator::status_tray_mut().clear_icon(slot.pid);
    }
    slot.status_bar_has_custom = false;
    slot.status_bar_left_count = 0;
    slot.status_bar_center_count = 0;
    slot.app_name.clear();
    slot.pkg_path.clear();
    // Safety cleanup: ensure radio TX and arbitrator VFO locks are released on exit
    if ctx.state.coord.is_transmitting {
        ctx.state.coord.request_ptt_stop(ctx.radio);
    }
    if slot.pid.0 != 0 {
        let client_id = kernel::service::radio::ClientId::for_krx_pid(slot.pid.0 as u8);
        ctx.state.coord.release_client_lock(ctx.radio, client_id);
    }
}

pub static NATIVE_RUNNER_VTABLE: AppVTable<SysContext<'static, 'static>> = AppVTable {
    on_start: native_on_start,
    on_event: native_on_event,
    on_modal_result: native_on_modal_result,
    render: native_on_render,
    on_exit: native_on_exit,
};
