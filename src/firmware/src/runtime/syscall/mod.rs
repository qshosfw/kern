//! Modular Capability Router & System Call Dispatch Engine (KRX-ABI 2.5.0 Chapter 8).
//!
//! Provides the primary system call boundary between userland executables and microkernel services.
//! Implements per-capability domain VTables (`display`, `ui`, `radio`, `audio`, `storage`, `io`, `service`),
//! the dynamic `ServiceRegistry`, and the `CapImports` GOT binder (`bind_cap_imports`).

use crate::services::SysContext;
use gfx::Canvas;
use kernel::caps::{Cap, CapabilitySet};
use kernel::proc::abi::*;
use kernel::proc::krx::CapBinding;

pub mod audio;
#[cfg(any(test, feature = "settings_debug"))]
pub mod bench;
pub mod display;
pub mod io;
pub mod radio;
pub mod service;
pub mod storage;
pub mod ui;

pub use audio::{AUDIO_VTABLE, api_audio_capture_start, api_audio_play_tone};
#[cfg(any(test, feature = "settings_debug"))]
pub use bench::*;
pub use display::{
    DISPLAY_VTABLE, api_commit_frame, api_commit_rows, api_draw_line, api_draw_pixel,
    api_draw_rect, api_draw_str, api_draw_str_align, api_fill_rect, api_get_framebuffer,
};
pub use io::{HW_BUS_VTABLE, KEYPAD_VTABLE, POWER_VTABLE, SERIAL_VTABLE, TIMER_VTABLE};
pub use radio::{
    FM_VTABLE, RADIO_VTABLE, api_rf_get_freq, api_rf_get_rssi, api_rf_set_tx, api_rf_sweep,
    api_rf_tune,
};
pub use service::{
    MATH_VTABLE, SERVICE_VTABLE, ServiceRegistry, active_caller_pid, api_ipc_poll,
    api_ipc_recv_msg, api_ipc_reply, api_ipc_send, api_ipc_send_msg, api_sys_find_proc,
    api_sys_self_pid, ipc_channel_mut, service_registry_mut,
};
pub use storage::STORAGE_VTABLE;
pub use ui::{UI_VTABLE, store_modal_text};

pub static mut ACTIVE_CTX: *mut SysContext<'static, 'static> = core::ptr::null_mut();
pub static mut ACTIVE_CANVAS: *mut Canvas = core::ptr::null_mut();
pub static mut ACTIVE_SWAP: *mut () = core::ptr::null_mut();

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

#[inline(always)]
pub unsafe fn active_ctx() -> &'static mut SysContext<'static, 'static> {
    unsafe { &mut *ACTIVE_CTX }
}

#[inline(always)]
pub unsafe fn active_canvas() -> &'static mut Canvas {
    unsafe { &mut *ACTIVE_CANVAS }
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
    // System tools (File Explorers, Package Managers, Hex Editors) holding
    // Cap::StorageWrite or Cap::StorageAdmin have access across operational paths.
    // Physical protection: MCU internal flash (0x0800_0000) is memory-mapped ROM;
    // filesystem driver communicates solely with external SPI NOR flash (Py25q16).
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

// ── Composite C-ABI Dispatch Table in XIP Flash (.rodata) ───────────────────

pub static NATIVE_API_TABLE: KrxApiTable = KrxApiTable {
    abi_version: KRX_ABI_VERSION_3_0,
    table_size: core::mem::size_of::<KrxApiTable>() as u16,
    _pad0: 0,
    granted_caps: 0xFFFF_FFFF,
    display: &raw const DISPLAY_VTABLE,
    radio: &raw const RADIO_VTABLE,
    audio: &raw const AUDIO_VTABLE,
    storage: &raw const STORAGE_VTABLE,
    keypad: &raw const KEYPAD_VTABLE,
    power: &raw const POWER_VTABLE,
    serial: &raw const SERIAL_VTABLE,
    timer: &raw const TIMER_VTABLE,
    ui: &raw const UI_VTABLE,
    math: &raw const MATH_VTABLE,
    fm: &raw const FM_VTABLE,
    service: &raw const SERVICE_VTABLE,
    hw_bus: &raw const HW_BUS_VTABLE,
};

// ── Dynamic Linker / GOT Resolution ─────────────────────────────────────────

static INTERFACE_CAP_MASKS: [u32; 13] = [
    // 0x0001: INTERFACE_DISPLAY
    Cap::DisplayDirect as u32,
    // 0x0002: INTERFACE_RADIO
    (Cap::RadioRx as u32) | (Cap::RadioTx as u32) | (Cap::BroadBandRx as u32),
    // 0x0003: INTERFACE_AUDIO
    (Cap::AudioOutput as u32) | (Cap::AudioCapture as u32),
    // 0x0004: INTERFACE_STORAGE
    (Cap::StorageRead as u32) | (Cap::StorageWrite as u32) | (Cap::StorageAdmin as u32),
    // 0x0005: INTERFACE_KEYPAD
    Cap::Keypad as u32,
    // 0x0006: INTERFACE_POWER
    Cap::PowerManage as u32,
    // 0x0007: INTERFACE_SERIAL
    (Cap::SerialComm as u32) | (Cap::UsbComm as u32),
    // 0x0008: INTERFACE_TIMER
    0,
    // 0x0009: INTERFACE_UI
    Cap::DisplayDirect as u32,
    // 0x000A: INTERFACE_MATH
    0,
    // 0x000B: INTERFACE_FM
    Cap::BroadcastFm as u32,
    // 0x000C: INTERFACE_SERVICE
    0,
    // 0x000D: INTERFACE_HW_BUS
    Cap::RawPeripheral as u32,
];

#[inline(always)]
fn resolve_capability_vtable(
    interface_id: u16,
    _min_abi: u16,
    granted_caps: CapabilitySet,
) -> Option<usize> {
    let idx = interface_id.wrapping_sub(INTERFACE_DISPLAY) as usize;
    if idx < INTERFACE_CAP_MASKS.len() {
        let mask = INTERFACE_CAP_MASKS[idx];
        if mask == 0 || (granted_caps.raw() & mask) != 0 {
            let vtable =
                unsafe { *(&raw const NATIVE_API_TABLE.display as *const *const ()).add(idx) };
            return Some(vtable as usize);
        }
    }
    None
}

/// Bind `SECTION_CAP_IMPORTS (0x0003)` into the loaded package's GOT slots (Appendix C).
/// Returns `false` if any `FLAG_MANDATORY` import fails to resolve.
#[inline(never)]
pub fn bind_cap_imports(
    cap_section_body: &[u8],
    got_words: &mut [u32],
    granted_caps: CapabilitySet,
) -> bool {
    let len = cap_section_body.len();
    if len == 0 {
        return true;
    }
    let (_, r3) = utils::math::div3_u16((len as u16) >> 1);
    let stride = if (len & 7) == 0 && r3 != 0 { 8 } else { 6 };
    let mut offset = 0usize;
    let mut slot_i = 0u16;
    while offset + stride <= len {
        // SAFETY: loop guard ensures `offset + stride <= len` (stride is 6 or 8).
        let s = unsafe { cap_section_body.get_unchecked(offset..offset + stride) };
        let interface_id = u16::from_le_bytes([s[0], s[1]]);
        let flags = s[4];
        let dst_slot = if stride == 8 {
            u16::from_le_bytes([s[6], s[7]])
        } else {
            slot_i
        };
        let Some(dst) = got_words.get_mut(dst_slot as usize) else {
            return false;
        };
        if let Some(vtable_addr) = resolve_capability_vtable(interface_id, 0, granted_caps) {
            *dst = vtable_addr as u32;
        } else if (flags as u16 & CapBinding::FLAG_MANDATORY) != 0 {
            return false;
        } else {
            *dst = 0;
        }
        offset += stride;
        slot_i = slot_i.wrapping_add(1);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_cross_process_ipc_and_discovery() {
        crate::orchestrator::init_proc_table();
        let table = crate::orchestrator::proc_table_mut();
        let _ = table.spawn(
            "Kernel",
            0xF0,
            kernel::sched::Priority::Safety,
            kernel::proc::ExecType::StaticFirmware,
            kernel::caps::CapabilitySet::empty(),
        );
        let _ = table.spawn(
            "Radio",
            0xF1,
            kernel::sched::Priority::Radio,
            kernel::proc::ExecType::StaticFirmware,
            kernel::caps::CapabilitySet::empty(),
        );
        let _ = table.spawn(
            "Power",
            0xF2,
            kernel::sched::Priority::Safety,
            kernel::proc::ExecType::StaticFirmware,
            kernel::caps::CapabilitySet::empty(),
        );

        // 1. Verify process discovery via ABI
        let radio_pid = unsafe { api_sys_find_proc(b"Radio\0".as_ptr(), 5) };
        assert_eq!(radio_pid, 2);

        let power_pid = unsafe { api_sys_find_proc(b"power\0".as_ptr(), 5) };
        assert_eq!(power_pid, 3);

        let non_existent = unsafe { api_sys_find_proc(b"ghost\0".as_ptr(), 5) };
        assert_eq!(non_existent, 0);

        // 2. Structured messaging via C-ABI
        let msg = kernel::ipc::KrxIpcMessage::new(
            radio_pid,
            1,
            kernel::ipc::ipc_msg_type::SERVICE_REQUEST,
            b"GET_CONFIG",
        );
        assert_eq!(unsafe { api_ipc_send_msg(&msg) }, 0);

        // Poll as PID 1
        let mut received = kernel::ipc::KrxIpcMessage::empty();
        assert_eq!(unsafe { api_ipc_recv_msg(&mut received) }, 0);
        assert_eq!(received.sender_pid, radio_pid);
        assert_eq!(received.target_pid, 1);
        assert_eq!(
            received.msg_type,
            kernel::ipc::ipc_msg_type::SERVICE_REQUEST
        );
        assert_eq!(&received.payload[..10], b"GET_CONFIG");

        // Reply via C-ABI
        assert_eq!(
            unsafe { api_ipc_reply(&received, b"CONFIG_DATA".as_ptr(), 11) },
            0
        );

        // 3. String-based send and poll round-trip
        let send_res = unsafe { api_ipc_send(b"Radio".as_ptr(), 5, b"CMD_SET_FREQ".as_ptr(), 12) };
        assert_eq!(send_res, 0);

        // 4. Typed KrxSysInfo descriptor validation
        let mut info = core::mem::MaybeUninit::<kernel::proc::abi::KrxSysInfo>::uninit();
        assert_eq!(
            unsafe { super::service::api_sys_info(info.as_mut_ptr()) },
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
        assert!(unsafe { super::service::api_sys_info(core::ptr::null_mut()) } != 0);

        // Clear IPC channel on test completion
        ipc_channel_mut().purge_pid(radio_pid);
        ipc_channel_mut().purge_pid(1);
    }
}
