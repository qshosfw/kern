//! Native & Demand-Paged ARM Thumb-1 Execution Engine (§6, §7, §11.2 / KRX-ABI 2.5.0).
//!
//! Unifies Class 0 (`Native` single-base PIC) and Class 1 (`Paged` demand-paged
//! fixed-window overlay cache with LR-thunking shadow return stack) in the
//! shared transient SRAM execution arena (`NATIVE_SLOT`), linked against the
//! modular capability VTables (`syscall`) and delta relocation engine (`apply_relocs`).

use super::syscall::NATIVE_API_TABLE;
use ::abi::CapabilitySet;
use gfx::Canvas;
use kernel::app::{EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyState};
use kernel::proc::abi::*;
use kernel::proc::memory::{MemoryRegionDescriptor, RegionKind, region_flags};
use kernel::runtime::paged::StreamingLeafState;
#[cfg(feature = "krx_paged")]
use kernel::runtime::paged::{
    BLOCK_FLAG_HINT_NEXT, BLOCK_FLAG_LZSS, PageReturnFrame, ShadowReturnStack, StreamedPageDesc,
    StreamedWindowCache,
};
use kernel::runtime::reloc::apply_relocs;
use kernel::runtime::{KrxLaunchError, KrxProfile, KrxSummary, SectionExecution};

pub const NATIVE_EXEC_BUFFER_WORDS: usize = crate::data::arena::TRANSIENT_ARENA_WORDS;
pub const PAGED_CACHE_MAX_BYTES: usize = 4096; // Up to 4 x 1024B fixed-window cache slots at arena base
pub const MICRO_DAEMON_ARENA_BYTES: usize = crate::data::arena::ZONE_A_MAX_BYTES;
pub const TRANSIENT_SCRATCH_OFFSET: usize = crate::data::arena::TRANSIENT_SCRATCH_OFFSET;

/// Resolve per-package P0 overlay window size from `SectionExecution::page_size` (defaults to 1024 B).
#[inline(always)]
pub(crate) fn p0_overlay_size(exec: &SectionExecution) -> usize {
    let req = exec.page_size as usize;
    if (1024..=4096).contains(&req) && (req & 3) == 0 {
        req
    } else {
        1024
    }
}

#[cfg(feature = "krx_paged")]
#[repr(C)]
pub(crate) struct PagedWindowState {
    pub(crate) cache: StreamedWindowCache<8>,
    pub(crate) shadow_stack: ShadowReturnStack<16, PageReturnFrame>,
}

/// Mutually exclusive overlay state: P1 v2 uses `window` (unlimited-size streamed cache + 16-deep shadow stack),
/// P0 uses `leaf` (24 B streaming flash-backed leaf state, supporting up to 255 leaves).
#[repr(C)]
pub(crate) union PagedOverlay {
    #[cfg(feature = "krx_paged")]
    pub(crate) window: core::mem::ManuallyDrop<PagedWindowState>,
    pub(crate) leaf: core::mem::ManuallyDrop<StreamingLeafState>,
}

#[repr(C, align(4))]
pub(crate) struct NativeExecSlot {
    pub(crate) pid: kernel::proc::Pid,
    pub(crate) is_loaded: bool,
    pub(crate) is_paged: bool,
    pub(crate) profile: KrxProfile,
    pub(crate) current_page: u16,
    pub(crate) current_slot: u8,
    pub(crate) status_bar_has_custom: bool,
    pub(crate) code_size: u16,
    pub(crate) pkg_ino: u16,
    pub(crate) entry_point: usize,
    pub(crate) got_base: u32,
    pub(crate) pending_start_arg: u32,
    pub(crate) caps: CapabilitySet,
    pub(crate) pkg_path: utils::format::InlineStr<31>,
    pub(crate) overlay: PagedOverlay,
    pub(crate) status_bar: kernel::proc::abi::KrxStatusBar,
}

impl NativeExecSlot {
    #[inline]
    pub(crate) fn pkg_path_str(&self) -> &str {
        let len = self.pkg_path.len as usize;
        if len > self.pkg_path.buf.len() {
            return "";
        }
        let bytes = unsafe { core::slice::from_raw_parts(self.pkg_path.buf.as_ptr(), len) };
        utils::format::ascii_str(bytes).unwrap_or("")
    }

    #[inline(always)]
    pub(crate) fn set_pkg_path(&mut self, path: &str) {
        self.pkg_path.set(path);
    }

    pub(crate) fn query_region(&self, rk: RegionKind) -> Option<MemoryRegionDescriptor> {
        let zone_a_off = crate::data::arena::zone_a_used_bytes();
        let load_base = crate::data::arena::zone_b_base_ptr() as u32;
        let max_arena = crate::data::arena::TRANSIENT_ARENA_BYTES.saturating_sub(zone_a_off) as u32;
        let is_p0 = self.profile.is_structured_paged();
        let overlay_size = if is_p0 {
            let ws = unsafe { (*self.overlay.leaf).window_size } as u32;
            if ws >= 1024 { ws } else { 1024 }
        } else {
            1024u32
        };
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
    pub(crate) fn window_mut(&mut self) -> Option<&mut PagedWindowState> {
        if self.is_loaded && self.is_paged && !self.profile.is_structured_paged() {
            Some(unsafe { &mut *self.overlay.window })
        } else {
            None
        }
    }

    #[inline(always)]
    fn leaf_mut(&mut self) -> Option<&mut StreamingLeafState> {
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

#[inline(always)]
pub fn is_leaf_dispatching() -> bool {
    let slot = slot_ref();
    slot.is_loaded
        && slot.profile.is_structured_paged()
        && unsafe { (*slot.overlay.leaf).is_dispatching }
}

pub fn invalidate_leaf_cache() {
    if let Some(leaf) = slot_mut().leaf_mut() {
        if !leaf.is_dispatching {
            leaf.active_leaf = None;
        }
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
    slot_mut()
}

pub(crate) fn slot_mut() -> &'static mut NativeExecSlot {
    unsafe { (*core::ptr::addr_of_mut!(NATIVE_SLOT)).assume_init_mut() }
}

pub fn active_native_app_name() -> &'static str {
    if slot_ref().is_loaded {
        super::loader::dynamic_app_name()
    } else {
        ""
    }
}

pub fn active_native_caps() -> CapabilitySet {
    if let Some(bg) = super::background::BgManager::active_bg_slot() {
        return bg.caps;
    }
    let slot = slot_ref();
    if slot.is_loaded {
        slot.caps
    } else {
        CapabilitySet::ALL
    }
}

/// Shared instruction-pipeline barrier for Cortex-M0+ SRAM code modification (`DSB; ISB`).
#[inline(always)]
pub(crate) fn flush_instruction_pipeline() {
    #[cfg(target_arch = "arm")]
    {
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
}

fn read_slot_pkg_bytes(
    storage: &mut crate::services::SystemStorage,
    offset: usize,
    dst: &mut [u8],
) -> bool {
    let slot = slot_ref();
    if slot.pkg_ino >= 2 {
        storage.read_ino_at(slot.pkg_ino, offset, dst) == Ok(dst.len())
    } else {
        let path_copy = slot.pkg_path;
        storage.read_file_at(path_copy.as_str(), offset, dst) == Ok(dst.len())
    }
}

fn read_active_pkg_region(flash_offset: usize, dst_vma: usize, len: usize) -> bool {
    let ok = unsafe {
        let dst = core::slice::from_raw_parts_mut(dst_vma as *mut u8, len);
        read_slot_pkg_bytes(crate::kern_fs(), flash_offset, dst)
    };
    if ok {
        flush_instruction_pipeline();
    }
    ok
}

#[cfg(feature = "krx_paged")]
fn read_block_entry_from_pkg(entry_off: u32) -> Option<kernel::runtime::BlockEntry> {
    let mut raw = [0u8; kernel::runtime::BlockEntry::SIZE];
    if read_active_pkg_region(entry_off as usize, raw.as_mut_ptr() as usize, raw.len()) {
        kernel::runtime::BlockEntry::parse(&raw)
    } else {
        None
    }
}

#[cfg(feature = "krx_paged")]
fn stream_paged_way(slot_byte_off: usize, page_size: usize, desc: &StreamedPageDesc) -> bool {
    let raw_len = (desc.raw_size as usize).min(page_size);
    if raw_len == 0 || slot_byte_off + page_size > crate::data::arena::TRANSIENT_ARENA_BYTES {
        return false;
    }
    let slot_vma = crate::data::arena::zone_b_base_ptr() as usize + slot_byte_off;
    if (desc.flags & BLOCK_FLAG_LZSS) != 0 {
        let comp_off = page_size - raw_len;
        if !read_active_pkg_region(desc.flash_offset as usize, slot_vma + comp_off, raw_len) {
            return false;
        }
        let slot_slice = unsafe { core::slice::from_raw_parts_mut(slot_vma as *mut u8, page_size) };
        let ok = utils::codec::lzss_decompress_inplace(slot_slice, comp_off, page_size).is_ok();
        if ok {
            flush_instruction_pipeline();
        }
        ok
    } else {
        read_active_pkg_region(desc.flash_offset as usize, slot_vma, raw_len)
    }
}

#[cfg(feature = "krx_paged")]
fn fault_in_way(page_id: u16, is_demand: bool) -> Option<(usize, StreamedPageDesc)> {
    let slot = slot_mut();
    let slot_pid = slot.pid;
    let win = slot.window_mut()?;
    let page_size = win.cache.page_size() as usize;
    let (s, desc) = win
        .cache
        .alloc_way_for_miss(page_id, is_demand, read_block_entry_from_pkg)
        .ok()?;
    let slot_byte_off = win.cache.slot_byte_offset(s);
    if !stream_paged_way(slot_byte_off, page_size, &desc) {
        if let Some(w) = slot_mut().window_mut() {
            w.cache.invalidate_way(s);
        }
        return None;
    }
    if is_demand && let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut(slot_pid) {
        pcb.health.record_page_fault();
    }
    Some((s, desc))
}

/// Unlimited-Size `P1 v2` Demand-Paged Fault & Cache Hit Handler (supports up to `768` pages = `384 KiB`).
///
/// Cache hits (`>97%` of cross-page calls) resolve in zero Flash reads via `probe_hit`.
/// Cache misses stream directly from `PY25Q16` via `ExtentStreamer` + `DMA1_Ch3` (`SPI2` @ 24 MHz),
/// optionally expanding `BLOCK_FLAG_LZSS` pages in-place in zero extra RAM.
#[cfg(feature = "krx_paged")]
#[inline(never)]
pub fn krx_page_fault(page_id: u16, func_offset: u16) -> usize {
    let slot = slot_mut();
    let entry_fallback = slot.entry_point;
    let Some(win) = slot.window_mut() else {
        return entry_fallback;
    };

    let (slot_idx, loaded_size) = if let Some((s, sz, _)) = win.cache.probe_hit(page_id) {
        (s, sz)
    } else {
        let Some((s, desc)) = fault_in_way(page_id, true) else {
            return 0;
        };
        if (desc.flags & BLOCK_FLAG_HINT_NEXT) != 0
            && let Some(next_id) = page_id.checked_add(1)
        {
            let _ = fault_in_way(next_id, false);
        }
        (s, desc.exec_size)
    };

    if func_offset >= loaded_size {
        return 0;
    }

    let slot = slot_mut();
    let slot_byte_off = unsafe { (*slot.overlay.window).cache.slot_byte_offset(slot_idx) };
    slot.current_page = page_id;
    slot.current_slot = slot_idx as u8;
    let slot_vma = crate::data::arena::zone_b_base_ptr() as usize + slot_byte_off;
    (slot_vma + (func_offset as usize)) | 1
}

/// Cross-page `P1 v2` call helper with 16-deep LR-thunking shadow stack push (`u16` `target_page_id`).
#[cfg(feature = "krx_paged")]
#[inline(never)]
pub fn krx_call_paged(target_page_id: u16, target_offset: u16, return_offset: u16) -> usize {
    let slot = slot_mut();
    let caller_page_id = slot.current_page;
    let Some(win) = slot.window_mut() else {
        return 0;
    };
    if win
        .shadow_stack
        .push(PageReturnFrame {
            caller_page_id,
            return_offset,
        })
        .is_err()
    {
        return 0;
    }
    let target_addr = krx_page_fault(target_page_id, target_offset);
    if target_addr == 0
        && let Some(w) = slot_mut().window_mut()
    {
        let _ = w.shadow_stack.pop();
    }
    target_addr
}

/// Resolve or invoke a `Vfp32` Virtual Function Pointer across resident or evicted pages (`P1 v2`).
#[cfg(feature = "krx_paged")]
pub fn krx_call_vfp(raw_vfp: u32, return_offset: u16) -> usize {
    let vfp = ::abi::Vfp32(raw_vfp);
    if !vfp.is_virtual() {
        return (raw_vfp as usize) | 1;
    }
    if return_offset == 0xFFFF {
        krx_page_fault(vfp.page_id(), vfp.offset())
    } else {
        krx_call_paged(vfp.page_id(), vfp.offset(), return_offset)
    }
}

/// Normative OS Return-Thunk (`krx_return_thunk`, `P1 v2`).
/// Restores caller's page into the streamed virtual code cache if evicted while the callee ran.
#[cfg(feature = "krx_paged")]
pub fn krx_return_thunk() -> usize {
    let Some(win) = slot_mut().window_mut() else {
        return 0;
    };
    let Ok(frame) = win.shadow_stack.pop() else {
        return 0;
    };
    krx_page_fault(frame.caller_page_id, frame.return_offset)
}

/// Loads and validates a P0 leaf into `overlay_vma` in its own stack frame so `LeafEntry`,
/// `raw` buffer, and `KernFS` I/O frames are completely popped before the leaf executes.
#[inline(never)]
fn prepare_p0_leaf(cur_id: u8) -> Option<usize> {
    let leaf_mgr = slot_mut().leaf_mut()?;
    let overlay_vma = leaf_mgr.overlay_vma;
    let window_size = leaf_mgr.window_size as usize;
    let Ok((leaf_entry, is_hit)) = leaf_mgr.begin_leaf_call(cur_id, |entry_off| {
        let mut raw = [0u8; kernel::runtime::LeafEntry::SIZE];
        if read_active_pkg_region(entry_off as usize, raw.as_mut_ptr() as usize, raw.len()) {
            kernel::runtime::LeafEntry::parse(&raw)
        } else {
            None
        }
    }) else {
        return None;
    };
    if !is_hit
        && !read_active_pkg_region(
            leaf_entry.flash_offset as usize,
            overlay_vma as usize,
            (leaf_entry.size as usize).min(window_size),
        )
    {
        leaf_mgr.active_leaf = None;
        leaf_mgr.is_dispatching = false;
        return None;
    }
    if leaf_entry.entry_offset >= leaf_entry.size {
        leaf_mgr.active_leaf = None;
        leaf_mgr.is_dispatching = false;
        return None;
    }
    Some(((overlay_vma + leaf_entry.entry_offset as u32) | 1) as usize)
}

/// Invokes a P0 Structured Leaf Overlay from the resident root actor (§5.1 / KRX 3.2).
/// Direct non-closure hop loop keeps the M0+ stack frame minimal across leaf -> syscall -> KernFS chains.
#[inline(never)]
pub fn krx_leaf_call(leaf_id: u8, arg: u32) -> u32 {
    let got_base = slot_ref().got_base;
    let mut cur_id = leaf_id;
    let mut cur_arg = arg;

    for _ in 0..8u8 {
        let Some(entry_addr) = prepare_p0_leaf(cur_id) else {
            return 0xFFFF_FFFF;
        };

        let mut ctx = kernel::runtime::LeafContext::new(cur_id, cur_arg);
        let raw_res = unsafe {
            invoke_krx_native(
                entry_addr,
                got_base,
                KRX_OP_START,
                &raw const NATIVE_API_TABLE,
                &raw mut ctx as *const (),
            )
        };
        if let Some(leaf_mgr) = slot_mut().leaf_mut() {
            leaf_mgr.is_dispatching = false;
        }

        let cont = ctx.continuation;
        if cont.is_call_leaf() {
            cur_id = cont.target_or_code;
            cur_arg = cont.payload;
        } else if cont.is_error() {
            return 0xFFFF_FFFF;
        } else if cont.payload != 0 {
            return cont.payload;
        } else {
            return raw_res;
        }
    }

    0xFFFF_FFFF
}

/// Read an asset slice from the currently loaded KRX package on KernFS via `ExtentStreamer`.
pub fn read_active_pkg_asset(
    storage: &mut crate::services::SystemStorage,
    offset: u32,
    dst: &mut [u8],
) -> Result<usize, ()> {
    let slot = slot_ref();
    if !slot.is_loaded || slot.pkg_path.is_empty() || dst.is_empty() {
        return Err(());
    }
    if read_slot_pkg_bytes(storage, offset as usize, dst) {
        Ok(dst.len())
    } else {
        Err(())
    }
}

pub(crate) fn apply_package_relocs_at(
    base_ptr: *mut u8,
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
            let res = unsafe {
                apply_relocs(
                    base_ptr,
                    r_off,
                    &buffer_bytes[r_off..payload_len],
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

pub(crate) fn register_service_exports_at(
    load_base: u32,
    pid: kernel::proc::Pid,
    exports_body: Option<&[u8]>,
) {
    if let Some(body) = exports_body {
        for chunk in body.chunks_exact(kernel::runtime::ServiceExport::SIZE) {
            if let Some(export) = kernel::runtime::ServiceExport::parse(chunk) {
                super::syscall::service_registry_mut().register(
                    export.interface_id,
                    export.abi_version,
                    pid,
                    (load_base + export.vtable_offset) as usize,
                    export.is_pinned(),
                );
            }
        }
    }
}

pub use kernel::runtime::KrxAuxSections;

#[inline(never)]
pub(crate) fn bind_krx_payload_at(
    base_off: usize,
    max_bytes: usize,
    payload_len: usize,
    summary: &KrxSummary<'_>,
    exec: &SectionExecution,
    pid: kernel::proc::Pid,
    is_paged: bool,
) -> Result<(usize, u32), KrxLaunchError> {
    if payload_len == 0 || (!is_paged && payload_len < (exec.payload_size as usize)) {
        return Err(KrxLaunchError::TruncatedPackage);
    }
    let base_ptr = unsafe { arena_base_ptr().add(base_off) };
    let load_base = base_ptr as u32;
    let buffer_bytes = unsafe { core::slice::from_raw_parts_mut(base_ptr, max_bytes) };
    if !is_paged
        && summary.preamble.payload_crc32 != 0
        && crypto::crc::CrcEngine::crc32(&buffer_bytes[..payload_len.min(max_bytes)])
            != summary.preamble.payload_crc32
    {
        return Err(KrxLaunchError::CrcMismatch);
    }
    let load_delta = load_base.wrapping_sub(summary.preamble.link_base);
    apply_package_relocs_at(base_ptr, buffer_bytes, payload_len, exec, load_delta)?;
    flush_instruction_pipeline();
    register_service_exports_at(load_base, pid, summary.aux.service_exports);
    let entry_thumb = (base_ptr as usize).wrapping_add(exec.entry_point as usize) | 1;
    let got_base = load_base.wrapping_add(exec.got_offset as u32);
    Ok((entry_thumb, got_base))
}

/// Active guest API table populated once at package load (avoids per-call stack temporaries).
static mut LIVE_API_TABLE: KrxApiTable = KrxApiTable::with_call(0, super::syscall::kern_call);

#[inline(always)]
pub(crate) fn set_live_api_caps(caps_raw: u32) {
    unsafe {
        LIVE_API_TABLE = KrxApiTable::with_call(caps_raw, super::syscall::kern_call);
    }
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

    // 2. Fast stack margin guard (prevent Cortex-M0+ stack exhaustion before touching guard canary)
    if kernel::sys::SystemDiagnostics::stack_margin_bytes() < 256 {
        super::fault::raise_contract_fault(kernel::runtime::fault::Exception::Stack, op as u8, 0);
        return kernel::runtime::fault::KRX_FAULT_SENTINEL;
    }

    let effective_api = if core::ptr::eq(api_table, &raw const NATIVE_API_TABLE) {
        &raw const LIVE_API_TABLE
    } else {
        api_table
    };

    #[cfg(all(target_arch = "arm", not(test)))]
    {
        unsafe {
            super::fault::GUEST_GOT_BASE = got_base;
            super::fault::invoke_krx_trampoline(op, effective_api as *const (), param, entry_point)
        }
    }
    #[cfg(not(all(target_arch = "arm", not(test))))]
    {
        let _ = got_base;
        let entry: KrxNativeEntryFn = unsafe {
            let ptr = (entry_point & !1) as *const u8;
            if *ptr == 0x00 && *ptr.add(1) == 0x4b && *ptr.add(2) == 0x18 && *ptr.add(3) == 0x47 {
                let target_fn_ptr = core::ptr::read_unaligned(ptr.add(4) as *const usize);
                core::mem::transmute(target_fn_ptr)
            } else {
                core::mem::transmute(entry_point)
            }
        };
        unsafe { entry(op, effective_api, param) }
    }
}

#[inline(never)]
unsafe fn invoke_guest(op: u32, param: *const ()) -> u32 {
    let slot = slot_ref();
    if !slot.is_loaded || slot.entry_point == 0 {
        return 0xFFFF_FFFF;
    }
    unsafe {
        // Ensure any earlier host-side stack spike cannot leave a stale canary breach
        // that would falsely accuse an innocent KRX guest upon return.
        if !kernel::sys::SystemDiagnostics::is_stack_guard_intact() {
            kernel::sys::SystemDiagnostics::paint_stack();
        }
        let res = invoke_krx_native(
            slot.entry_point,
            slot.got_base,
            op,
            &raw const NATIVE_API_TABLE,
            param,
        );
        if !kernel::sys::SystemDiagnostics::is_stack_guard_intact() {
            kernel::sys::SystemDiagnostics::paint_stack();
            super::fault::raise_contract_fault(
                kernel::runtime::fault::Exception::Stack,
                op as u8,
                0,
            );
        }
        if res == kernel::runtime::fault::KRX_FAULT_SENTINEL || super::fault::has_pending_fault() {
            super::fault::finish_fault_cleanup();
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

#[inline(always)]
pub fn set_pending_start_arg(arg: u32) {
    slot_mut().pending_start_arg = arg;
}

#[inline(always)]
fn take_pending_start_arg() -> u32 {
    let slot = slot_mut();
    let a = slot.pending_start_arg;
    slot.pending_start_arg = 0;
    a
}

#[inline(always)]
pub fn active_pkg_matches(path: &str) -> bool {
    let slot = slot_ref();
    slot.is_loaded && slot.pkg_path.as_bytes() == path.as_bytes()
}

pub fn dispatch_action(action_id: u16) -> EventResult {
    dispatch_krx_event(
        kernel::proc::abi::KRX_EV_IPC,
        0,
        kernel::proc::abi::KRX_IPC_ACTION_FLAG,
        action_id as u32,
    )
}

pub unsafe fn native_on_start() {
    let arg = take_pending_start_arg();
    let res = unsafe { invoke_guest(KRX_OP_START, arg as usize as *const ()) };
    if res == 0xFFFF_FFFF {
        slot_mut().is_loaded = false;
    }
}

pub unsafe fn native_on_event(ev: &Event) -> EventResult {
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
        Event::Tick => (KRX_EV_TICK, 0, 0, gfx::views::repeater::tick_dt_ms() as u32),
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
    dispatch_krx_event(kind, key, flags, payload)
}

fn dispatch_krx_event(kind: u8, key: u8, flags: u8, payload: u32) -> EventResult {
    let krx_ev = KrxEvent {
        kind,
        key,
        flags,
        reserved: 0,
        payload,
    };
    let res = unsafe { invoke_guest(KRX_OP_EVENT, &krx_ev as *const _ as *const ()) };
    if res != 0 && res != 0xFFFF_FFFF {
        EventResult::Consumed
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn native_on_modal_result(r: &ModalResult<'_>) -> EventResult {
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
        ModalResult::NumberInputDone { tag, value } => (
            *tag,
            kernel::proc::abi::MODAL_RESULT_NUMBER_DONE,
            *value as u32,
        ),
        ModalResult::SheetSelected { tag, index } => (
            *tag,
            kernel::proc::abi::MODAL_RESULT_SHEET_SELECTED,
            *index as u32,
        ),
        ModalResult::Canceled(tag) => (*tag, kernel::proc::abi::MODAL_RESULT_CANCELED, 0),
    };
    dispatch_krx_event(kernel::proc::abi::KRX_EV_MODAL, tag, flags, payload)
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
    }
}

/// Decode active KRX custom status-bar icon IDs into stack-local glyph buffers (saving ~80 B static BSS).
pub(crate) fn apply_slot_status_icons<'a>(
    status_bar: &mut StatusBarConfig<'a>,
    left_out: &'a mut [kernel::ui::StatusGlyph; 4],
    center_out: &'a mut [kernel::ui::StatusGlyph; 4],
) {
    let slot = slot_ref();
    if slot.status_bar_has_custom {
        let sb = &slot.status_bar;
        let ln = decode_icon_list(sb, &sb.left_icons, sb.left_icon_count, left_out) as usize;
        let cn = decode_icon_list(sb, &sb.center_icons, sb.center_icon_count, center_out) as usize;
        status_bar.left_icons = &left_out[..ln];
        status_bar.center_icons = &center_out[..cn];
    }
}

pub(crate) fn decode_status_glyph(
    sb: &kernel::proc::abi::KrxStatusBar,
    id: u8,
) -> Option<kernel::ui::StatusGlyph> {
    if id == kernel::proc::abi::KRX_ICON_CUSTOM_0 {
        Some(kernel::ui::StatusGlyph::Custom(sb.custom_icon_0))
    } else if id == kernel::proc::abi::KRX_ICON_CUSTOM_1 {
        Some(kernel::ui::StatusGlyph::Custom(sb.custom_icon_1))
    } else {
        kernel::ui::Icon::from_u8(id).map(kernel::ui::StatusGlyph::Builtin)
    }
}

fn decode_icon_list(
    sb: &kernel::proc::abi::KrxStatusBar,
    icons: &[u8],
    count: u8,
    out: &mut [kernel::ui::StatusGlyph; 4],
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
    } else {
        unsafe {
            core::ptr::copy_nonoverlapping(
                sb as *const u8,
                &mut slot.status_bar as *mut kernel::proc::abi::KrxStatusBar as *mut u8,
                64,
            );
        }
        slot.status_bar_has_custom = true;
        // Auto-register or clear status tray icon requested by app
        if slot.status_bar.tray_icon_id != 0 {
            super::syscall::ui::set_tray_icon(slot.status_bar.tray_icon_id);
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
                src as *const kernel::proc::abi::KrxStatusBar as *const u8,
                out_sb as *mut u8,
                64,
            );
        }
    }
}

pub unsafe fn native_on_render(_canvas: &mut Canvas, status_bar: &mut StatusBarConfig<'_>) {
    unsafe { invoke_guest(KRX_OP_RENDER, core::ptr::null()) };
    apply_slot_status_bar(slot_ref(), status_bar);
}

pub(crate) fn teardown_slot_and_hw(pid: kernel::proc::Pid, persist_tray: bool) {
    let slot = slot_mut();
    crate::audio_capture_mut().stop_capture();
    let cfg = crate::master_config();
    cfg.release_controls();
    if pid.0 != 0 {
        super::syscall::ipc_channel_mut().purge_pid(pid.0);
        super::syscall::service_registry_mut().unbind_provider(pid);
        if !persist_tray {
            crate::orchestrator::status_tray_mut().clear_icon(pid);
        }
        #[cfg(not(test))]
        {
            let client_id = kernel::service::radio::ClientId::for_krx_pid(pid.0);
            crate::radio_state()
                .coord
                .release_client_lock(&mut crate::kern_hw().radio, client_id);
        }
    }
    #[cfg(not(test))]
    if crate::radio_state().coord.is_transmitting {
        crate::radio_state()
            .coord
            .request_ptt_stop(&mut crate::kern_hw().radio);
    }
    slot.is_loaded = false;
    slot.entry_point = 0;
    slot.status_bar_has_custom = false;
    slot.pkg_path.clear();
    #[cfg(feature = "filesystem")]
    crate::services::lang::unbind_app();
    super::fault::clear_loaded_pkg();
}

pub unsafe fn native_on_exit() {
    let slot = slot_mut();
    let pid = slot.pid;
    if slot.is_loaded && slot.entry_point != 0 {
        unsafe { invoke_guest(KRX_OP_EXIT, core::ptr::null()) };
    }
    let persist_tray = slot.status_bar_has_custom
        && (slot.status_bar.custom_flags & kernel::proc::abi::KRX_CUSTOM_TRAY_PERSIST) != 0;
    teardown_slot_and_hw(pid, persist_tray);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::proc::memory::MemoryMapLayout;

    #[test]
    fn query_region_matches_memory_map_layout_for_native_and_p0() {
        let overlay_size = 1024u32;

        for (profile, is_paged, code_size, got_off) in [
            (KrxProfile::N1NativePic, false, 1200u16, 1200u32),
            (KrxProfile::N1NativePic, false, 900u16, 0u32),
            (KrxProfile::P0StructuredPaged, true, 2048u16, 2048u32),
            (KrxProfile::P1TransparentPaged, true, 4096u16, 0u32),
        ] {
            super::super::background::BgManager::reset();
            let load_base = crate::data::arena::zone_b_base_ptr() as u32;
            let max_arena = crate::data::arena::TRANSIENT_ARENA_BYTES
                .saturating_sub(crate::data::arena::zone_a_used_bytes())
                as u32;
            let overlay_vma = load_base + max_arena - overlay_size;
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
