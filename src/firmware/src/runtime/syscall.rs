//! Unified Single-Entry System Call Router (`kern_call` — 22 orthogonal operations).
//!
//! Routes all KRX system calls through one AAPCS-Thumb entry point (`kern_call(op, a0, a1, a2)`)
//! with centralized `OpDesc` capability enforcement, stack guard, and `GuestBounds` sandboxing.

use ::abi::Cap;
use gfx::Canvas;
use kernel::proc::abi::{KRX_ABI_VERSION, KrxApiTable};

pub mod audio;
pub mod display;
pub mod io;
pub mod radio;
pub mod service;
pub mod storage;
pub mod ui;

pub use service::{
    ServiceRegistry, active_caller_pid, ipc_channel_mut, live_prop_get, live_prop_set,
    service_registry_mut,
};
pub use ui::{set_tray_icon, store_modal_text};

#[inline(always)]
pub fn active_canvas_mut() -> &'static mut Canvas {
    crate::system_canvas()
}

#[inline(always)]
pub fn has_cap(cap: Cap) -> bool {
    super::native_runner::active_native_caps().contains(cap)
}

/// Pure address-range validator for `GuestBounds` sandboxing.
pub const fn check_guest_range_bounds(
    addr: usize,
    len: usize,
    writable: bool,
    actor_start: usize,
    actor_end: usize,
    stack_floor: usize,
    stack_top: usize,
    flash_start: usize,
    flash_end: usize,
) -> bool {
    if addr == 0 || len == 0 {
        return false;
    }
    let Some(end) = addr.checked_add(len) else {
        return false;
    };
    let in_actor = addr >= actor_start && end <= actor_end;
    let in_stack = addr >= stack_floor && end <= stack_top;
    if in_actor || in_stack {
        return true;
    }
    !writable && addr >= flash_start && end <= flash_end
}

/// Cached per-syscall guest memory sandbox bounds, computed once at `kern_call` entry.
#[derive(Clone, Copy)]
struct GuestBounds {
    actor_start: usize,
    actor_end: usize,
    stack_floor: usize,
    active: bool,
}

static mut ACTIVE_GUEST_BOUNDS: GuestBounds = GuestBounds {
    actor_start: 0,
    actor_end: 0,
    stack_floor: 0,
    active: false,
};

#[inline(always)]
fn capture_guest_bounds() -> GuestBounds {
    #[cfg(all(target_arch = "arm", not(test)))]
    {
        let arena_base = crate::data::arena::arena_base_ptr() as usize;
        let (actor_start, actor_end, active) =
            if let Some(bg) = super::background::BgManager::active_bg_slot() {
                (
                    arena_base + bg.arena_off as usize,
                    arena_base + bg.end_off(),
                    true,
                )
            } else {
                let slot = super::native_runner::slot_ref();
                (
                    crate::data::arena::zone_b_base_ptr() as usize,
                    arena_base + crate::data::arena::TRANSIENT_ARENA_BYTES,
                    slot.is_loaded,
                )
            };
        let msp: usize;
        unsafe {
            core::arch::asm!("mrs {}, MSP", out(reg) msp);
        }
        GuestBounds {
            actor_start,
            actor_end,
            stack_floor: msp.saturating_sub(64),
            active,
        }
    }
    #[cfg(not(all(target_arch = "arm", not(test))))]
    {
        GuestBounds {
            actor_start: 0,
            actor_end: 0,
            stack_floor: 0,
            active: false,
        }
    }
}

#[inline(never)]
pub fn guest_range_valid(addr: usize, len: usize, writable: bool) -> bool {
    if addr == 0 || len == 0 || addr.checked_add(len).is_none() {
        return false;
    }
    let b = unsafe { ACTIVE_GUEST_BOUNDS };
    if !b.active {
        return true;
    }
    check_guest_range_bounds(
        addr,
        len,
        writable,
        b.actor_start,
        b.actor_end,
        b.stack_floor,
        0x2000_4000,
        0x0800_0000,
        0x0802_0000,
    )
}

fn guest_sig_ptrs_valid(sig: ::abi::Sig, a0: usize, a1: usize, a2: usize) -> bool {
    let args = [a0, a1, a2];
    let mut i = 0u8;
    while i < 3 {
        match sig.arg(i) {
            ::abi::ArgKind::Val => {}
            ::abi::ArgKind::Out => {
                if !guest_range_valid(args[i as usize], 1, true) {
                    return false;
                }
            }
            ::abi::ArgKind::In | ::abi::ArgKind::Str => {
                if !guest_range_valid(args[i as usize], 1, false) {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

pub fn guest_struct_ptr(addr: usize, size_and_flags: u32) -> *mut u8 {
    let size = (size_and_flags & 0xFFFF) as usize;
    let align_mask = ((size_and_flags >> 16) & 0xFF) as usize;
    let writable = (size_and_flags >> 24) != 0;
    if (addr & align_mask) == 0 && guest_range_valid(addr, size, writable) {
        addr as *mut u8
    } else {
        core::ptr::null_mut()
    }
}

#[inline(always)]
pub unsafe fn ptr_ref<'a, T>(ptr: *const T) -> Option<&'a T> {
    const fn pack<T>() -> u32 {
        (core::mem::size_of::<T>() as u32) | (((core::mem::align_of::<T>() - 1) as u32) << 16)
    }
    let p = guest_struct_ptr(ptr as usize, pack::<T>()) as *const T;
    unsafe { p.as_ref() }
}

#[inline(always)]
pub unsafe fn ptr_ref_mut<'a, T>(ptr: *mut T) -> Option<&'a mut T> {
    const fn pack_mut<T>() -> u32 {
        (core::mem::size_of::<T>() as u32)
            | (((core::mem::align_of::<T>() - 1) as u32) << 16)
            | (1 << 24)
    }
    let p = guest_struct_ptr(ptr as usize, pack_mut::<T>()) as *mut T;
    unsafe { p.as_mut() }
}

pub unsafe fn ptr_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if guest_range_valid(ptr as usize, len, false) {
        Some(unsafe { core::slice::from_raw_parts(ptr, len) })
    } else {
        None
    }
}

pub unsafe fn ptr_slice_mut<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    if guest_range_valid(ptr as usize, len, true) {
        Some(unsafe { core::slice::from_raw_parts_mut(ptr, len) })
    } else {
        None
    }
}

pub unsafe fn slice_or_empty<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    unsafe { ptr_slice(ptr, len) }.unwrap_or(&[])
}

pub unsafe fn parse_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    unsafe { ptr_slice(ptr, len) }.and_then(utils::format::ascii_str)
}

pub unsafe fn parse_user_path<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    let path = unsafe { parse_str(ptr, len)? };
    if !utils::path::is_clean_path(path.as_bytes()) {
        return None;
    }
    let caps = super::native_runner::active_native_caps();
    if utils::path::root_bit(path.as_bytes()) == utils::path::ROOT_SYS {
        return if caps.contains(Cap::FactoryCalib) {
            Some(path)
        } else {
            None
        };
    }
    if caps.contains(Cap::StorageWrite) || caps.contains(Cap::StorageAdmin) {
        Some(path)
    } else {
        None
    }
}

pub unsafe fn str_or_default<'a>(ptr: *const u8, len: usize, default: &'a str) -> &'a str {
    unsafe { parse_str(ptr, len) }.unwrap_or(default)
}

pub type OpHandler = unsafe fn(usize, usize, usize) -> usize;

static OP_HANDLERS: [OpHandler; ::abi::OP_TABLE.len()] = [
    // SYS (0x0)
    service::op_sys_prop,
    service::op_sys_query,
    service::op_sys_ctl,
    service::op_sys_div,
    service::op_sys_sec,
    // PROC (0x1)
    service::op_proc_ctl,
    service::op_proc_exec,
    // IPC (0x2)
    service::op_ipc,
    // FS (0x3)
    storage::op_fs_read,
    storage::op_fs_write,
    storage::op_fs_meta,
    // RF (0x4)
    radio::op_rf_tune,
    radio::op_rf_vfo,
    radio::op_rf_bulk,
    // AUD (0x5)
    audio::op_aud,
    // GFX (0x6)
    display::op_gfx_draw,
    display::op_gfx_text,
    // UI (0x7)
    ui::op_ui_modal,
    ui::op_ui_widget,
    // IO (0x8)
    io::op_io_key,
    io::op_io_serial,
    io::op_io_bus,
];

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kern_call(op: u32, a0: usize, a1: usize, a2: usize) -> usize {
    let op_byte = op as u8;
    let Some((op_idx, desc)) = ::abi::Op::lookup(op_byte) else {
        return ::abi::Status::Unsupported.to_raw();
    };

    #[cfg(all(target_arch = "arm", not(test)))]
    if super::fault::can_contain_guest_in_thread() {
        let min_margin = if (desc.flags & ::abi::op_flags::HIGH_STACK) != 0 {
            304usize
        } else {
            176usize
        };
        if !kernel::sys::SystemDiagnostics::is_stack_guard_intact()
            || kernel::sys::SystemDiagnostics::stack_margin_bytes() < min_margin
        {
            unsafe {
                super::fault::unwind_guest_fault(kernel::runtime::fault::Exception::Stack, op_byte);
            }
        }
    }

    let req_mask = desc.cap_mask();
    if req_mask != 0 && !super::native_runner::active_native_caps().contains_mask(req_mask) {
        return ::abi::Status::PermissionDenied.to_raw();
    }
    let prev_bounds = unsafe { ACTIVE_GUEST_BOUNDS };
    unsafe {
        ACTIVE_GUEST_BOUNDS = capture_guest_bounds();
    }
    if !guest_sig_ptrs_valid(desc.sig, a0, a1, a2) {
        unsafe {
            ACTIVE_GUEST_BOUNDS = prev_bounds;
        }
        return ::abi::Status::InvalidArg.to_raw();
    }

    let ret = unsafe { (*OP_HANDLERS.get_unchecked(op_idx))(a0, a1, a2) };
    unsafe {
        ACTIVE_GUEST_BOUNDS = prev_bounds;
    }
    ret
}

pub static NATIVE_API_TABLE: KrxApiTable = KrxApiTable {
    abi_version: KRX_ABI_VERSION,
    table_size: core::mem::size_of::<KrxApiTable>() as u16,
    _pad0: 0,
    granted_caps: 0xFFFF_FFFF,
    call: kern_call,
};

#[cfg(test)]
mod tests {
    use super::*;
    use ::abi::{Op, prop, sub};
    use kernel::proc::abi::{KrxIpcSendReq, KrxSysInfo};

    #[test]
    fn operation_without_its_imported_capability_is_refused_at_dispatch() {
        use ::abi::{Cap, CapabilitySet};
        let slot = crate::runtime::native_runner::slot_mut();
        slot.is_loaded = true;
        slot.caps = CapabilitySet::empty().with(Cap::DisplayDirect);
        let api = &NATIVE_API_TABLE;
        let _ticks = api.ticks_ms();
        assert_eq!(
            unsafe { api.invoke(Op::IoKey, 0, 0, 0) },
            ::abi::Status::PermissionDenied.to_raw()
        );
        // Radio live property refused without RadioRx
        assert_eq!(
            unsafe { api.invoke(Op::SysProp, prop::RF_FREQ as usize, 0, 0) },
            ::abi::Status::PermissionDenied.to_raw()
        );
        slot.caps = CapabilitySet::ALL;
        assert_eq!(
            unsafe { kern_call(0xFF, 0, 0, 0) },
            ::abi::Status::Unsupported.to_raw()
        );
        assert_eq!(
            unsafe { api.invoke(Op::GfxText, 0, 0, 0) },
            ::abi::Status::InvalidArg.to_raw()
        );
        crate::runtime::native_runner::init_native_slot();
    }

    #[test]
    fn guest_bounds_sandbox_rejects_kernel_bss_zone_a_and_mmio() {
        let actor_start = 0x2000_1200usize;
        let actor_end = 0x2000_2A00usize;
        let stack_floor = 0x2000_3A00usize;
        let stack_top = 0x2000_4000usize;
        let flash_start = 0x0800_0000usize;
        let flash_end = 0x0802_0000usize;

        let check = |addr, len, wr| {
            check_guest_range_bounds(
                addr,
                len,
                wr,
                actor_start,
                actor_end,
                stack_floor,
                stack_top,
                flash_start,
                flash_end,
            )
        };

        assert!(check(0x2000_1200, 64, true));
        assert!(check(0x2000_3C00, 128, true));
        assert!(check(0x0800_4000, 16, false));
        assert!(!check(0x0800_4000, 16, true));
        assert!(!check(0x2000_0100, 4, true));
        assert!(!check(0x2000_0100, 4, false));
        assert!(!check(0x2000_0E00, 16, true));
        assert!(!check(0x2000_2B00, 16, true));
        assert!(!check(0x4000_0000, 4, true));
        assert!(!check(0x2000_1400, usize::MAX, false));
    }

    #[test]
    fn test_abi_cross_process_ipc_and_discovery() {
        crate::orchestrator::init_proc_table();
        let table = crate::orchestrator::proc_table_mut();
        let _ = table.spawn(
            "Home",
            crate::apps::APP_ID_LAUNCHER,
            kernel::sched::Priority::Ui,
            kernel::proc::ExecType::StaticFirmware,
            ::abi::CapabilitySet::empty(),
        );
        let _ = table.spawn(
            "AprsSvc",
            0xE0,
            kernel::sched::Priority::Radio,
            kernel::proc::ExecType::RelocatableNative,
            ::abi::CapabilitySet::empty(),
        );
        let _ = table.spawn(
            "Modem",
            0xE1,
            kernel::sched::Priority::Idle,
            kernel::proc::ExecType::RelocatableNative,
            ::abi::CapabilitySet::empty(),
        );

        let api = &NATIVE_API_TABLE;

        let aprs_pid = unsafe {
            api.invoke(
                Op::ProcCtl,
                (sub::proc_ctl::FIND as usize) | (7 << 8),
                b"AprsSvc\0".as_ptr() as usize,
                0,
            )
        } as u8;
        assert_eq!(aprs_pid, 2);

        let modem_pid = unsafe {
            api.invoke(
                Op::ProcCtl,
                (sub::proc_ctl::FIND as usize) | (5 << 8),
                b"modem\0".as_ptr() as usize,
                0,
            )
        } as u8;
        assert_eq!(modem_pid, 3);

        let non_existent = unsafe {
            api.invoke(
                Op::ProcCtl,
                (sub::proc_ctl::FIND as usize) | (5 << 8),
                b"ghost\0".as_ptr() as usize,
                0,
            )
        } as u8;
        assert_eq!(non_existent, 0);

        let msg = kernel::ipc::KrxIpcMessage::new(
            aprs_pid,
            1,
            kernel::ipc::ipc_msg_type::SERVICE_REQUEST,
            b"GET_CONFIG",
        );
        assert!(api.ipc_send(&msg));

        let mut received = kernel::ipc::KrxIpcMessage::empty();
        assert!(api.ipc_recv(&mut received));
        assert_eq!(received.sender_pid, aprs_pid);
        assert_eq!(received.target_pid, 1);
        assert_eq!(&received.payload[..10], b"GET_CONFIG");

        assert!(api.ipc_reply(&received, b"CONFIG_DATA"));

        let sreq = KrxIpcSendReq {
            target: b"AprsSvc".as_ptr(),
            msg: b"CMD_SET_FREQ".as_ptr(),
            target_len: 7,
            msg_len: 12,
            _pad: [0; 2],
        };
        assert!(api.ipc_send_str(&sreq));

        let mut info = core::mem::MaybeUninit::<KrxSysInfo>::uninit();
        assert_eq!(
            unsafe {
                api.invoke(
                    Op::SysQuery,
                    sub::sys_query::INFO as usize,
                    info.as_mut_ptr() as usize,
                    0,
                )
            },
            0
        );
        let info = unsafe { info.assume_init() };
        assert_eq!(info.flash_kb, 128);
        assert_eq!(info.sram_kb, 16);
        assert_eq!(info.mac, bsp::uid::read_mac_address());
        assert_eq!(info.serial, bsp::uid::get_crockford_serial());
        assert_eq!(info.mcu_uid, bsp::uid::read_mcu_uid());
        assert_ne!(
            info.flags & kernel::proc::abi::KRX_SYS_INFO_FLAG_HAS_CERT,
            0
        );
        assert_eq!(&info.qs1[..info.qs1_len as usize], b"C111520222909161");
        assert_eq!(info.qs1[info.qs1_len as usize], 0);
        assert_eq!(info.qs1_status, kernel::proc::abi::KRX_QS1_UNVERIFIED);
        assert_eq!(&info.boot_ver[..7], b"7.02.02");
        assert_ne!(
            unsafe { api.invoke(Op::SysQuery, sub::sys_query::INFO as usize, 0, 0) },
            0
        );

        ipc_channel_mut().purge_pid(aprs_pid);
        ipc_channel_mut().purge_pid(1);
    }

    #[test]
    fn fuzz_kern_call_invalid_and_null_boundary() {
        use ::abi::domain;
        super::super::native_runner::init_native_slot();
        for dom in 0u8..16 {
            for idx in 0u8..16 {
                let op = ((dom as u32) << 4) | (idx as u32);
                if op == Op::SysCtl.u32_id()
                    || dom == domain::IO
                    || dom == domain::RF
                    || dom == domain::AUD
                    || (dom == domain::PROC && idx == 0)
                {
                    continue;
                }
                let _ = unsafe { kern_call(op, 0, 0, 0) };
                if op != Op::GfxText.u32_id() {
                    let _ = unsafe { kern_call(op, 0xFF, 0, 0) };
                }
            }
        }
        let mut raw = [0u8; core::mem::size_of::<KrxSysInfo>() + 4];
        let unaligned_ptr = unsafe { raw.as_mut_ptr().add(1) } as usize;
        assert_ne!(
            unsafe {
                kern_call(
                    Op::SysQuery.u32_id(),
                    sub::sys_query::INFO as usize,
                    unaligned_ptr,
                    0,
                )
            },
            0
        );
        assert_ne!(
            unsafe {
                kern_call(
                    Op::ProcCtl.u32_id(),
                    sub::proc_ctl::INFO as usize,
                    unaligned_ptr,
                    0,
                )
            },
            0
        );
        let mut rem = 0xDEAD_BEEFu32;
        assert_eq!(
            unsafe { kern_call(Op::SysDiv.u32_id(), 42, 0, &raw mut rem as usize) },
            0
        );
        assert_eq!(rem, 42);
    }
}
