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
use kernel::proc::krx::{
    KrxClass, KrxLaunchError, KrxPreamble, KrxProfile, LeafEntry, SectionExecution,
};
use kernel::proc::memory::MemoryMapLayout;
use kernel::proc::paged::{
    FixedWindowCache, PagedError, ReturnFrame, ShadowReturnStack, StructuredLeafManager,
};
use kernel::proc::reloc::apply_relocs;

pub const NATIVE_EXEC_BUFFER_WORDS: usize = 2048; // 8.0 KB (8192 bytes) shared transient execution arena
pub const PAGED_CACHE_MAX_BYTES: usize = 4096; // Up to 4 x 1024B fixed-window cache slots at arena base
pub const MICRO_DAEMON_ARENA_BYTES: usize = 2048; // 2.0 KB headroom reservation for background micro-daemons / applets
pub const TRANSIENT_SCRATCH_OFFSET: usize = 6144; // Scratch storage offset for asset chooser, staging, etc.

#[repr(C, align(4))]
pub(crate) struct NativeExecSlot {
    pub(crate) buffer: [u32; NATIVE_EXEC_BUFFER_WORDS],
    pub(crate) entry_point: usize,
    pub(crate) got_base: u32,
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
    let slot_vma = (slot.buffer.as_ptr() as usize) + slot_byte_off;

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

    let res = slot.leaf_mgr.dispatch_leaf_trampoline(leaf_id, arg, 8, |leaf_entry, is_hit, ctx| {
        if !is_hit {
            let read_len = (leaf_entry.size as usize).min(window_size as usize);
            let read_ok = unsafe {
                if !ACTIVE_CTX.is_null() {
                    let path = slot_ref().pkg_path.as_str();
                    let dst = core::slice::from_raw_parts_mut(
                        overlay_vma as *mut u8,
                        read_len,
                    );
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

/// Loads a Native (`KrxClass::Native`) or Demand-Paged (`KrxClass::Paged`) binary from flash
/// into the shared execution arena, applies delta relocations, and binds `CapImports` into the GOT.
#[inline(never)]
pub fn load_native_from_storage(
    name: &str,
    storage: &mut crate::services::SystemStorage,
    path: &str,
    preamble: &KrxPreamble,
    exec: &SectionExecution,
    cap_imports_body: Option<&[u8]>,
    block_map_body: Option<&[u8]>,
    leaf_map_body: Option<&[u8]>,
    caps: CapabilitySet,
) -> Result<(), KrxLaunchError> {
    let slot = slot_mut();
    slot.cache.reset();
    slot.shadow_stack.clear();
    slot.leaf_mgr.reset();
    slot.current_block = 0;
    slot.current_slot = 0;

    let buffer_bytes = unsafe {
        core::slice::from_raw_parts_mut(
            slot.buffer.as_mut_ptr() as *mut u8,
            NATIVE_EXEC_BUFFER_WORDS * 4,
        )
    };

    let total_read = storage
        .read_file(path, buffer_bytes)
        .map_err(|_| KrxLaunchError::ReadFailed)?;
    let h_len = preamble.header_len as usize;
    if total_read <= h_len {
        return Err(KrxLaunchError::ReadFailed);
    }

    let payload_len = total_read - h_len;
    if exec.payload_size > 0 && payload_len < (exec.payload_size as usize) {
        return Err(KrxLaunchError::TruncatedPackage);
    }
    if exec.payload_size > 0 && (exec.payload_size as usize) > (NATIVE_EXEC_BUFFER_WORDS * 4) {
        return Err(KrxLaunchError::PayloadTooLarge);
    }

    // Shift payload to the beginning of the aligned buffer
    buffer_bytes.copy_within(h_len..total_read, 0);

    let load_base = slot.buffer.as_ptr() as u32;
    let load_delta = load_base.wrapping_sub(preamble.link_base);

    let profile = KrxProfile::detect(
        preamble,
        exec,
        block_map_body.is_some(),
        leaf_map_body.is_some(),
    );

    // Hardened check for N0 (NativeFixed): link_base must strictly match load_base!
    if profile == KrxProfile::N0NativeFixed && preamble.link_base != load_base {
        return Err(KrxLaunchError::RelocationFailed);
    }

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
                return Err(KrxLaunchError::RelocationFailed);
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
            return Err(KrxLaunchError::ImportBindingFailed);
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

    // 4. Configure P0 Structured Leaf Manager if SECTION_LEAF_MAP is present
    let overlay_size = 1024u32;
    let overlay_vma = (load_base + (NATIVE_EXEC_BUFFER_WORDS as u32 * 4)).saturating_sub(overlay_size);
    slot.leaf_mgr.overlay_vma = overlay_vma;
    slot.leaf_mgr.window_size = overlay_size as u16;
    if let Some(leaf_body) = leaf_map_body {
        let count = leaf_body.len() / LeafEntry::SIZE;
        for i in 0..count {
            let offset = i * LeafEntry::SIZE;
            if let Some(entry) = LeafEntry::parse(&leaf_body[offset..]) {
                let _ = slot.leaf_mgr.register_leaf(entry);
            }
        }
    }

    // 4b. Configure normative memory regions for the detected execution profile
    slot.profile = profile;
    slot.mem_layout.reset();
    if profile.is_structured_paged() {
        let ram_vma = slot.got_base;
        let ram_size = overlay_vma.saturating_sub(ram_vma);
        slot.mem_layout = MemoryMapLayout::for_p0_structured(
            load_base,
            overlay_vma.saturating_sub(load_base),
            overlay_vma,
            overlay_size,
            ram_vma,
            ram_size,
        );
    } else {
        let code_size = (exec.got_offset as u32).min(NATIVE_EXEC_BUFFER_WORDS as u32 * 4);
        let ram_vma = slot.got_base;
        let ram_size = (load_base + (NATIVE_EXEC_BUFFER_WORDS as u32 * 4)).saturating_sub(ram_vma);
        slot.mem_layout = MemoryMapLayout::for_native_pic(
            load_base,
            code_size,
            ram_vma,
            ram_size,
        );
    }

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
    let slot_start = slot.buffer.as_ptr() as usize;
    let slot_end = slot_start + (NATIVE_EXEC_BUFFER_WORDS * 4);

    // 1. Validate Thumb-1 mode bit and code bounds
    let code_addr = entry_point & !1;
    if (entry_point & 1) == 0 || code_addr < slot_start || code_addr >= slot_end {
        return 0xFFFF_FFFF;
    }

    // 1b. Validate memory region execution permissions if layout is active
    if slot.mem_layout.region_count > 0
        && !slot.mem_layout.validate_access(code_addr as u32, 2, false, true)
    {
        return 0xFFFF_FFFF;
    }

    // 2. Validate GOT base alignment and memory bounds
    let got_addr = got_base as usize;
    if (got_addr & 3) != 0 || got_addr < slot_start || got_addr >= slot_end {
        return 0xFFFF_FFFF;
    }

    // 3. Ensure minimum stack headroom exists (prevent Cortex-M0+ stack exhaustion)
    if kernel::sys::SystemDiagnostics::stack_margin_bytes() < 256 {
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
        let entry: KrxNativeEntryFn = core::mem::transmute(entry_point);
        entry(op, api_table, param)
    }
}

pub unsafe fn native_on_start(_ptr: *mut (), ctx: &mut FirmwareContext<'static, 'static>) {
    let slot = slot_mut();
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            ACTIVE_CTX = ctx as *mut _;
            let res = invoke_krx_native(
                slot.entry_point,
                slot.got_base,
                KRX_OP_START,
                &raw const NATIVE_API_TABLE,
                core::ptr::null(),
            );
            ACTIVE_CTX = core::ptr::null_mut();
            if res == 0xFFFF_FFFF {
                slot.is_loaded = false;
            }
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
            let (kind_val, key_val, flags_val, payload_val) = match ev {
                Event::Key { code, state } => {
                    let s_val = match state {
                        KeyState::Pressed => 0u32,
                        KeyState::Released => 1u32,
                        KeyState::Held => 2u32,
                    };
                    (KRX_EV_KEY, *code as u8, 0u8, s_val)
                }
                Event::Encoder(dir) => (KRX_EV_ENCODER, 0u8, 0u8, *dir as u32),
                Event::Tick => (KRX_EV_TICK, 0u8, 0u8, 0u32),
                Event::Bus(bus_ev) => (
                    KRX_EV_BUS,
                    bus_ev.event_id,
                    (bus_ev.topic.0 & 0xFF) as u8,
                    bus_ev.data,
                ),
                Event::Usb(usb_ev) => (KRX_EV_USB, *usb_ev as u8, 0u8, 0u32),
                Event::Charging(chg) => (KRX_EV_POWER, *chg as u8, 0u8, 0u32),
                _ => (KRX_EV_NONE, 0u8, 0u8, 0u32),
            };
            if kind_val == KRX_EV_NONE {
                ACTIVE_CTX = core::ptr::null_mut();
                return EventResult::Ignored;
            }
            let krx_ev = KrxEvent {
                kind: kind_val,
                key: key_val,
                flags: flags_val,
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
            if res != 0 && res != 0xFFFF_FFFF {
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
