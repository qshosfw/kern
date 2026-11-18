//! Service Broker, IPC Channel, and System Management System Call Handlers.
//!
//! Provides dynamic service registration and resolution (`ServiceApi`),
//! process discovery, bounded non-blocking IPC channels, shadow stack paged calls,
//! and hardware-accelerated math routines (`MathApi`).

use kernel::ipc::{IpcChannel, KrxIpcMessage, ipc_msg_type, ipc_target};
use kernel::proc::abi::{
    KRX_ABI_VERSION_3_0, KRX_QS1_EMPTY, KRX_QS1_INVALID, KRX_QS1_NONE, KRX_QS1_UNVERIFIED,
    KRX_SYS_INFO_FLAG_CALIB_VALID, KRX_SYS_INFO_FLAG_HAS_CERT, KRX_SYS_INFO_FLAG_RDP_PROTECTED,
    KrxProcInfo, KrxSysInfo, MathApi, ServiceApi,
};
use kernel::proc::primitives::*;
pub use kernel::proc::service_registry::ServiceRegistry;

pub static mut SERVICE_REGISTRY: ServiceRegistry = ServiceRegistry::new();
pub static mut KERNEL_IPC_CHANNEL: IpcChannel<8> = IpcChannel::new();

#[inline(always)]
pub fn ipc_channel_mut() -> &'static mut IpcChannel<8> {
    unsafe { &mut *core::ptr::addr_of_mut!(KERNEL_IPC_CHANNEL) }
}

#[inline(always)]
pub fn service_registry_mut() -> &'static mut ServiceRegistry {
    unsafe { &mut *core::ptr::addr_of_mut!(SERVICE_REGISTRY) }
}

#[inline(never)]
pub fn active_caller_pid() -> u8 {
    let slot = super::super::native_runner::slot_ref();
    if slot.is_loaded && slot.pid.0 != 0 {
        slot.pid.0
    } else {
        for p in &crate::orchestrator::proc_table().slots {
            if p.state == kernel::proc::ProcState::Running {
                return p.pid.0;
            }
        }
        1
    }
}

pub unsafe extern "C" fn api_sys_stack_free() -> u16 {
    kernel::sys::SystemDiagnostics::stack_margin_bytes() as u16
}

pub unsafe extern "C" fn api_sys_stack_free_min() -> u16 {
    kernel::sys::SystemDiagnostics::ram_available_bytes() as u16
}

pub unsafe extern "C" fn api_sys_device_uid(out_uid12: *mut u8) {
    unsafe {
        if !out_uid12.is_null() {
            let uid = bsp::uid::read_mcu_uid();
            core::ptr::copy_nonoverlapping(uid.as_ptr(), out_uid12, 12);
        }
    }
}

pub unsafe extern "C" fn api_page_fault(block_id: u8, offset: u16) -> usize {
    super::super::native_runner::krx_page_fault(block_id, offset)
}

pub unsafe extern "C" fn api_svc_register(
    interface_id: u16,
    abi_version: u16,
    vtable_ptr: usize,
) -> u8 {
    (!service_registry_mut().register(
        interface_id,
        abi_version,
        kernel::proc::Pid(active_caller_pid()),
        vtable_ptr,
        false,
    )) as u8
}

pub unsafe extern "C" fn api_svc_resolve(interface_id: u16, min_abi: u16) -> usize {
    service_registry_mut()
        .resolve(interface_id, min_abi)
        .unwrap_or(0)
}

pub unsafe extern "C" fn api_sys_find_proc(name: *const u8, len: u8) -> u8 {
    if let Some(name_str) = unsafe { super::parse_str(name, (len as usize).min(16)) } {
        if let Some(pcb) = crate::orchestrator::proc_table_mut().find_by_name(name_str) {
            return pcb.pid.0;
        }
    }
    0
}

pub unsafe extern "C" fn api_sys_self_pid() -> u8 {
    active_caller_pid()
}

#[inline(never)]
pub unsafe extern "C" fn api_ipc_send_msg(msg: *const KrxIpcMessage) -> u8 {
    if msg.is_null() {
        return 1;
    }
    let m = unsafe { &*msg };
    let sender_pid = if m.sender_pid == 0 {
        active_caller_pid()
    } else {
        m.sender_pid
    };
    if ipc_channel_mut().send_from(m, sender_pid) {
        if let Some(pcb) =
            crate::orchestrator::proc_table_mut().find_mut(kernel::proc::Pid(m.target_pid))
        {
            pcb.ledger.record_ipc();
        }
        crate::system::enqueue_event(kernel::event::Event::Sys(kernel::ipc::SysEvent {
            target: m.target_pid,
            command: (m.msg_type & 0xFF) as u8,
            arg: m.seq,
        }));
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn api_ipc_recv_msg(out_msg: *mut KrxIpcMessage) -> u8 {
    if out_msg.is_null() {
        return 1;
    }
    let receiver_pid = active_caller_pid();
    if ipc_channel_mut().poll_into(receiver_pid, unsafe { &mut *out_msg }) {
        0
    } else {
        1
    }
}

#[inline(never)]
unsafe fn ipc_send_raw(
    sender_pid: u8,
    target_pid: u8,
    msg_type: u16,
    flags_and_seq: u32,
    ptr: *const u8,
    len: u8,
) -> u8 {
    let slice = unsafe { super::slice_or_empty(ptr, (len as usize).min(24)) };
    let mut msg = KrxIpcMessage::new(sender_pid, target_pid, msg_type, slice);
    if flags_and_seq != 0 {
        msg.flags = (flags_and_seq >> 16) as u8;
        msg.seq = flags_and_seq as u16;
    }
    unsafe { api_ipc_send_msg(&msg) }
}

pub unsafe extern "C" fn api_ipc_reply(
    req: *const KrxIpcMessage,
    payload: *const u8,
    len: u8,
) -> u8 {
    if req.is_null() {
        return 1;
    }
    let req_msg = unsafe { &*req };
    let f_seq = (req_msg.seq as u32) | ((kernel::ipc::ipc_flags::REPLY as u32) << 16);
    unsafe {
        ipc_send_raw(
            req_msg.target_pid,
            req_msg.sender_pid,
            req_msg.msg_type,
            f_seq,
            payload,
            len,
        )
    }
}

pub unsafe extern "C" fn api_ipc_send(
    target_name: *const u8,
    name_len: u8,
    msg: *const u8,
    msg_len: u8,
) -> u8 {
    let target_pid =
        if let Some(slice) = unsafe { super::ptr_slice(target_name, name_len as usize) } {
            if slice == b"*" || slice == b"ALL" {
                ipc_target::BROADCAST
            } else {
                let pid = unsafe { api_sys_find_proc(target_name, name_len) };
                if pid == 0 {
                    return 1;
                }
                pid
            }
        } else {
            ipc_target::FOREGROUND_APP
        };
    unsafe {
        ipc_send_raw(
            active_caller_pid(),
            target_pid,
            ipc_msg_type::APP_DATA_TRANSFER,
            0,
            msg,
            msg_len,
        )
    }
}

pub unsafe extern "C" fn api_ipc_poll(out_buf: *mut u8, max_len: u8) -> u8 {
    if out_buf.is_null() || max_len == 0 {
        return 0xFF;
    }
    let mut msg = core::mem::MaybeUninit::<KrxIpcMessage>::uninit();
    if unsafe { api_ipc_recv_msg(msg.as_mut_ptr()) } == 0 {
        let msg = unsafe { msg.assume_init_ref() };
        let copy_len = (msg.payload_len as usize).min(max_len as usize);
        unsafe {
            core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), out_buf, copy_len);
        }
        copy_len as u8
    } else {
        0xFF
    }
}

pub unsafe extern "C" fn api_svc_release(interface_id: u16) -> u8 {
    (!service_registry_mut().release(interface_id)) as u8
}

pub unsafe extern "C" fn api_sys_os_version() -> u32 {
    KRX_ABI_VERSION_3_0
}

#[inline(never)]
fn active_slot_scan(target: u8) -> (u8, Option<&'static kernel::proc::Pcb>) {
    let mut count = 0u8;
    for slot in crate::orchestrator::proc_table().slots.iter() {
        if slot.state != kernel::proc::ProcState::Dormant
            && slot.state != kernel::proc::ProcState::Dead
        {
            if count == target {
                return (count, Some(slot));
            }
            count += 1;
        }
    }
    (count, None)
}

pub unsafe extern "C" fn api_sys_proc_count() -> u8 {
    active_slot_scan(u8::MAX).0
}

pub unsafe extern "C" fn api_sys_proc_info(idx: u8, out_info: *mut KrxProcInfo) -> u8 {
    if out_info.is_null() {
        return 1;
    }
    let (_, Some(slot)) = active_slot_scan(idx) else {
        return 1;
    };
    unsafe {
        core::ptr::write_bytes(out_info, 0, 1);
        let info = &mut *out_info;
        info.pid = slot.pid.0;
        info.app_id = slot.app_id as u8;
        info.state = slot.state as u8;
        info.priority = slot.priority as u8;
        info.memory_bytes = slot.mem_bytes;
        let src = slot.name.as_bytes();
        let len = src.len().min(info.name.len());
        core::ptr::copy_nonoverlapping(src.as_ptr(), info.name.as_mut_ptr(), len);
        info.runtime_ticks = slot.ledger.cpu_ticks;
        info.exec_type = slot.exec_type as u8;

        let has_lease = crate::orchestrator::lease_manager()
            .leases
            .iter()
            .flatten()
            .any(|l| l.owner == slot.pid);
        let is_core = slot.pid.0 <= 4 || slot.app_id >= 0xF0;
        let mut pflags = 0u8;
        if is_core {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_PERSISTENT;
        }
        if slot.state == kernel::proc::ProcState::Suspended {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_SUSPENDED;
        }
        if slot.state == kernel::proc::ProcState::Running {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_FOREGROUND;
        }
        if has_lease {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_HAS_LEASE;
        }
        if !is_core {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_KILLABLE;
        }
        info.flags = pflags;
        info.stack_watermark = slot.ledger.stack_watermark;
        info.ipc_events = slot.ledger.ipc_events.min(u16::MAX as u32) as u16;
        info.page_faults = slot.ledger.page_faults.min(255) as u8;
        info.slice_overruns = slot.ledger.slice_overruns;
        info.hibernations = slot.ledger.hibernations;
        info.fault_count = slot.ledger.fault_count;
    }
    0
}

pub unsafe extern "C" fn api_sys_proc_kill(pid: u8) -> u8 {
    let is_fg = crate::orchestrator::proc_table()
        .find(kernel::proc::Pid(pid))
        .is_some_and(|p| p.state == kernel::proc::ProcState::Running);
    if !crate::orchestrator::reap_pid_resources(pid) {
        return 1;
    }
    if is_fg {
        crate::system::enqueue_event(kernel::event::Event::Sys(kernel::ipc::SysEvent {
            target: 0,
            command: 17,
            arg: 0,
        }));
    }
    0
}

pub unsafe extern "C" fn api_paged_call(
    target_block_id: u8,
    target_offset: u16,
    return_offset: u16,
) -> usize {
    super::super::native_runner::krx_call_paged(target_block_id, target_offset, return_offset)
        .unwrap_or(0)
}

pub unsafe extern "C" fn api_paged_return() -> usize {
    super::super::native_runner::krx_return_thunk().unwrap_or(0)
}

pub unsafe extern "C" fn api_leaf_call(leaf_id: u8, arg: u32) -> u32 {
    super::super::native_runner::krx_leaf_call(leaf_id, arg)
}

#[inline(never)]
fn lease_op(lease_kind: u8, acquire: bool, timeout_ms: u32) -> u8 {
    let Some(kind) = kernel::proc::LeaseKind::from_u8(lease_kind) else {
        return 1;
    };
    let pid = kernel::proc::Pid(active_caller_pid());
    let mgr = crate::orchestrator::lease_manager_mut();
    let res = if acquire {
        let tick = py32_hal::delay::millis();
        mgr.acquire(pid, kind, timeout_ms, tick)
    } else {
        mgr.release(pid, kind)
    };
    res.is_err() as u8
}

pub unsafe extern "C" fn api_lease_acquire(lease_kind: u8, timeout_ms: u32) -> u8 {
    lease_op(lease_kind, true, timeout_ms)
}

pub unsafe extern "C" fn api_lease_release(lease_kind: u8) -> u8 {
    lease_op(lease_kind, false, 0)
}

pub unsafe extern "C" fn api_mem_query(
    kind: u8,
    out_desc: *mut kernel::proc::MemoryRegionDescriptor,
) -> u8 {
    if out_desc.is_null() {
        return 1;
    }
    let slot = super::super::native_runner::slot_ref();
    if !slot.is_loaded {
        return 1;
    }
    if let Some(rk) = kernel::proc::RegionKind::from_u8(kind) {
        if let Some(desc) = slot.mem_layout.find_region_by_kind(rk) {
            unsafe {
                core::ptr::copy_nonoverlapping(desc as *const _, out_desc, 1);
            }
            return 0;
        }
    }
    1
}

#[inline(never)]
unsafe fn evt_op(op: u8, arg: u32, data: *const u8, len: u8) -> u8 {
    use kernel::proc::primitives::ServicePrimitive;
    let slice = unsafe { super::slice_or_empty(data, len as usize) };
    let Some(ctx) = super::active_ctx_opt() else {
        return 1;
    };
    let ok = match op {
        0 => <crate::services::SysContext as ServicePrimitive>::post_event(ctx, arg),
        1 => <crate::services::SysContext as ServicePrimitive>::publish_event(
            ctx, arg as u16, slice,
        ),
        _ => {
            ctx.rand32();
            return 0;
        }
    };
    u8::from(!ok)
}

pub unsafe extern "C" fn api_sys_post_event(event_id: u32) -> u8 {
    unsafe { evt_op(0, event_id, core::ptr::null(), 0) }
}

pub unsafe extern "C" fn api_sys_publish_event(topic: u16, data: *const u8, len: u8) -> u8 {
    unsafe { evt_op(1, topic as u32, data, len) }
}

pub unsafe extern "C" fn api_sys_rand32() -> u32 {
    super::active_ctx_opt().map_or(0, |ctx| ctx.rand32())
}

pub unsafe extern "C" fn api_math_crc32(data: *const u8, len: u16) -> u32 {
    unsafe { super::ptr_slice(data, len as usize) }.map_or(0, crypto::crc::CrcEngine::crc32)
}

pub unsafe extern "C" fn api_math_crc16(data: *const u8, len: u16) -> u16 {
    unsafe { super::ptr_slice(data, len as usize) }.map_or(0, crypto::crc::CrcEngine::crc16_ccitt)
}

pub unsafe extern "C" fn api_math_idivmod(n: i32, d: i32) -> u64 {
    if d == 0 {
        return (n as u32 as u64) << 32;
    }
    let m_n = n >> 31;
    let un = ((n ^ m_n) - m_n) as u32;
    let m_d = d >> 31;
    let ud = ((d ^ m_d) - m_d) as u32;
    let packed = unsafe { api_math_uidivmod(un, ud) };
    let uq = packed as i32;
    let ur = (packed >> 32) as i32;
    let m_q = m_n ^ m_d;
    let q = (uq ^ m_q) - m_q;
    let r = (ur ^ m_n) - m_n;
    ((r as u32 as u64) << 32) | (q as u32 as u64)
}

pub unsafe extern "C" fn api_math_uidivmod(n: u32, d: u32) -> u64 {
    let (q, r) = if d == 0 { (0, n) } else { (n / d, n % d) };
    ((r as u64) << 32) | (q as u64)
}

pub unsafe extern "C" fn api_math_isqrt(val: u32) -> u32 {
    kernel::proc::abi::isqrt(val)
}

#[inline(never)]
unsafe fn copy_cstr_raw(dst: *mut u8, cap: usize, src: &[u8]) {
    let n = src.len().min(cap.saturating_sub(1));
    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, n) };
}

/// Populate a typed zero-copy `KrxSysInfo` descriptor from canonical BSP/OS sources.
pub unsafe extern "C" fn api_sys_info(out_info: *mut KrxSysInfo) -> u8 {
    if out_info.is_null() {
        return 1;
    }
    unsafe { core::ptr::write_bytes(out_info, 0, 1) };
    let out = unsafe { &mut *out_info };

    out.flash_kb = py32_hal::uid::SystemInfo::flash_size_kb();
    out.sram_kb = 16;
    if bsp::uid::is_mcu_read_protected() {
        out.flags |= KRX_SYS_INFO_FLAG_RDP_PROTECTED;
    }
    let model = if let Some(ctx) = super::active_ctx_opt() {
        if ctx.calibration.is_valid() {
            out.flags |= KRX_SYS_INFO_FLAG_CALIB_VALID;
        }
        ctx.config.system.model_str().as_bytes()
    } else {
        b"UV-K1"
    };
    out.mac = bsp::uid::read_mac_address();

    let meta = &crate::build_info::FW_METADATA;
    out.fw_major = meta.version_major;
    out.fw_minor = meta.version_minor;
    out.fw_patch = meta.version_patch;
    out.target_id = meta.target_id;

    let base = out_info as *mut u8;
    unsafe {
        copy_cstr_raw(base.add(core::mem::offset_of!(KrxSysInfo, model)), 16, model);
        copy_cstr_raw(
            base.add(core::mem::offset_of!(KrxSysInfo, mcu)),
            16,
            b"PY32F071xB",
        );
        copy_cstr_raw(
            base.add(core::mem::offset_of!(KrxSysInfo, boot_ver)),
            16,
            bsp::uid::bootloader_version_str().as_bytes(),
        );
        copy_cstr_raw(
            base.add(core::mem::offset_of!(KrxSysInfo, git_hash)),
            16,
            crate::build_info::GIT_HASH.as_bytes(),
        );
        copy_cstr_raw(
            base.add(core::mem::offset_of!(KrxSysInfo, build_date)),
            16,
            crate::build_info::BUILD_DATE.as_bytes(),
        );
    }

    out.serial = bsp::uid::get_crockford_serial();
    out.mcu_uid = bsp::uid::read_mcu_uid();

    // Non-AES certificate classification keeps AES-128-CBC out of resident firmware Flash;
    // packages requiring cryptographic QS-1 checks (e.g. sysinfo leaf 2) verify on demand.
    match bsp::uid::factory_cert_status() {
        bsp::uid::FactoryCertStatus::Erased => {
            out.qs1_status = KRX_QS1_NONE;
        }
        bsp::uid::FactoryCertStatus::Present => {
            out.flags |= KRX_SYS_INFO_FLAG_HAS_CERT;
            let empty_tail = py32_hal::uid::SystemInfo::factory_cert_slice()
                .is_some_and(|c| c[80..104].iter().all(|&b| matches!(b, 0x00 | 0xFF | b' ')));
            out.qs1_status = if empty_tail {
                KRX_QS1_EMPTY
            } else {
                KRX_QS1_INVALID
            };
        }
        bsp::uid::FactoryCertStatus::Qs1Ascii { buf, len } => {
            out.flags |= KRX_SYS_INFO_FLAG_HAS_CERT;
            out.qs1_status = KRX_QS1_UNVERIFIED;
            let n = (len as usize).min(16);
            unsafe {
                copy_cstr_raw(
                    base.add(core::mem::offset_of!(KrxSysInfo, qs1)),
                    17,
                    &buf[..n],
                );
            }
            out.qs1_len = n as u8;
        }
    }
    0
}

#[inline(never)]
fn proc_ctl_read(op: u8, target: u8, out: &mut [u32; 4]) -> bool {
    use kernel::proc::abi::*;
    use kernel::proc::{Pid, ProcState};

    match op {
        PROC_CTL_SYS_OVERVIEW => {
            let mut procs = 0u32;
            let mut fg_pid = 0u32;
            for s in &crate::orchestrator::proc_table().slots {
                if !matches!(s.state, ProcState::Dormant | ProcState::Dead) {
                    procs += 1;
                    if s.state == ProcState::Running && s.app_id < 0xF0 {
                        fg_pid = s.pid.0 as u32;
                    }
                }
            }
            out[0] = (kernel::sys::SystemDiagnostics::stack_peak_used_bytes() as u32 & 0xFFFF)
                | ((kernel::sys::SystemDiagnostics::memory_pressure() as u32 & 0xFF) << 16)
                | (fg_pid << 24);
            out[1] = procs
                | ((crate::orchestrator::lease_manager().active_count() as u32) << 8)
                | ((service_registry_mut().active_count() as u32) << 16)
                | ((ipc_channel_mut().pending_count() as u32) << 24);
            out[2] = kernel::sys::SystemDiagnostics::ram_static_used_bytes() as u32;
            out[3] = kernel::sys::SystemDiagnostics::stack_margin_bytes() as u32;
            true
        }
        PROC_CTL_LEASE_ENTRY => {
            let mut n = 0u8;
            for slot in &crate::orchestrator::lease_manager().leases {
                if let Some(l) = slot {
                    if n == target {
                        out[0] = (l.kind as u32) | ((l.owner.0 as u32) << 8);
                        out[1] = l.acquired_tick;
                        out[2] = l.timeout_ms;
                        return true;
                    }
                    n += 1;
                }
            }
            false
        }
        PROC_CTL_SVC_ENTRY => {
            let mut n = 0u8;
            for e in &service_registry_mut().entries {
                if e.is_active() {
                    if n == target {
                        out[0] = (e.interface_id as u32) | ((e.abi_version as u32) << 16);
                        out[1] = (e.provider_pid.0 as u32)
                            | ((e.pinned as u32) << 8)
                            | ((e.client_count as u32) << 16);
                        out[2] = e.vtable_ptr as u32;
                        return true;
                    }
                    n += 1;
                }
            }
            false
        }
        PROC_CTL_PROC_EXT => {
            let Some(pcb) = crate::orchestrator::proc_table().find(Pid(target)) else {
                return false;
            };
            out[0] = pcb.caps.raw();
            out[1] = (pcb.restart_budget as u32) | ((pcb.stack_cost as u32) << 8);
            out[2] = pcb.compressed_bytes as u32;
            out[3] = pcb.ledger.ipc_events;
            true
        }
        _ => false,
    }
}

#[inline(never)]
fn proc_ctl_write(op: u8, target: u8, arg: u32) -> u8 {
    use kernel::proc::abi::*;
    use kernel::proc::{LeaseKind, Pid, ProcState};

    if !super::super::native_runner::active_native_caps().contains(kernel::caps::Cap::TaskControl) {
        return 2;
    }
    match op {
        PROC_CTL_KILL => unsafe { api_sys_proc_kill(target) },
        PROC_CTL_REVOKE_LEASE => LeaseKind::from_u8(target).map_or(1, |lk| {
            u8::from(!crate::orchestrator::lease_manager_mut().revoke_kind(lk))
        }),
        PROC_CTL_RESET_STATS => {
            for slot in &mut crate::orchestrator::proc_table_mut().slots {
                slot.ledger.slice_overruns = 0;
                slot.ledger.fault_count = 0;
            }
            0
        }
        PROC_CTL_SET_STATE | PROC_CTL_SET_PRIORITY | PROC_CTL_FOCUS_APP => {
            let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut(Pid(target)) else {
                return 1;
            };
            match op {
                PROC_CTL_SET_STATE => {
                    if target == 1 || target == 4 || (target <= 4 && arg >= 2) {
                        return 1;
                    }
                    pcb.set_state(match arg {
                        1 => ProcState::Suspended,
                        2 => ProcState::Stopped,
                        _ if target <= 4 => ProcState::ServiceIdle,
                        _ => ProcState::Running,
                    });
                }
                PROC_CTL_SET_PRIORITY => {
                    let Some(prio) = kernel::sched::Priority::from_u8(arg as u8) else {
                        return 1;
                    };
                    pcb.priority = prio;
                }
                _ => {
                    if pcb.app_id >= crate::AppRegistry::app_count() {
                        return 1;
                    }
                    crate::system::enqueue_event(kernel::event::Event::Sys(
                        kernel::ipc::SysEvent {
                            target: 0,
                            command: 15,
                            arg: pcb.app_id as u16,
                        },
                    ));
                }
            }
            0
        }
        _ => 1,
    }
}

pub unsafe extern "C" fn api_sys_proc_ctl(op_and_id: u16, arg: u32, buf: *mut u8, len: u16) -> u8 {
    let op = op_and_id as u8;
    let target = (op_and_id >> 8) as u8;

    if op < 0x80 {
        return proc_ctl_write(op, target, arg);
    }
    if buf.is_null() || len < 16 {
        return 1;
    }
    if op == kernel::proc::abi::PROC_CTL_IPC_ENTRY {
        let Some(m) = ipc_channel_mut().peek_at(target as usize) else {
            return 1;
        };
        unsafe {
            core::ptr::copy_nonoverlapping(m as *const KrxIpcMessage as *const u8, buf, 16);
        }
        return 0;
    }
    let mut out = [0u32; 4];
    if !proc_ctl_read(op, target, &mut out) {
        return 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(out.as_ptr() as *const u8, buf, 16);
    }
    0
}

pub static SERVICE_VTABLE: ServiceApi = ServiceApi {
    svc_register: api_svc_register,
    svc_resolve: api_svc_resolve,
    svc_release: api_svc_release,
    ipc_send: api_ipc_send,
    ipc_poll: api_ipc_poll,
    sys_device_uid: api_sys_device_uid,
    sys_os_version: api_sys_os_version,
    sys_ram_available: api_sys_stack_free_min,
    sys_stack_margin: api_sys_stack_free,
    sys_proc_count: api_sys_proc_count,
    sys_proc_info: api_sys_proc_info,
    sys_proc_kill: api_sys_proc_kill,
    paged_call: api_paged_call,
    paged_return: api_paged_return,
    leaf_call: api_leaf_call,
    lease_acquire: api_lease_acquire,
    lease_release: api_lease_release,
    mem_query: api_mem_query,
    sys_post_event: api_sys_post_event,
    sys_publish_event: api_sys_publish_event,
    sys_stack_free_min: api_sys_stack_free_min,
    ipc_send_msg: api_ipc_send_msg,
    ipc_recv_msg: api_ipc_recv_msg,
    ipc_reply: api_ipc_reply,
    sys_find_proc: api_sys_find_proc,
    sys_self_pid: api_sys_self_pid,
    sys_info: api_sys_info,
    sys_proc_ctl: api_sys_proc_ctl,
};

pub static MATH_VTABLE: MathApi = MathApi {
    sys_rand32: api_sys_rand32,
    crc32: api_math_crc32,
    crc16: api_math_crc16,
    idivmod: api_math_idivmod,
    uidivmod: api_math_uidivmod,
    isqrt: api_math_isqrt,
};
