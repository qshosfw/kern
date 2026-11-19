//! Native & Demand-Paged ARM Thumb-1 Execution Engine (§6, §7, §11.2 / KRX-ABI 2.5.0).
//!
//! Unifies Class 0 (`Native` single-base PIC) and Class 1 (`Paged` demand-paged
//! fixed-window overlay cache with LR-thunking shadow return stack) in the
//! shared transient SRAM execution arena (`NATIVE_SLOT`), linked against the
//! modular capability VTables (`syscall`) and delta relocation engine (`apply_relocs`).

#[cfg(any(test, feature = "settings_debug"))]
use super::syscall::ACTIVE_CTX;
use super::syscall::NATIVE_API_TABLE;
use crate::services::SysContext;
use gfx::Canvas;
use kernel::app::{EventResult, ModalResult, StatusBarConfig};
use kernel::caps::CapabilitySet;
use kernel::event::{Event, KeyState};
use kernel::proc::abi::*;
use kernel::proc::krx::{KrxLaunchError, KrxProfile, KrxSummary, SectionExecution};
use kernel::proc::memory::{MemoryRegionDescriptor, RegionKind, region_flags};
#[cfg(feature = "krx_paged")]
use kernel::proc::paged::{FixedWindowCache, ReturnFrame, ShadowReturnStack};
use kernel::proc::paged::{PagedError, StructuredLeafManager};
use kernel::proc::reloc::apply_relocs;

pub const NATIVE_EXEC_BUFFER_WORDS: usize = crate::data::arena::TRANSIENT_ARENA_WORDS;
pub const PAGED_CACHE_MAX_BYTES: usize = 4096; // Up to 4 x 1024B fixed-window cache slots at arena base
pub const MICRO_DAEMON_ARENA_BYTES: usize = 2048; // 2.0 KB headroom reservation for background micro-daemons / applets
pub const TRANSIENT_SCRATCH_OFFSET: usize = crate::data::arena::TRANSIENT_SCRATCH_OFFSET;

#[cfg(feature = "krx_paged")]
#[repr(C)]
pub(crate) struct PagedWindowState {
    pub(crate) cache: FixedWindowCache<16, 4>,
    pub(crate) shadow_stack: ShadowReturnStack<8>,
}

/// Mutually exclusive overlay state: P1 uses `window` (fixed-window cache + shadow return stack),
/// P0 uses `leaf` (structured leaf manager). Overlapping them saves ~192 B of SRAM for runtime stack.
#[repr(C)]
pub(crate) union PagedOverlay {
    #[cfg(feature = "krx_paged")]
    pub(crate) window: core::mem::ManuallyDrop<PagedWindowState>,
    pub(crate) leaf: core::mem::ManuallyDrop<StructuredLeafManager<16>>,
}

#[repr(C, align(4))]
pub(crate) struct NativeExecSlot {
    pub(crate) pid: kernel::proc::Pid,
    pub(crate) is_loaded: bool,
    pub(crate) is_paged: bool,
    pub(crate) profile: KrxProfile,
    pub(crate) current_block: u8,
    pub(crate) current_slot: u8,
    pub(crate) code_size: u16,
    pub(crate) status_bar_has_custom: bool,
    pub(crate) status_bar_left_count: u8,
    pub(crate) status_bar_center_count: u8,
    pub(crate) pkg_path_len: u8,
    pub(crate) entry_point: usize,
    pub(crate) got_base: u32,
    pub(crate) caps: CapabilitySet,
    pub(crate) pkg_path: [u8; 31],
    pub(crate) overlay: PagedOverlay,
    pub(crate) status_bar_left_icons: [kernel::canvas::StatusGlyph; 4],
    pub(crate) status_bar_center_icons: [kernel::canvas::StatusGlyph; 4],
    pub(crate) status_bar: kernel::proc::abi::KrxStatusBar,
}

impl NativeExecSlot {
    #[inline(always)]
    pub(crate) fn pkg_path_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.pkg_path[..self.pkg_path_len as usize]) }
    }

    pub(crate) fn set_pkg_path(&mut self, path: &str) {
        let bytes = path.as_bytes();
        let n = bytes.len().min(self.pkg_path.len());
        self.pkg_path[..n].copy_from_slice(&bytes[..n]);
        self.pkg_path_len = n as u8;
    }

    #[inline(never)]
    pub(crate) fn query_region(&self, rk: RegionKind) -> Option<MemoryRegionDescriptor> {
        let load_base = arena_base_ptr() as u32;
        let max_arena = (NATIVE_EXEC_BUFFER_WORDS * 4) as u32;
        let is_p0 = self.profile.is_structured_paged();
        let overlay_size = 1024u32;
        let code_size = self.code_size as u32;
        match rk {
            RegionKind::ImmutableCode => Some(MemoryRegionDescriptor::new(
                RegionKind::ImmutableCode,
                region_flags::READ | region_flags::EXEC,
                load_base,
                code_size,
            )),
            RegionKind::OverlayWindow if is_p0 => Some(MemoryRegionDescriptor::new(
                RegionKind::OverlayWindow,
                region_flags::READ | region_flags::EXEC,
                load_base + (max_arena - overlay_size),
                overlay_size,
            )),
            RegionKind::VirtualRam => {
                let code_limit = if is_p0 {
                    max_arena - overlay_size
                } else {
                    max_arena
                };
                let limit_vma = load_base + code_limit;
                let ram_vma = if !self.is_paged && self.got_base > load_base {
                    self.got_base
                } else {
                    load_base + code_size
                };
                Some(MemoryRegionDescriptor::new(
                    RegionKind::VirtualRam,
                    region_flags::READ | region_flags::WRITE,
                    ram_vma,
                    limit_vma.saturating_sub(ram_vma),
                ))
            }
            _ => None,
        }
    }

    #[cfg(feature = "krx_paged")]
    #[inline(never)]
    fn window_mut(&mut self) -> Option<&mut PagedWindowState> {
        if self.is_loaded && self.is_paged && !self.profile.is_structured_paged() {
            Some(unsafe { &mut *self.overlay.window })
        } else {
            None
        }
    }

    #[inline(always)]
    fn leaf_mut(&mut self) -> Option<&mut StructuredLeafManager<16>> {
        if self.is_loaded && self.profile.is_structured_paged() {
            Some(unsafe { &mut *self.overlay.leaf })
        } else {
            None
        }
    }
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
pub(crate) fn slot_mut() -> &'static mut NativeExecSlot {
    unsafe { (*core::ptr::addr_of_mut!(NATIVE_SLOT)).assume_init_mut() }
}

#[inline(never)]
pub fn active_native_app_name() -> &'static str {
    if slot_ref().is_loaded {
        super::loader::dynamic_app_name()
    } else {
        ""
    }
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
        let path = slot_ref().pkg_path_str();
        let dst = core::slice::from_raw_parts_mut(dst_vma as *mut u8, len);
        #[cfg(not(test))]
        {
            read_file_region(crate::kern_fs(), path, flash_offset, dst)
        }
        #[cfg(test)]
        {
            if !ACTIVE_CTX.is_null() {
                read_file_region((*ACTIVE_CTX).storage, path, flash_offset, dst)
            } else {
                false
            }
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
#[cfg(feature = "krx_paged")]
#[inline(never)]
pub fn krx_page_fault(block_id: u8, func_offset: u16) -> usize {
    let slot = slot_mut();
    let entry_fallback = slot.entry_point;
    let Some(win) = slot.window_mut() else {
        return entry_fallback;
    };

    let Ok((slot_idx, is_hit, block_entry)) = win.cache.resolve_or_select_slot(block_id) else {
        return 0;
    };

    // Bounds check func_offset against declared block size
    if func_offset >= block_entry.size {
        return 0;
    }

    let slot_byte_off = win.cache.slot_byte_offset(slot_idx);
    let slot_vma = (arena_base_ptr() as usize) + slot_byte_off;

    if !is_hit {
        // Stream block from SPI Flash via active context storage
        let read_len = (block_entry.size as usize).min(win.cache.page_size as usize);
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
    }

    slot.current_block = block_id;
    slot.current_slot = slot_idx as u8;
    (slot_vma + (func_offset as usize)) | 1
}

/// Cross-block PLT call helper with LR-thunking shadow stack push (KRX-ABI 2.5.0 Section 7.2).
#[cfg(feature = "krx_paged")]
pub fn krx_call_paged(
    target_block_id: u8,
    target_offset: u16,
    return_offset: u16,
) -> Result<usize, PagedError> {
    let slot = slot_mut();
    let caller_block_id = slot.current_block;
    let caller_slot = slot.current_slot;
    let win = slot.window_mut().ok_or(PagedError::InvalidBlockId)?;
    win.shadow_stack.push(ReturnFrame {
        caller_block_id,
        caller_slot,
        return_offset,
    })?;
    let target_addr = krx_page_fault(target_block_id, target_offset);
    if target_addr == 0 {
        if let Some(w) = slot_mut().window_mut() {
            let _ = w.shadow_stack.pop();
        }
        return Err(PagedError::StorageReadFailed);
    }
    Ok(target_addr)
}

/// Normative OS Return-Thunk (`krx_return_thunk`, KRX-ABI 2.5.0 Section 7.2).
/// Restores caller's block into the fixed-window cache if it was evicted while the callee ran.
#[cfg(feature = "krx_paged")]
pub fn krx_return_thunk() -> Result<usize, PagedError> {
    let win = slot_mut().window_mut().ok_or(PagedError::InvalidBlockId)?;
    let frame = win.shadow_stack.pop()?;
    let resume_addr = krx_page_fault(frame.caller_block_id, frame.return_offset);
    if resume_addr == 0 {
        return Err(PagedError::StorageReadFailed);
    }
    Ok(resume_addr)
}

/// Invokes a P0 Structured Leaf Overlay from the resident root actor (§5.1 / KRX 3.2).
#[inline(never)]
pub fn krx_leaf_call(leaf_id: u8, arg: u32) -> u32 {
    let slot = slot_mut();
    let got_base = slot.got_base;
    let Some(leaf_mgr) = slot.leaf_mut() else {
        return 0xFFFF_FFFF;
    };

    let overlay_vma = leaf_mgr.overlay_vma;
    let window_size = leaf_mgr.window_size;

    let res = leaf_mgr.dispatch_leaf_trampoline(leaf_id, arg, 8, |leaf_entry, is_hit, ctx| {
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
    if !slot.is_loaded || slot.pkg_path_len == 0 || dst.is_empty() {
        return Err(());
    }
    if read_file_region(storage, slot.pkg_path_str(), offset as usize, dst) {
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

    let max_arena_bytes = NATIVE_EXEC_BUFFER_WORDS * 4;
    let h_len = preamble.header_len as usize;
    let load_base = arena_base_ptr() as u32;
    let profile = KrxProfile::detect(
        preamble,
        exec,
        aux.block_map.is_some(),
        aux.leaf_map.is_some(),
    );
    #[cfg(not(feature = "krx_paged"))]
    if profile == KrxProfile::P1TransparentPaged {
        let _ = h_len;
        return Err(KrxLaunchError::UnsupportedProfile);
    }
    let is_paged = matches!(
        profile,
        KrxProfile::P0StructuredPaged | KrxProfile::P1TransparentPaged
    );

    if payload_len == 0 || (!is_paged && payload_len < (exec.payload_size as usize)) {
        return Err(KrxLaunchError::TruncatedPackage);
    }
    if !is_paged && (exec.payload_size as usize) > max_arena_bytes {
        return Err(KrxLaunchError::PayloadTooLarge);
    }

    // Hardened check for N0 (NativeFixed): link_base must strictly match load_base!
    if profile == KrxProfile::N0NativeFixed && preamble.link_base != load_base {
        return Err(KrxLaunchError::RelocationFailed);
    }

    init_native_slot();
    let slot = slot_mut();
    slot.pid = pid;

    let buffer_bytes =
        unsafe { core::slice::from_raw_parts_mut(arena_base_ptr(), max_arena_bytes) };
    let load_delta = load_base.wrapping_sub(preamble.link_base);

    // 1. Configure Paged Fixed-Window Cache (P1) or Structured Leaf Manager (P0)
    slot.is_paged = is_paged;
    slot.set_pkg_path(path);
    let is_p0 = profile.is_structured_paged();
    let overlay_size = 1024u32;
    let overlay_vma = load_base + (max_arena_bytes as u32 - overlay_size);

    if is_p0 {
        let leaf_mgr = unsafe { &mut *slot.overlay.leaf };
        leaf_mgr.configure(overlay_vma, overlay_size as u16, aux.leaf_map);
    } else if is_paged {
        #[cfg(feature = "krx_paged")]
        {
            let win = unsafe { &mut *slot.overlay.window };
            win.shadow_stack.clear();
            let pay_size = if exec.payload_size > 0 {
                exec.payload_size
            } else {
                payload_len as u32
            };
            win.cache.configure(
                exec.page_size,
                exec.cache_blocks,
                h_len as u32,
                pay_size,
                aux.block_map,
            );
            // Mark block 0 as already resident in slot 0 from initial load
            if win.cache.block_count > 0 {
                win.cache.access_clock = 1;
                win.cache.slots[0] = kernel::proc::paged::CacheSlotMeta {
                    resident_block: Some(0),
                    pinned: win.cache.blocks[0].is_pinned(),
                    last_access_tick: 1,
                };
            }
        }
    }

    // 2b. Verify unrelocated payload CRC32 before patching words in SRAM
    if !is_paged
        && preamble.payload_crc32 != 0
        && crypto::crc::CrcEngine::crc32(&buffer_bytes[..payload_len.min(max_arena_bytes)])
            != preamble.payload_crc32
    {
        return Err(KrxLaunchError::CrcMismatch);
    }

    // 3. Apply escape-capable delta relocations v2 (Section 6.2)
    apply_package_relocs(buffer_bytes, payload_len, exec, load_delta)?;

    // 4. Configure package GOT base (all kernel ops route via single-entry `api->call`)
    slot.got_base = load_base.wrapping_add(exec.got_offset as u32);
    if (exec.got_offset & 3) != 0 || (exec.got_offset as usize) >= max_arena_bytes {
        return Err(KrxLaunchError::ImportBindingFailed);
    }

    // 4b. Record profile & code window size for `ProcMem` region queries
    slot.profile = profile;
    let code_limit = max_arena_bytes as u32 - if is_p0 { overlay_size } else { 0 };

    #[cfg(feature = "krx_paged")]
    let code_size = if is_paged && !is_p0 {
        let win = unsafe { &*slot.overlay.window };
        (win.cache.active_slots as u32 * win.cache.page_size as u32)
            .min(PAGED_CACHE_MAX_BYTES as u32)
    } else if exec.got_offset > 0 {
        (exec.got_offset as u32).min(code_limit)
    } else {
        (payload_len as u32).min(code_limit)
    };
    #[cfg(not(feature = "krx_paged"))]
    let code_size = if exec.got_offset > 0 {
        (exec.got_offset as u32).min(code_limit)
    } else {
        (payload_len as u32).min(code_limit)
    };
    slot.code_size = code_size as u16;

    if (exec.entry_point & !1) >= code_size {
        return Err(KrxLaunchError::InvalidEntryPoint);
    }
    let entry_thumb = (load_base.wrapping_add(exec.entry_point) | 1) as usize;

    // 6. Flush instruction pipeline before branching into SRAM arena
    flush_instruction_pipeline();

    slot.entry_point = entry_thumb;
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
        super::fault::raise_contract_fault(kernel::proc::fault::Exception::Stack, op as u8, 0);
        return kernel::proc::fault::KRX_FAULT_SENTINEL;
    }

    #[cfg(all(target_arch = "arm", not(test)))]
    {
        unsafe {
            super::fault::GUEST_GOT_BASE = got_base;
            super::fault::invoke_krx_trampoline(op, api_table as *const (), param, entry_point)
        }
    }
    #[cfg(not(all(target_arch = "arm", not(test))))]
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
unsafe fn invoke_guest(op: u32, ctx: *mut SysContext<'static, 'static>, param: *const ()) -> u32 {
    let slot = slot_ref();
    if !slot.is_loaded || slot.entry_point == 0 {
        return 0xFFFF_FFFF;
    }
    unsafe {
        #[cfg(any(test, feature = "settings_debug"))]
        {
            ACTIVE_CTX = ctx;
        }
        let res = invoke_krx_native(
            slot.entry_point,
            slot.got_base,
            op,
            &raw const NATIVE_API_TABLE,
            param,
        );
        #[cfg(any(test, feature = "settings_debug"))]
        {
            ACTIVE_CTX = core::ptr::null_mut();
        }
        if res == kernel::proc::fault::KRX_FAULT_SENTINEL || super::fault::has_pending_fault() {
            if !ctx.is_null() {
                super::fault::finish_fault_cleanup(&mut *ctx);
            }
            return 0xFFFF_FFFF;
        }
        res
    }
}

pub unsafe fn invoke_boot_start(param: usize) -> u32 {
    let slot = slot_ref();
    if !slot.is_loaded {
        return 0xFFFF_FFFF;
    }
    unsafe {
        invoke_krx_native(
            slot.entry_point,
            slot.got_base,
            KRX_OP_START,
            &raw const NATIVE_API_TABLE,
            param as *const (),
        )
    }
}

static mut PENDING_START_ARG: u32 = 0;

#[inline(always)]
pub fn set_pending_start_arg(arg: u32) {
    unsafe {
        PENDING_START_ARG = arg;
    }
}

#[inline(always)]
fn take_pending_start_arg() -> u32 {
    unsafe {
        let a = PENDING_START_ARG;
        PENDING_START_ARG = 0;
        a
    }
}

#[inline(always)]
pub fn active_pkg_matches(path: &str) -> bool {
    let slot = slot_ref();
    slot.is_loaded && slot.pkg_path_str() == path
}

#[inline(never)]
pub fn dispatch_action(ctx: &mut SysContext<'_, '_>, action_id: u16) -> EventResult {
    dispatch_krx_event(
        ctx as *mut _ as *mut _,
        kernel::proc::abi::KRX_EV_IPC,
        0,
        kernel::proc::abi::KRX_IPC_ACTION_FLAG,
        action_id as u32,
    )
}

pub unsafe fn native_on_start(ctx: &mut SysContext<'static, 'static>) {
    let arg = take_pending_start_arg();
    let res = unsafe { invoke_guest(KRX_OP_START, ctx as *mut _, arg as usize as *const ()) };
    if res == 0xFFFF_FFFF {
        slot_mut().is_loaded = false;
    }
}

pub unsafe fn native_on_event(ctx: &mut SysContext<'static, 'static>, ev: &Event) -> EventResult {
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
    let res = unsafe { invoke_guest(KRX_OP_EVENT, ctx, &krx_ev as *const _ as *const ()) };
    if res != 0 && res != 0xFFFF_FFFF {
        EventResult::Consumed
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn native_on_modal_result(
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
    *status_bar = StatusBarConfig::new(active_native_app_name());
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
        if (sb.flags & kernel::proc::abi::KRX_STATUS_CUSTOM_TITLE) != 0 {
            status_bar.title = c_str_to_str(&sb.title);
        }
        if visible && !show_divider && status_bar.title.is_none() && signal_level.is_none() {
            partial = true;
        }
        status_bar.visible = visible;
        status_bar.partial = partial;
        status_bar.show_divider = show_divider;
        status_bar.signal_level = signal_level;
        status_bar.left_badge = c_str_to_str(&sb.badge);
        status_bar.center_text = c_str_to_str(&sb.center);
        status_bar.left_icons = &slot.status_bar_left_icons[..slot.status_bar_left_count as usize];
        status_bar.center_icons =
            &slot.status_bar_center_icons[..slot.status_bar_center_count as usize];
    }
}

pub(crate) fn decode_status_glyph(
    sb: &kernel::proc::abi::KrxStatusBar,
    id: u8,
) -> Option<kernel::canvas::StatusGlyph> {
    if id == kernel::proc::abi::KRX_ICON_CUSTOM_0 {
        Some(kernel::canvas::StatusGlyph::Custom(sb.custom_icon_0))
    } else if id == kernel::proc::abi::KRX_ICON_CUSTOM_1 {
        Some(kernel::canvas::StatusGlyph::Custom(sb.custom_icon_1))
    } else {
        kernel::canvas::Icon::from_u8(id).map(kernel::canvas::StatusGlyph::Builtin)
    }
}

#[inline(never)]
fn decode_icon_list(
    sb: &kernel::proc::abi::KrxStatusBar,
    icons: &[u8],
    count: u8,
    out: &mut [kernel::canvas::StatusGlyph; 4],
) -> u8 {
    let mut n = 0;
    let limit = (count as usize).min(4);
    for &id in &icons[..limit] {
        if let Some(glyph) = decode_status_glyph(sb, id) {
            out[n] = glyph;
            n += 1;
        }
    }
    n as u8
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
            s,
            &s.left_icons,
            s.left_icon_count,
            &mut slot.status_bar_left_icons,
        );

        slot.status_bar_center_count = decode_icon_list(
            s,
            &s.center_icons,
            s.center_icon_count,
            &mut slot.status_bar_center_icons,
        );

        // Auto-register or clear status tray icon requested by app
        if s.tray_icon_id != 0 {
            super::syscall::ui::set_tray_icon(s.tray_icon_id);
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
    _canvas: &mut Canvas,
    ctx: &SysContext<'static, 'static>,
    status_bar: &mut StatusBarConfig<'_>,
) {
    unsafe {
        invoke_guest(KRX_OP_RENDER, ctx as *const _ as *mut _, core::ptr::null());
    }
    apply_slot_status_bar(slot_ref(), status_bar);
}

pub(crate) fn teardown_slot_and_hw(
    ctx: &mut SysContext<'_, '_>,
    pid: kernel::proc::Pid,
    persist_tray: bool,
) {
    let slot = slot_mut();
    crate::audio_capture_mut().stop_capture();
    if pid.0 != 0 {
        super::syscall::ipc_channel_mut().purge_pid(pid.0);
        if !persist_tray {
            crate::orchestrator::status_tray_mut().clear_icon(pid);
        }
        let client_id = kernel::service::radio::ClientId::for_krx_pid(pid.0);
        ctx.state.coord.release_client_lock(ctx.radio, client_id);
    }
    if ctx.state.coord.is_transmitting {
        ctx.state.coord.request_ptt_stop(ctx.radio);
    }
    slot.is_loaded = false;
    slot.entry_point = 0;
    slot.status_bar_has_custom = false;
    slot.status_bar_left_count = 0;
    slot.status_bar_center_count = 0;
    slot.pkg_path_len = 0;
}

pub unsafe fn native_on_exit(ctx: &mut SysContext<'static, 'static>) {
    let slot = slot_mut();
    let pid = slot.pid;
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe {
            invoke_guest(KRX_OP_EXIT, ctx as *mut _, core::ptr::null());
        }
        super::syscall::service_registry_mut().unbind_provider(pid);
    }
    let persist_tray = slot.status_bar_has_custom
        && (slot.status_bar.custom_flags & kernel::proc::abi::KRX_CUSTOM_TRAY_PERSIST) != 0;
    teardown_slot_and_hw(ctx, pid, persist_tray);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::proc::memory::MemoryMapLayout;

    #[test]
    fn query_region_matches_memory_map_layout_for_native_and_p0() {
        let load_base = arena_base_ptr() as u32;
        let max_arena = (NATIVE_EXEC_BUFFER_WORDS * 4) as u32;
        let overlay_size = 1024u32;
        let overlay_vma = load_base + max_arena - overlay_size;

        for (profile, is_paged, code_size, got_off) in [
            (KrxProfile::N1NativePic, false, 1200u16, 1200u32),
            (KrxProfile::N1NativePic, false, 900u16, 0u32),
            (KrxProfile::P0StructuredPaged, true, 2048u16, 2048u32),
            (KrxProfile::P1TransparentPaged, true, 4096u16, 0u32),
        ] {
            init_native_slot();
            let slot = slot_mut();
            slot.profile = profile;
            slot.is_paged = is_paged;
            slot.code_size = code_size;
            slot.got_base = load_base.wrapping_add(got_off);

            let is_p0 = profile.is_structured_paged();
            let code_limit = if is_p0 {
                max_arena - overlay_size
            } else {
                max_arena
            };
            let ram_vma = if !is_paged && got_off > 0 {
                slot.got_base
            } else {
                load_base + code_size as u32
            };
            let ram_size = (load_base + code_limit).saturating_sub(ram_vma);
            let mut layout = MemoryMapLayout::<4>::new();
            layout.init_regions(
                load_base,
                code_size as u32,
                overlay_vma,
                if is_p0 { overlay_size } else { 0 },
                ram_vma,
                ram_size,
            );

            for rk in [
                RegionKind::ImmutableCode,
                RegionKind::OverlayWindow,
                RegionKind::VirtualRam,
                RegionKind::PersistentState,
                RegionKind::IoBuffer,
            ] {
                assert_eq!(
                    slot.query_region(rk),
                    layout.find_region_by_kind(rk).copied(),
                    "mismatch for profile={profile:?} rk={rk:?}"
                );
            }
        }
    }
}
