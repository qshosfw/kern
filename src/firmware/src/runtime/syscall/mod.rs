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
pub mod bench;
pub mod display;
pub mod io;
pub mod radio;
pub mod service;
pub mod storage;
pub mod ui;

pub use audio::{AUDIO_VTABLE, api_audio_capture_start, api_audio_play_tone};
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
    api_sys_self_pid, ipc_mailbox_mut, service_registry_mut,
};
pub use storage::STORAGE_VTABLE;
pub use ui::{UI_VTABLE, store_modal_text};

pub static mut ACTIVE_CTX: *mut SysContext<'static, 'static> = core::ptr::null_mut();
pub static mut ACTIVE_CANVAS: *mut Canvas = core::ptr::null_mut();
pub static mut ACTIVE_SWAP: *mut () = core::ptr::null_mut();

#[inline(always)]
pub fn active_ctx_opt() -> Option<&'static mut SysContext<'static, 'static>> {
    let ptr = unsafe { ACTIVE_CTX };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

#[inline(always)]
pub fn active_canvas_opt() -> Option<&'static mut Canvas> {
    let ptr = unsafe { ACTIVE_CANVAS };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
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
pub unsafe fn parse_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if !ptr.is_null() && len > 0 {
        let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
        core::str::from_utf8(slice).ok()
    } else {
        None
    }
}

#[inline(never)]
pub unsafe fn parse_user_path<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    let path = unsafe { parse_str(ptr, len)? };
    if path.starts_with("/sys/") {
        None
    } else {
        Some(path)
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
    draw_pixel: display::api_draw_pixel,
    draw_line: display::api_draw_line,
    draw_rect: display::api_draw_rect,
    fill_rect: display::api_fill_rect,
    draw_str: display::api_draw_str,
    draw_str_align: display::api_draw_str_align,
    commit_frame: display::api_commit_frame,
    commit_rows: display::api_commit_rows,
    get_framebuffer: display::api_get_framebuffer,
    rf_tune: radio::api_rf_tune,
    rf_get_freq: radio::api_rf_get_freq,
    rf_get_rssi: radio::api_rf_get_rssi,
    rf_set_tx: radio::api_rf_set_tx,
    rf_sweep: radio::api_rf_sweep,
    audio_play_tone: audio::api_audio_play_tone,
    audio_capture_start: audio::api_audio_capture_start,
    audio_capture_read: audio::api_audio_capture_read,
    audio_capture_stop: audio::api_audio_capture_stop,
    audio_is_capturing: audio::api_audio_is_capturing,
    rf_symbol_tx_emit: radio::api_rf_symbol_tx_emit,
    storage_load_config: storage::api_storage_load_config,
    storage_save_config: storage::api_storage_save_config,
    storage_read_asset: storage::api_storage_read_asset,
    sys_poll_key: io::api_sys_poll_key,
    sys_nav_dir: io::api_sys_nav_dir,
    sys_ticks_ms: io::api_sys_ticks_ms,
    sys_delay_ms: io::api_sys_delay_ms,
    sys_battery_mv: io::api_sys_battery_mv,
    sys_battery_pct: io::api_sys_battery_pct,
    sys_rand32: service::api_sys_rand32,
    sys_stack_free: service::api_sys_stack_free,
    hw_i2c_transfer: io::api_hw_i2c_transfer,
    hw_spi_transfer: io::api_hw_spi_transfer,
    hw_gpio_set: io::api_hw_gpio_set,
    hw_gpio_get: io::api_hw_gpio_get,
    hw_rf_read_reg: io::api_hw_rf_read_reg,
    hw_rf_write_reg: io::api_hw_rf_write_reg,
    hw_flash_read: io::api_hw_flash_read,
};

// ── Dynamic Linker / GOT Resolution ─────────────────────────────────────────

#[inline(always)]
fn resolve_capability_vtable(
    interface_id: u16,
    _min_abi: u16,
    granted_caps: CapabilitySet,
) -> Option<usize> {
    match interface_id {
        INTERFACE_DISPLAY if granted_caps.contains(Cap::DisplayDirect) => {
            Some(&raw const DISPLAY_VTABLE as usize)
        }
        INTERFACE_RADIO
            if granted_caps.contains(Cap::RadioRx)
                || granted_caps.contains(Cap::RadioTx)
                || granted_caps.contains(Cap::BroadBandRx) =>
        {
            Some(&raw const RADIO_VTABLE as usize)
        }
        INTERFACE_AUDIO
            if granted_caps.contains(Cap::AudioOutput)
                || granted_caps.contains(Cap::AudioCapture) =>
        {
            Some(&raw const AUDIO_VTABLE as usize)
        }
        INTERFACE_STORAGE
            if granted_caps.contains(Cap::StorageRead)
                || granted_caps.contains(Cap::StorageWrite) =>
        {
            Some(&raw const STORAGE_VTABLE as usize)
        }
        INTERFACE_KEYPAD if granted_caps.contains(Cap::Keypad) => {
            Some(&raw const KEYPAD_VTABLE as usize)
        }
        INTERFACE_POWER if granted_caps.contains(Cap::PowerManage) => {
            Some(&raw const POWER_VTABLE as usize)
        }
        INTERFACE_SERIAL
            if granted_caps.contains(Cap::SerialComm) || granted_caps.contains(Cap::UsbComm) =>
        {
            Some(&raw const SERIAL_VTABLE as usize)
        }
        INTERFACE_TIMER => Some(&raw const TIMER_VTABLE as usize),
        INTERFACE_UI if granted_caps.contains(Cap::DisplayDirect) => {
            Some(&raw const UI_VTABLE as usize)
        }
        INTERFACE_MATH => Some(&raw const MATH_VTABLE as usize),
        INTERFACE_FM if granted_caps.contains(Cap::BroadcastFm) => {
            Some(&raw const FM_VTABLE as usize)
        }
        INTERFACE_SERVICE => Some(&raw const SERVICE_VTABLE as usize),
        INTERFACE_HW_BUS if granted_caps.contains(Cap::RawPeripheral) => {
            Some(&raw const HW_BUS_VTABLE as usize)
        }
        _ => None,
    }
}

/// Bind `SECTION_CAP_IMPORTS (0x0003)` into the loaded package's GOT slots (Appendix C).
/// Returns `false` if any `FLAG_MANDATORY` import fails to resolve.
#[inline(never)]
pub fn bind_cap_imports(
    cap_section_body: &[u8],
    got_words: &mut [u32],
    granted_caps: CapabilitySet,
) -> bool {
    if cap_section_body.is_empty() {
        return true;
    }
    let stride = if cap_section_body.len() % 8 == 0 && cap_section_body.len() % 6 != 0 {
        CapBinding::SIZE_WITH_SLOT
    } else {
        CapBinding::SIZE
    };
    let count = cap_section_body.len() / stride;
    for i in 0..count {
        let slice = &cap_section_body[i * stride..];
        let Some(binding) = CapBinding::parse(slice, i as u16) else {
            return false;
        };
        let slot = binding.slot_idx as usize;
        if slot >= got_words.len() {
            return false;
        }
        match resolve_capability_vtable(binding.interface_id, binding.min_abi, granted_caps) {
            Some(vtable_addr) => {
                got_words[slot] = vtable_addr as u32;
            }
            None => {
                if (binding.flags & CapBinding::FLAG_MANDATORY) != 0 {
                    return false;
                }
                got_words[slot] = 0; // NULL for OPTIONAL binding
            }
        }
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

        // Clear mailbox on test completion
        ipc_mailbox_mut().purge_pid(radio_pid);
        ipc_mailbox_mut().purge_pid(1);
    }
}
