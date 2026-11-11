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

#[inline(always)]
pub fn active_ctx_opt() -> Option<&'static mut SysContext<'static, 'static>> {
    let ptr = unsafe { ACTIVE_CTX };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

#[inline(never)]
pub fn active_canvas_mut() -> &'static mut Canvas {
    let ptr = unsafe { ACTIVE_CANVAS };
    if !ptr.is_null() {
        unsafe { &mut *ptr }
    } else {
        crate::system_canvas()
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
    let caps = super::native_runner::active_native_caps();

    // Anti-Brick Lock 1: Factory Calibration Guard
    // Any mutation targeting /sys/calib.* strictly requires Cap::FactoryCalib.
    // User applications and external packages without this token are blocked.
    if path.starts_with("/sys/calib") {
        if caps.contains(Cap::FactoryCalib) {
            return Some(path);
        } else {
            return None;
        }
    }

    // System configuration manifest (/sys/*) requires Cap::FactoryCalib for modification
    if path.starts_with("/sys/") {
        if caps.contains(Cap::FactoryCalib) {
            return Some(path);
        } else {
            return None;
        }
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

// ── Golden Standard KRX Structured System Table (v3.1) ──────────────────────

pub static GFX_API: GfxApi = GfxApi {
    clear: display::api_display_clear,
    draw_pixel: display::api_draw_pixel,
    draw_line: display::api_draw_line,
    draw_rect: display::api_draw_rect,
    fill_rect: display::api_fill_rect,
    draw_bitmap: display::api_draw_bitmap,
    draw_str: display::api_draw_str,
    draw_str_align: display::api_draw_str_align,
    commit_frame: display::api_commit_frame,
    commit_rows: display::api_commit_rows,
    get_framebuffer: display::api_get_framebuffer,
    str_width: display::api_str_width,
    draw_icon: display::api_draw_icon,
    fill_rect_dither: display::api_fill_rect_dither,
    invert_rect: display::api_invert_rect,
    scroll: display::api_scroll,
    show_popup: ui::api_ui_show_popup,
    show_confirm: ui::api_ui_show_confirm,
    set_status_bar: ui::api_ui_set_status_bar,
};

pub static RADIO_API: RadioApi = RadioApi {
    tune: radio::api_rf_tune,
    get_freq: radio::api_rf_get_freq,
    get_rssi: radio::api_rf_get_rssi,
    is_carrier: radio::api_rf_is_carrier,
    set_modulation: radio::api_rf_set_modulation,
    set_squelch: radio::api_rf_set_squelch,
    set_tx: radio::api_rf_set_tx,
    set_tx_power: radio::api_rf_set_tx_power,
    set_af_mode: radio::api_rf_set_af_mode,
    sweep: radio::api_rf_sweep,
    set_subaudible_ctcss: radio::api_rf_set_subaudible_ctcss,
    set_subaudible_dcs: radio::api_rf_set_subaudible_dcs,
    fsk_send: radio::api_rf_fsk_send,
    fsk_recv: radio::api_rf_fsk_recv,
    fm_tune: radio::api_fm_tune,
    get_snr: radio::api_rf_get_snr,
};

pub static FS_API: FsApi = FsApi {
    read_file: storage::api_storage_read_file,
    read_file_at: storage::api_storage_read_file_at,
    write_file: storage::api_storage_write_file,
    append_file: storage::api_storage_append_file,
    remove_file: storage::api_storage_remove_file,
    file_size: storage::api_storage_file_size,
    file_exists: storage::api_storage_file_exists,
    list_dir: storage::api_storage_list_dir,
    create_dir: storage::api_storage_create_dir,
    rename_file: storage::api_storage_rename_file,
    load_config: storage::api_storage_load_config,
    save_config: storage::api_storage_save_config,
};

pub static AUDIO_API: AudioApi = AudioApi {
    play_tone: audio::api_audio_play_tone,
    play_beep: audio::api_audio_play_beep,
    set_path: audio::api_audio_set_path,
    set_volume: audio::api_audio_set_volume,
    get_volume: audio::api_audio_get_volume,
    capture_start: audio::api_audio_capture_start,
    capture_read: audio::api_audio_capture_read,
    capture_stop: audio::api_audio_capture_stop,
    is_capturing: audio::api_audio_is_capturing,
    stop: audio::api_audio_stop,
};

pub static SYSTEM_API: SystemApi = SystemApi {
    ticks_ms: io::api_sys_ticks_ms,
    uptime_secs: io::api_sys_uptime_secs,
    delay_ms: io::api_sys_delay_ms,
    battery_mv: io::api_sys_battery_mv,
    battery_pct: io::api_sys_battery_pct,
    is_charging: io::api_sys_is_charging,
    poll_key: io::api_sys_poll_key,
    is_key_pressed: io::api_sys_is_key_pressed,
    is_keypad_locked: io::api_sys_is_keypad_locked,
    set_backlight: io::api_sys_set_backlight,
    serial_write: io::api_serial_write,
    serial_read: io::api_serial_read,
    rand32: service::api_sys_rand32,
    crc32: service::api_math_crc32,
    ipc_send: service::api_ipc_send_msg,
    ipc_recv: service::api_ipc_recv_msg,
    ipc_reply: service::api_ipc_reply,
    self_pid: service::api_sys_self_pid,
    find_proc: service::api_sys_find_proc,
    ram_available: service::api_sys_stack_free_min,
    stack_margin: service::api_sys_stack_free,
};

pub static NATIVE_SYS_TABLE: KrxSysTable = KrxSysTable {
    abi_version: KRX_ABI_VERSION_3_1,
    table_size: core::mem::size_of::<KrxSysTable>() as u16,
    _reserved: 0,
    granted_caps: 0xFFFF_FFFF,
    gfx: &raw const GFX_API,
    radio: &raw const RADIO_API,
    fs: &raw const FS_API,
    audio: &raw const AUDIO_API,
    sys: &raw const SYSTEM_API,
};

// ── Dynamic Linker / GOT Resolution ─────────────────────────────────────────

struct InterfaceBinding {
    vtable: *const (),
    required_caps_mask: u32,
}

unsafe impl Sync for InterfaceBinding {}

static INTERFACE_TABLE: [InterfaceBinding; 13] = [
    // 0x0001: INTERFACE_DISPLAY
    InterfaceBinding {
        vtable: &raw const DISPLAY_VTABLE as *const (),
        required_caps_mask: Cap::DisplayDirect as u32,
    },
    // 0x0002: INTERFACE_RADIO
    InterfaceBinding {
        vtable: &raw const RADIO_VTABLE as *const (),
        required_caps_mask: (Cap::RadioRx as u32)
            | (Cap::RadioTx as u32)
            | (Cap::BroadBandRx as u32),
    },
    // 0x0003: INTERFACE_AUDIO
    InterfaceBinding {
        vtable: &raw const AUDIO_VTABLE as *const (),
        required_caps_mask: (Cap::AudioOutput as u32) | (Cap::AudioCapture as u32),
    },
    // 0x0004: INTERFACE_STORAGE
    InterfaceBinding {
        vtable: &raw const STORAGE_VTABLE as *const (),
        required_caps_mask: (Cap::StorageRead as u32)
            | (Cap::StorageWrite as u32)
            | (Cap::StorageAdmin as u32),
    },
    // 0x0005: INTERFACE_KEYPAD
    InterfaceBinding {
        vtable: &raw const KEYPAD_VTABLE as *const (),
        required_caps_mask: Cap::Keypad as u32,
    },
    // 0x0006: INTERFACE_POWER
    InterfaceBinding {
        vtable: &raw const POWER_VTABLE as *const (),
        required_caps_mask: Cap::PowerManage as u32,
    },
    // 0x0007: INTERFACE_SERIAL
    InterfaceBinding {
        vtable: &raw const SERIAL_VTABLE as *const (),
        required_caps_mask: (Cap::SerialComm as u32) | (Cap::UsbComm as u32),
    },
    // 0x0008: INTERFACE_TIMER
    InterfaceBinding {
        vtable: &raw const TIMER_VTABLE as *const (),
        required_caps_mask: 0,
    },
    // 0x0009: INTERFACE_UI
    InterfaceBinding {
        vtable: &raw const UI_VTABLE as *const (),
        required_caps_mask: Cap::DisplayDirect as u32,
    },
    // 0x000A: INTERFACE_MATH
    InterfaceBinding {
        vtable: &raw const MATH_VTABLE as *const (),
        required_caps_mask: 0,
    },
    // 0x000B: INTERFACE_FM
    InterfaceBinding {
        vtable: &raw const FM_VTABLE as *const (),
        required_caps_mask: Cap::BroadcastFm as u32,
    },
    // 0x000C: INTERFACE_SERVICE
    InterfaceBinding {
        vtable: &raw const SERVICE_VTABLE as *const (),
        required_caps_mask: 0,
    },
    // 0x000D: INTERFACE_HW_BUS
    InterfaceBinding {
        vtable: &raw const HW_BUS_VTABLE as *const (),
        required_caps_mask: Cap::RawPeripheral as u32,
    },
];

#[inline(always)]
fn resolve_capability_vtable(
    interface_id: u16,
    _min_abi: u16,
    granted_caps: CapabilitySet,
) -> Option<usize> {
    if (INTERFACE_DISPLAY..=INTERFACE_HW_BUS).contains(&interface_id) {
        let entry = &INTERFACE_TABLE[(interface_id - INTERFACE_DISPLAY) as usize];
        if entry.required_caps_mask == 0 || (granted_caps.raw() & entry.required_caps_mask) != 0 {
            return Some(entry.vtable as usize);
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

        // Clear IPC channel on test completion
        ipc_channel_mut().purge_pid(radio_pid);
        ipc_channel_mut().purge_pid(1);
    }
}
