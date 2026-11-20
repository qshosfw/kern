//! Unified Single-Entry System Call Router (`kern_call`).
//!
//! Routes all KRX system calls through one AAPCS-Thumb entry point (`kern_call(op, a0, a1, a2)`)
//! into 9 `#[inline(never)]` domain dispatchers (`SYS`, `PROC`, `IPC`, `FS`, `RF`, `AUD`, `GFX`, `UI`, `IO`).

#[cfg(test)]
use crate::services::SysContext;
use ::abi::Cap;
use gfx::Canvas;
use kernel::proc::abi::{KRX_ABI_VERSION_3_0, KrxApiTable};

pub mod audio;
pub mod display;
pub mod io;
pub mod radio;
pub mod service;
pub mod storage;
pub mod ui;

pub use service::{ServiceRegistry, active_caller_pid, ipc_channel_mut, service_registry_mut};
pub use ui::{set_tray_icon, store_modal_text};

#[cfg(test)]
pub static mut ACTIVE_CTX: *mut SysContext<'static, 'static> = core::ptr::null_mut();

#[cfg(test)]
#[inline(never)]
pub fn active_ctx_opt() -> Option<&'static mut SysContext<'static, 'static>> {
    let ptr = unsafe { ACTIVE_CTX };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

#[inline(always)]
pub fn active_canvas_mut() -> &'static mut Canvas {
    crate::system_canvas()
}

#[inline(never)]
pub unsafe fn ptr_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if !ptr.is_null() && len > 0 {
        Some(unsafe { core::slice::from_raw_parts(ptr, len) })
    } else {
        None
    }
}

#[inline(never)]
pub unsafe fn ptr_slice_mut<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    if !ptr.is_null() && len > 0 {
        Some(unsafe { core::slice::from_raw_parts_mut(ptr, len) })
    } else {
        None
    }
}

#[inline(never)]
pub unsafe fn slice_or_empty<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    unsafe { ptr_slice(ptr, len) }.unwrap_or(&[])
}

#[inline(never)]
pub unsafe fn parse_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    unsafe { ptr_slice(ptr, len) }.and_then(utils::format::ascii_str)
}

#[inline(never)]
pub unsafe fn parse_user_path<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    let path = unsafe { parse_str(ptr, len)? };
    if !utils::path::is_clean_path(path.as_bytes()) {
        return None;
    }
    let caps = super::native_runner::active_native_caps();

    // Anti-Brick Lock 1: Factory Calibration & System Manifest (/sys/*)
    // Strictly requires Cap::FactoryCalib.
    if utils::path::root_bit(path.as_bytes()) == utils::path::ROOT_SYS {
        return if caps.contains(Cap::FactoryCalib) {
            Some(path)
        } else {
            None
        };
    }

    // Anti-Brick Locks 2 & 3: Operational & User Filesystem (/user/*, /pref/*)
    if caps.contains(Cap::StorageWrite) || caps.contains(Cap::StorageAdmin) {
        Some(path)
    } else {
        None
    }
}

#[inline(never)]
pub unsafe fn str_or_default<'a>(ptr: *const u8, len: usize, default: &'a str) -> &'a str {
    unsafe { parse_str(ptr, len) }.unwrap_or(default)
}

// ── Single-Entry System Call Dispatcher & API Table ──────────────────────────

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kern_call(op: u32, a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::domain;
    let op_byte = op as u8;
    let idx = op_byte & 0x0F;
    unsafe {
        match op_byte >> 4 {
            domain::SYS => service::dispatch_sys(idx, a0, a1, a2),
            domain::PROC => service::dispatch_proc(idx, a0, a1, a2),
            domain::IPC => service::dispatch_ipc(idx, a0, a1, a2),
            domain::FS => storage::dispatch_fs(idx, a0, a1, a2),
            domain::RF => radio::dispatch_rf(idx, a0, a1, a2),
            domain::AUD => audio::dispatch_aud(idx, a0, a1, a2),
            domain::GFX => display::dispatch_gfx(idx, a0, a1, a2),
            domain::UI => ui::dispatch_ui(idx, a0, a1, a2),
            domain::IO => io::dispatch_io(idx, a0, a1, a2),
            _ => 0,
        }
    }
}

pub static NATIVE_API_TABLE: KrxApiTable = KrxApiTable {
    abi_version: KRX_ABI_VERSION_3_0,
    table_size: core::mem::size_of::<KrxApiTable>() as u16,
    _pad0: 0,
    granted_caps: 0xFFFF_FFFF,
    call: kern_call,
};

#[cfg(test)]
mod tests {
    use super::*;
    use ::abi::Op;
    use kernel::proc::abi::{KrxIpcSendReq, KrxSysInfo};

    #[test]
    fn test_abi_cross_process_ipc_and_discovery() {
        crate::orchestrator::init_proc_table();
        let table = crate::orchestrator::proc_table_mut();
        let _ = table.spawn(
            "Kernel",
            0xF0,
            kernel::sched::Priority::Safety,
            kernel::proc::ExecType::StaticFirmware,
            ::abi::CapabilitySet::empty(),
        );
        let _ = table.spawn(
            "Radio",
            0xF1,
            kernel::sched::Priority::Radio,
            kernel::proc::ExecType::StaticFirmware,
            ::abi::CapabilitySet::empty(),
        );
        let _ = table.spawn(
            "Power",
            0xF2,
            kernel::sched::Priority::Safety,
            kernel::proc::ExecType::StaticFirmware,
            ::abi::CapabilitySet::empty(),
        );

        let api = &NATIVE_API_TABLE;

        // 1. Verify process discovery via single-entry kern_call
        let radio_pid =
            unsafe { api.invoke(Op::ProcFind, b"Radio\0".as_ptr() as usize, 5, 0) } as u8;
        assert_eq!(radio_pid, 2);

        let power_pid =
            unsafe { api.invoke(Op::ProcFind, b"power\0".as_ptr() as usize, 5, 0) } as u8;
        assert_eq!(power_pid, 3);

        let non_existent =
            unsafe { api.invoke(Op::ProcFind, b"ghost\0".as_ptr() as usize, 5, 0) } as u8;
        assert_eq!(non_existent, 0);

        // 2. Structured messaging via kern_call
        let msg = kernel::ipc::KrxIpcMessage::new(
            radio_pid,
            1,
            kernel::ipc::ipc_msg_type::SERVICE_REQUEST,
            b"GET_CONFIG",
        );
        assert_eq!(
            unsafe { api.invoke(Op::IpcSend, &raw const msg as usize, 0, 0) },
            0
        );

        let mut received = kernel::ipc::KrxIpcMessage::empty();
        assert_eq!(
            unsafe { api.invoke(Op::IpcRecv, &raw mut received as usize, 0, 0) },
            0
        );
        assert_eq!(received.sender_pid, radio_pid);
        assert_eq!(received.target_pid, 1);
        assert_eq!(
            received.msg_type,
            kernel::ipc::ipc_msg_type::SERVICE_REQUEST
        );
        assert_eq!(&received.payload[..10], b"GET_CONFIG");

        assert_eq!(
            unsafe {
                api.invoke(
                    Op::IpcReply,
                    &raw const received as usize,
                    b"CONFIG_DATA".as_ptr() as usize,
                    11,
                )
            },
            0
        );

        // 3. String-based send round-trip
        let sreq = KrxIpcSendReq {
            target: b"Radio".as_ptr(),
            msg: b"CMD_SET_FREQ".as_ptr(),
            target_len: 5,
            msg_len: 12,
            _pad: [0; 2],
        };
        assert_eq!(
            unsafe { api.invoke(Op::IpcSendStr, &raw const sreq as usize, 0, 0) },
            0
        );

        // 4. Typed KrxSysInfo descriptor validation
        let mut info = core::mem::MaybeUninit::<KrxSysInfo>::uninit();
        assert_eq!(
            unsafe { api.invoke(Op::SysInfo, info.as_mut_ptr() as usize, 0, 0) },
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
        assert_ne!(unsafe { api.invoke(Op::SysInfo, 0, 0, 0) }, 0);

        ipc_channel_mut().purge_pid(radio_pid);
        ipc_channel_mut().purge_pid(1);
    }

    #[test]
    fn fuzz_kern_call_invalid_and_null_boundary() {
        use ::abi::domain;
        super::super::native_runner::init_native_slot();
        // Exercise invalid domains, invalid op indices, and null-pointer boundary args
        // across safe non-rebooting ops to prove zero panics or UB.
        for dom in 0u8..16 {
            for idx in 0u8..16 {
                let op = ((dom as u32) << 4) | (idx as u32);
                // Skip reboot/delay/hardware-bus ops that spin or halt on target/host.
                if op == Op::SysReboot.u32_id()
                    || dom == domain::IO
                    || dom == domain::RF
                    || dom == domain::AUD
                    || (dom == domain::PROC && idx == 4)
                {
                    continue;
                }
                let _ = unsafe { kern_call(op, 0, 0, 0) };
                let a0_is_ptr = op == Op::SysInfo.u32_id()
                    || op == Op::FsRename.u32_id()
                    || op == Op::GfxText.u32_id()
                    || op == Op::ProcFind.u32_id()
                    || dom == domain::IPC;
                if !a0_is_ptr {
                    let _ = unsafe { kern_call(op, 0xFF, 0, 0) };
                }
            }
        }
        // Misaligned destination pointers must be rejected cleanly (prevent Cortex-M0+ HardFault / UB).
        let mut raw = [0u8; core::mem::size_of::<KrxSysInfo>() + 4];
        let unaligned_ptr = unsafe { raw.as_mut_ptr().add(1) } as usize;
        assert_ne!(
            unsafe { kern_call(Op::SysInfo.u32_id(), unaligned_ptr, 0, 0) },
            0
        );
        assert_ne!(
            unsafe { kern_call(Op::ProcInfo.u32_id(), 0, unaligned_ptr, 0) },
            0
        );
        // Division by zero must never trap.
        let mut rem = 0xDEAD_BEEFu32;
        assert_eq!(
            unsafe { kern_call(Op::SysDiv.u32_id(), 42, 0, &raw mut rem as usize) },
            0
        );
        assert_eq!(rem, 42);
        assert_eq!(
            unsafe {
                kern_call(
                    Op::SysIdiv.u32_id(),
                    (-42i32) as u32 as usize,
                    0,
                    &raw mut rem as usize,
                )
            },
            0
        );
        assert_eq!(rem as i32, -42);
    }
}
