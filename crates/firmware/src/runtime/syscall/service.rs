//! System, Process, Service Broker & IPC System Call Dispatchers
//! (`Op::Sys*` 0x0, `Op::Proc*` 0x1, `Op::Ipc*` 0x2).

use super::{parse_str, ptr_slice, slice_or_empty};
use kernel::ipc::{IpcChannel, KrxIpcMessage, ipc_msg_type, ipc_target};
use kernel::proc::abi::{
    KRX_ABI_VERSION, KRX_QS1_INVALID, KRX_QS1_UNVERIFIED, KRX_SYS_INFO_FLAG_CALIB_VALID,
    KRX_SYS_INFO_FLAG_HAS_CERT, KRX_SYS_INFO_FLAG_RDP_PROTECTED, KrxIpcSendReq, KrxProcInfo,
    KrxSysInfo,
};
pub use kernel::proc::service_registry::ServiceRegistry;

pub static mut SERVICE_REGISTRY: ServiceRegistry<4> = ServiceRegistry::new();
pub static mut KERNEL_IPC_CHANNEL: IpcChannel<4> = IpcChannel::new();

#[inline(always)]
pub fn ipc_channel_mut() -> &'static mut IpcChannel<4> {
    unsafe { &mut *core::ptr::addr_of_mut!(KERNEL_IPC_CHANNEL) }
}

#[inline(always)]
pub fn service_registry_mut() -> &'static mut ServiceRegistry<4> {
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

#[inline(never)]
fn post_sys_event(target: u8, command: u8, arg: u16) {
    crate::system::enqueue_event(kernel::event::Event::Sys(kernel::ipc::SysEvent {
        target,
        command,
        arg,
    }));
}

#[inline(never)]
fn find_proc_by_ptr(name: *const u8, len: u8) -> u8 {
    if let Some(name_str) = unsafe { parse_str(name, (len as usize).min(16)) } {
        if let Some(pcb) = crate::orchestrator::proc_table_mut().find_by_name(name_str) {
            return pcb.pid.0;
        }
    }
    0
}

#[inline(never)]
fn active_slot_scan(target: u8) -> (u8, Option<&'static kernel::proc::Pcb>) {
    let mut count = 0u8;
    for slot in crate::orchestrator::proc_table().slots.iter() {
        if !matches!(
            slot.state,
            kernel::proc::ProcState::Dormant | kernel::proc::ProcState::Dead
        ) {
            if count == target {
                return (count, Some(slot));
            }
            count += 1;
        }
    }
    (count, None)
}

#[inline(never)]
fn copy_cstr(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

unsafe fn fill_sys_info(out_info: *mut KrxSysInfo) -> usize {
    if out_info.is_null() || !out_info.is_aligned() {
        return 1;
    }
    unsafe { core::ptr::write_bytes(out_info, 0, 1) };
    let out = unsafe { &mut *out_info };

    out.flash_kb = py32_hal::uid::SystemInfo::flash_size_kb();
    out.sram_kb = 16;
    if bsp::uid::is_mcu_read_protected() {
        out.flags |= KRX_SYS_INFO_FLAG_RDP_PROTECTED;
    }
    if unsafe { (*core::ptr::addr_of!(crate::CALIBRATION)).is_valid() } {
        out.flags |= KRX_SYS_INFO_FLAG_CALIB_VALID;
    }
    #[cfg(not(test))]
    let model = crate::master_config_ref().system.model_str().as_bytes();
    #[cfg(test)]
    let model = super::active_ctx_opt().map_or(b"UV-K1".as_slice(), |ctx| {
        ctx.config.system.model_str().as_bytes()
    });
    let serial64 = bsp::uid::get_hardware_serial64();
    bsp::uid::mac_from_serial64_into(serial64, &mut out.mac);
    bsp::uid::encode_crockford64_into(serial64, &mut out.serial);
    out.mcu_uid = bsp::uid::read_mcu_uid();

    let meta = &crate::build_info::FW_METADATA;
    out.fw_major = meta.version_major;
    out.fw_minor = meta.version_minor;
    out.fw_patch = meta.version_patch;
    out.target_id = meta.target_id;

    copy_cstr(&mut out.model, model);
    copy_cstr(&mut out.mcu, b"PY32F071xB");
    copy_cstr(
        &mut out.boot_ver,
        bsp::uid::bootloader_version_str().as_bytes(),
    );
    copy_cstr(&mut out.git_hash, crate::build_info::GIT_HASH.as_bytes());
    copy_cstr(
        &mut out.build_date,
        crate::build_info::BUILD_DATE.as_bytes(),
    );

    if let Some(cert) = py32_hal::uid::SystemInfo::factory_cert_slice() {
        out.flags |= KRX_SYS_INFO_FLAG_HAS_CERT;
        if let Some(qs1) = bsp::uid::extract_qs1_slice(cert) {
            out.qs1_status = KRX_QS1_UNVERIFIED;
            out.qs1_len = qs1.len() as u8;
            copy_cstr(&mut out.qs1, qs1);
        } else {
            out.qs1_status = KRX_QS1_INVALID;
        }
    }
    0
}

#[inline(never)]
fn post_event_id(event_id: u32) -> usize {
    let ev = kernel::ipc::SysEvent {
        target: (event_id >> 16) as u8,
        command: (event_id & 0xFF) as u8,
        arg: ((event_id >> 8) & 0xFF) as u16,
    };
    #[cfg(not(test))]
    {
        usize::from(crate::kern_hw().ipc_queue.try_push(ev).is_err())
    }
    #[cfg(test)]
    {
        super::active_ctx_opt().map_or(1, |ctx| usize::from(!ctx.post_event(ev)))
    }
}

#[inline(never)]
pub unsafe fn dispatch_sys(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::{idx as op, sub::sys_query};
    match idx {
        op::SysInfo => unsafe { fill_sys_info(a0 as *mut KrxSysInfo) },
        op::SysQuery => match a0 as u8 {
            sys_query::OS_VERSION => KRX_ABI_VERSION as usize,
            sys_query::STACK_FREE => {
                kernel::sys::SystemDiagnostics::stack_margin_bytes() as u16 as usize
            }
            sys_query::CPU_HZ => py32_hal::rcc::sysclk_hz() as usize,
            sys_query::DEVICE_UID => {
                let out = a1 as *mut u8;
                if !out.is_null() {
                    let uid = bsp::uid::read_mcu_uid();
                    unsafe { core::ptr::copy_nonoverlapping(uid.as_ptr(), out, 12) };
                    0
                } else {
                    1
                }
            }
            sys_query::RAND32 => {
                #[cfg(not(test))]
                let noise = crate::kern_hw()
                    .radio
                    .read_reg(drivers::bk4829::registers::REG_NOISE_VAL)
                    as u32;
                #[cfg(test)]
                let noise = super::active_ctx_opt().map_or(0, |ctx| {
                    ctx.radio
                        .read_reg(drivers::bk4829::registers::REG_NOISE_VAL)
                        as u32
                });
                let seed = py32_hal::delay::millis() ^ (noise << 16);
                crypto::crc::CrcEngine::crc32(&seed.to_le_bytes()) as usize
            }
            sys_query::CRC32 => unsafe { ptr_slice(a1 as *const u8, (a2 as u16) as usize) }
                .map_or(0, crypto::crc::CrcEngine::crc32) as usize,
            sys_query::CRC16 => unsafe { ptr_slice(a1 as *const u8, (a2 as u16) as usize) }
                .map_or(0, crypto::crc::CrcEngine::crc16_ccitt)
                as usize,
            _ => 0,
        },
        // Unsigned divide; optional remainder written to `a2`. A zero divisor
        // yields quotient 0 and remainder `n` instead of trapping.
        op::SysDiv => {
            let n = a0 as u32;
            let d = a1 as u32;
            let (q, r) = if d == 0 { (0, n) } else { (n / d, n % d) };
            let rem_out = a2 as *mut u32;
            if !rem_out.is_null() {
                unsafe { *rem_out = r };
            }
            q as usize
        }
        op::SysReboot => {
            let reboot_mode = match a0 as u8 {
                1 => kernel::power::RebootMode::Bootloader,
                2 => kernel::power::RebootMode::SafeRecovery,
                _ => kernel::power::RebootMode::Firmware,
            };
            kernel::power::PowerControl::reboot_with(reboot_mode);
        }
        // SysEvent: 0=post_event(a1), 1=publish_event(topic=a1 as u16, len=(a1>>16) as u8, data=a2)
        op::SysEvent => {
            if a0 == 0 {
                post_event_id(a1 as u32)
            } else {
                let data = a2 as *const u8;
                let len = (a1 >> 16) as u8;
                let b0 = if !data.is_null() && len > 0 {
                    unsafe { *data as u32 }
                } else {
                    0
                };
                let topic = a1 as u16;
                let ev = ((topic & 0xFF) as u32) | (b0 << 8) | (((topic >> 8) as u32) << 16);
                post_event_id(ev)
            }
        }
        op::PropGet | op::PropSet | op::PrefSet => {
            #[cfg(not(test))]
            let config = Some(crate::master_config());
            #[cfg(test)]
            let config = super::active_ctx_opt().map(|ctx| &mut *ctx.config);
            let Some(config) = config else {
                return 0;
            };
            let id = a0 as u8;
            if idx == op::PropGet {
                return config.prop_get(id) as usize;
            }
            if idx == op::PrefSet {
                return usize::from(!config.prop_set(id, a1 as u8));
            }
            let granted = crate::runtime::native_runner::active_native_caps();
            let allowed = ::abi::prop::CONTROL_CAP
                .get((id >> 5) as usize)
                .is_some_and(|&cap| granted.contains(cap));
            usize::from(!(allowed && config.control_set(id, a1 as u8)))
        }
        op::SysFlush => {
            #[cfg(not(test))]
            {
                usize::from(
                    crate::master_config()
                        .flush_dirty(crate::kern_fs())
                        .is_err(),
                )
            }
            #[cfg(test)]
            {
                super::active_ctx_opt().map_or(0, |ctx| usize::from(ctx.flush_config().is_err()))
            }
        }
        _ => 0,
    }
}

#[inline(never)]
fn proc_kill(pid: u8) -> u8 {
    let is_fg = crate::orchestrator::proc_table()
        .find(kernel::proc::Pid(pid))
        .is_some_and(|p| p.state == kernel::proc::ProcState::Running);
    if !crate::orchestrator::reap_pid_resources(pid) {
        return 1;
    }
    if is_fg {
        let ev: kernel::ipc::SysEvent = crate::apps::Shortcut::CloseApp.into();
        post_sys_event(ev.target, ev.command, ev.arg);
    }
    0
}

unsafe fn proc_ctl(op_and_id: u16, arg: u32, buf: *mut u8, len: u16) -> u8 {
    use kernel::proc::abi::*;
    use kernel::proc::{LeaseKind, Pid, ProcState};

    let op = op_and_id as u8;
    let target = (op_and_id >> 8) as u8;

    if op < 0x80 {
        if !super::super::native_runner::active_native_caps().contains(::abi::Cap::TaskControl) {
            return 2;
        }
        let table = crate::orchestrator::proc_table_mut();
        match op {
            PROC_CTL_KILL => return proc_kill(target),
            PROC_CTL_REVOKE_LEASE => {
                return LeaseKind::from_u8(target).map_or(1, |lk| {
                    u8::from(!crate::orchestrator::lease_manager_mut().revoke_kind(lk))
                });
            }
            PROC_CTL_RESET_STATS => {
                for slot in &mut table.slots {
                    slot.ledger.slice_overruns = 0;
                    slot.ledger.fault_count = 0;
                }
                return 0;
            }
            _ => {}
        }
        let Some(pcb) = table.find_mut(Pid(target)) else {
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
            PROC_CTL_FOCUS_APP => {
                if pcb.app_id >= crate::AppRegistry::app_count() {
                    return 1;
                }
                let ev: kernel::ipc::SysEvent =
                    crate::apps::Shortcut::LaunchApp(pcb.app_id as u8).into();
                post_sys_event(ev.target, ev.command, ev.arg);
            }
            _ => return 1,
        }
        return 0;
    }

    if buf.is_null() || len < 16 {
        return 1;
    }
    let mut w = [0u32; 4];
    let src: *const u8 = if op == PROC_CTL_IPC_ENTRY {
        let Some(m) = ipc_channel_mut().peek_at(target as usize) else {
            return 1;
        };
        m as *const KrxIpcMessage as *const u8
    } else {
        match op {
            PROC_CTL_SYS_OVERVIEW => {
                w[0] = (kernel::sys::SystemDiagnostics::stack_peak_used_bytes() as u32 & 0xFFFF)
                    | ((kernel::sys::SystemDiagnostics::stack_margin_bytes() as u32) << 16);
                w[1] = kernel::sys::SystemDiagnostics::ram_static_used_bytes() as u32;
            }
            PROC_CTL_LEASE_ENTRY => {
                let Some(l) = crate::orchestrator::lease_manager().entry(target as usize) else {
                    return 1;
                };
                w[0] = (l.kind.map_or(0, |k| k as u32)) | ((l.owner.0 as u32) << 8);
                w[1] = l.acquired_tick;
                w[2] = l.timeout_ms;
            }
            PROC_CTL_SVC_ENTRY => {
                let Some(e) = service_registry_mut()
                    .entries
                    .get(target as usize)
                    .filter(|e| e.is_active())
                else {
                    return 1;
                };
                w[0] = (e.interface_id as u32) | ((e.abi_version as u32) << 16);
                w[1] = (e.provider_pid.0 as u32)
                    | ((e.pinned as u32) << 8)
                    | ((e.client_count as u32) << 16);
                w[2] = e.vtable_ptr as u32;
            }
            PROC_CTL_PROC_EXT => {
                let Some(pcb) = crate::orchestrator::proc_table().find(Pid(target)) else {
                    return 1;
                };
                w[0] = pcb.caps.raw();
                w[1] = (pcb.restart_budget as u32) | ((pcb.stack_cost as u32) << 8);
                w[2] = pcb.compressed_bytes as u32;
                w[3] = pcb.ledger.ipc_events;
            }
            _ => return 1,
        }
        w.as_ptr() as *const u8
    };
    unsafe { core::ptr::copy_nonoverlapping(src, buf, 16) };
    0
}

#[inline(never)]
pub unsafe fn dispatch_proc(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    #[cfg(feature = "krx_paged")]
    use ::abi::sub::proc_paged;
    use ::abi::{idx as op, sub::proc_svc};
    match idx {
        op::ProcSelf => active_caller_pid() as usize,
        op::ProcFind => find_proc_by_ptr(a0 as *const u8, a1 as u8) as usize,
        op::ProcCount => active_slot_scan(u8::MAX).0 as usize,
        op::ProcInfo => {
            let out_info = a1 as *mut KrxProcInfo;
            if out_info.is_null() || !out_info.is_aligned() {
                return 1;
            }
            let (_, Some(slot)) = active_slot_scan(a0 as u8) else {
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
                info.name[..len].copy_from_slice(&src[..len]);
                info.runtime_ticks = slot.ledger.cpu_ticks;
                info.exec_type = slot.exec_type as u8;

                let is_core = slot.pid.0 <= 4 || slot.app_id >= 0xF0;
                let mut pflags = if is_core {
                    kernel::proc::abi::KRX_PROC_FLAG_PERSISTENT
                } else {
                    kernel::proc::abi::KRX_PROC_FLAG_KILLABLE
                };
                if slot.state == kernel::proc::ProcState::Suspended {
                    pflags |= kernel::proc::abi::KRX_PROC_FLAG_SUSPENDED;
                } else if slot.state == kernel::proc::ProcState::Running {
                    pflags |= kernel::proc::abi::KRX_PROC_FLAG_FOREGROUND;
                }
                if crate::orchestrator::lease_manager().has_owner(slot.pid) {
                    pflags |= kernel::proc::abi::KRX_PROC_FLAG_HAS_LEASE;
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
        op::ProcCtl => unsafe {
            proc_ctl(a0 as u16, a1 as u32, a2 as *mut u8, (a0 >> 16) as u16) as usize
        },
        op::ProcLeaf => super::super::native_runner::krx_leaf_call(a0 as u8, a1 as u32) as usize,
        #[cfg(feature = "krx_paged")]
        op::ProcPaged => match a0 as u8 {
            proc_paged::CALL => {
                super::super::native_runner::krx_call_paged(a1 as u8, (a1 >> 16) as u16, a2 as u16)
                    .unwrap_or(0)
            }
            proc_paged::RETURN => super::super::native_runner::krx_return_thunk().unwrap_or(0),
            proc_paged::FAULT => super::super::native_runner::krx_page_fault(a1 as u8, a2 as u16),
            _ => 0,
        },
        op::ProcLease => {
            let Some(kind) = kernel::proc::LeaseKind::from_u8(a0 as u8) else {
                return 1;
            };
            let pid = kernel::proc::Pid(active_caller_pid());
            let mgr = crate::orchestrator::lease_manager_mut();
            let res = if (a0 >> 8) != 0 {
                mgr.acquire(pid, kind, a1 as u32, py32_hal::delay::millis())
            } else {
                mgr.release(pid, kind)
            };
            usize::from(res.is_err())
        }
        op::ProcMem => {
            let out_desc = a1 as *mut kernel::proc::MemoryRegionDescriptor;
            let slot = super::super::native_runner::slot_ref();
            if out_desc.is_null() || !out_desc.is_aligned() || !slot.is_loaded {
                return 1;
            }
            if let Some(rk) = kernel::proc::RegionKind::from_u8(a0 as u8)
                && let Some(desc) = slot.query_region(rk)
            {
                unsafe { *out_desc = desc };
                return 0;
            }
            1
        }
        op::ProcSvc => {
            let sub = a0 as u8;
            let iface = (a0 >> 8) as u16;
            match sub {
                proc_svc::REGISTER => usize::from(!service_registry_mut().register(
                    iface,
                    a1 as u16,
                    kernel::proc::Pid(active_caller_pid()),
                    a2,
                    false,
                )),
                proc_svc::RESOLVE => service_registry_mut()
                    .resolve(iface, a1 as u16)
                    .unwrap_or(0),
                proc_svc::RELEASE => usize::from(!service_registry_mut().release(iface)),
                _ => 1,
            }
        }
        _ => 0,
    }
}

#[inline(never)]
unsafe fn ipc_send_msg(msg: *const KrxIpcMessage) -> u8 {
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
        post_sys_event(m.target_pid, (m.msg_type & 0xFF) as u8, m.seq);
        0
    } else {
        1
    }
}

#[inline(never)]
unsafe fn ipc_recv_msg(out_msg: *mut KrxIpcMessage) -> u8 {
    if out_msg.is_null() {
        return 1;
    }
    let receiver_pid = active_caller_pid();
    u8::from(!ipc_channel_mut().poll_into(receiver_pid, unsafe { &mut *out_msg }))
}

#[inline(never)]
unsafe fn ipc_send_raw(pids_type: u32, flags_and_seq: u32, ptr: *const u8, len: u8) -> u8 {
    let slice = unsafe { slice_or_empty(ptr, (len as usize).min(24)) };
    let mut msg = KrxIpcMessage::new(
        pids_type as u8,
        (pids_type >> 8) as u8,
        (pids_type >> 16) as u16,
        slice,
    );
    if flags_and_seq != 0 {
        msg.flags = (flags_and_seq >> 16) as u8;
        msg.seq = flags_and_seq as u16;
    }
    unsafe { ipc_send_msg(&msg) }
}

#[inline(never)]
pub unsafe fn dispatch_ipc(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::idx as op;
    match idx {
        op::IpcSend => unsafe { ipc_send_msg(a0 as *const KrxIpcMessage) as usize },
        op::IpcRecv => unsafe { ipc_recv_msg(a0 as *mut KrxIpcMessage) as usize },
        op::IpcReply => {
            let req = a0 as *const KrxIpcMessage;
            if req.is_null() {
                return 1;
            }
            let req_msg = unsafe { &*req };
            let pids_type = (req_msg.target_pid as u32)
                | ((req_msg.sender_pid as u32) << 8)
                | ((req_msg.msg_type as u32) << 16);
            let f_seq = (req_msg.seq as u32) | ((kernel::ipc::ipc_flags::REPLY as u32) << 16);
            unsafe { ipc_send_raw(pids_type, f_seq, a1 as *const u8, a2 as u8) as usize }
        }
        op::IpcPoll => {
            let out_buf = a0 as *mut u8;
            let max_len = a1 as u8;
            if out_buf.is_null() || max_len == 0 {
                return 0xFF;
            }
            let mut msg = core::mem::MaybeUninit::<KrxIpcMessage>::uninit();
            if unsafe { ipc_recv_msg(msg.as_mut_ptr()) } == 0 {
                let msg = unsafe { msg.assume_init_ref() };
                let copy_len = (msg.payload_len as usize).min(max_len as usize);
                unsafe {
                    core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), out_buf, copy_len);
                }
                copy_len
            } else {
                0xFF
            }
        }
        op::IpcSendStr => {
            let req = a0 as *const KrxIpcSendReq;
            if req.is_null() {
                return 1;
            }
            let r = unsafe { &*req };
            let target_pid =
                if let Some(slice) = unsafe { ptr_slice(r.target, r.target_len as usize) } {
                    if slice == b"*" || slice == b"ALL" {
                        ipc_target::BROADCAST
                    } else {
                        let pid = find_proc_by_ptr(r.target, r.target_len);
                        if pid == 0 {
                            return 1;
                        }
                        pid
                    }
                } else {
                    ipc_target::FOREGROUND_APP
                };
            let pids_type = (active_caller_pid() as u32)
                | ((target_pid as u32) << 8)
                | ((ipc_msg_type::APP_DATA_TRANSFER as u32) << 16);
            unsafe { ipc_send_raw(pids_type, 0, r.msg, r.msg_len) as usize }
        }
        _ => 1,
    }
}
