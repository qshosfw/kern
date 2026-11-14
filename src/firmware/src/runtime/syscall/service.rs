//! Service Broker, IPC Channel, and System Management System Call Handlers.
//!
//! Provides dynamic service registration and resolution (`ServiceApi`),
//! process discovery, bounded non-blocking IPC channels, shadow stack paged calls,
//! and hardware-accelerated math routines (`MathApi`).

use kernel::ipc::{IpcChannel, KrxIpcMessage, ipc_msg_type, ipc_target};
use kernel::proc::abi::{KRX_ABI_VERSION_3_0, KrxProcInfo, MathApi, ServiceApi};
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
            let dst = core::slice::from_raw_parts_mut(out_uid12, 12);
            dst.copy_from_slice(&uid[..12]);
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
    let provider_id = crate::AppRegistry::count() as u8;
    (!service_registry_mut().register(
        interface_id,
        abi_version,
        kernel::proc::Pid(provider_id),
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

pub unsafe extern "C" fn api_ipc_send_msg(msg: *const KrxIpcMessage) -> u8 {
    if msg.is_null() {
        return 1;
    }
    let mut m = unsafe { *msg };
    if m.sender_pid == 0 {
        m.sender_pid = active_caller_pid();
    }
    if ipc_channel_mut().send(m) {
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
    if let Some(msg) = ipc_channel_mut().poll(receiver_pid) {
        unsafe {
            *out_msg = msg;
        }
        0
    } else {
        1
    }
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
    let slice = unsafe { super::slice_or_empty(payload, (len as usize).min(24)) };
    let reply_msg = req_msg.reply_to(slice, false);
    unsafe { api_ipc_send_msg(&reply_msg) }
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

    let payload_slice = unsafe { super::slice_or_empty(msg, (msg_len as usize).min(24)) };

    let ipc_msg = KrxIpcMessage::new(
        active_caller_pid(),
        target_pid,
        ipc_msg_type::APP_DATA_TRANSFER,
        payload_slice,
    );
    unsafe { api_ipc_send_msg(&ipc_msg) }
}

pub unsafe extern "C" fn api_ipc_poll(out_buf: *mut u8, max_len: u8) -> u8 {
    if out_buf.is_null() || max_len == 0 {
        return 0xFF;
    }
    let mut msg = KrxIpcMessage::empty();
    if unsafe { api_ipc_recv_msg(&mut msg) } == 0 {
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

pub unsafe extern "C" fn api_sys_proc_count() -> u8 {
    let table = crate::orchestrator::proc_table();
    let mut count = 0u8;
    for slot in table.slots.iter() {
        if slot.state != kernel::proc::ProcState::Dormant
            && slot.state != kernel::proc::ProcState::Dead
        {
            count += 1;
        }
    }
    count
}

pub unsafe extern "C" fn api_sys_proc_info(idx: u8, out_info: *mut KrxProcInfo) -> u8 {
    if out_info.is_null() {
        return 1;
    }
    let table = crate::orchestrator::proc_table();
    let mut match_idx = 0u8;
    for slot in table.slots.iter() {
        if slot.state != kernel::proc::ProcState::Dormant
            && slot.state != kernel::proc::ProcState::Dead
        {
            if match_idx == idx {
                unsafe {
                    core::ptr::write_bytes(out_info, 0, 1);
                    let info = &mut *out_info;
                    info.pid = slot.pid.0;
                    info.app_id = slot.app_id as u8;
                    info.state = match slot.state {
                        kernel::proc::ProcState::Running => 2,
                        kernel::proc::ProcState::Suspended => 3,
                        _ => 1,
                    };
                    info.priority = slot.priority as u8;
                    info.memory_bytes = slot.mem_bytes;
                    let name_bytes = slot.name.as_bytes();
                    let copy_len = name_bytes.len().min(12);
                    info.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                    info.runtime_ticks = slot.ledger.cpu_ticks;
                }
                return 0;
            }
            match_idx += 1;
        }
    }
    1
}

pub unsafe extern "C" fn api_sys_proc_kill(pid: u8) -> u8 {
    let table = crate::orchestrator::proc_table_mut();
    if let Some(pcb) = table.find_mut(kernel::proc::Pid(pid)) {
        if pcb.app_id < crate::AppRegistry::count() {
            // Cannot kill core firmware app
            return 1;
        }
        pcb.state = kernel::proc::ProcState::Dead;
        crate::apps::AppRegistry::unregister_dynamic();
        0
    } else {
        1
    }
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

fn lease_op(lease_kind: u8, acquire: bool, timeout_ms: u32) -> u8 {
    let Some(kind) = kernel::proc::LeaseKind::from_u8(lease_kind) else {
        return 1;
    };
    let pid = kernel::proc::Pid(crate::apps::AppRegistry::count() as u8);
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

pub unsafe extern "C" fn api_sys_post_event(event_id: u32) -> u8 {
    use kernel::proc::primitives::ServicePrimitive;
    if let Some(ctx) = super::active_ctx_opt() {
        if <crate::services::SysContext as ServicePrimitive>::post_event(ctx, event_id) {
            0
        } else {
            1
        }
    } else {
        1
    }
}

pub unsafe extern "C" fn api_sys_publish_event(topic: u16, data: *const u8, len: u8) -> u8 {
    let slice = unsafe { super::slice_or_empty(data, len as usize) };
    use kernel::proc::primitives::ServicePrimitive;
    if let Some(ctx) = super::active_ctx_opt() {
        u8::from(
            !<crate::services::SysContext as ServicePrimitive>::publish_event(ctx, topic, slice),
        )
    } else {
        1
    }
}

pub unsafe extern "C" fn api_sys_rand32() -> u32 {
    if let Some(ctx) = super::active_ctx_opt() {
        ctx.rand32()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_math_crc32(data: *const u8, len: u16) -> u32 {
    if !data.is_null() && len > 0 {
        let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
        crypto::crc::CrcEngine::crc32(slice)
    } else {
        0
    }
}

pub unsafe extern "C" fn api_math_crc16(data: *const u8, len: u16) -> u16 {
    if !data.is_null() && len > 0 {
        let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
        crypto::crc::CrcEngine::crc16_ccitt(slice)
    } else {
        0
    }
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
};

pub static MATH_VTABLE: MathApi = MathApi {
    sys_rand32: api_sys_rand32,
    crc32: api_math_crc32,
    crc16: api_math_crc16,
    idivmod: api_math_idivmod,
    uidivmod: api_math_uidivmod,
    isqrt: api_math_isqrt,
};
