//! KRX-ABI 2.5.0 / 3.0 Modular Capability API Tables & Stable C-ABI Specification (§8).
//!
//! Defines the normative, append-only per-capability domain API tables (`DisplayApi`,
//! `RadioApi`, `AudioApi`, `StorageApi`, `KeypadApi`, `PowerApi`, `SerialApi`,
//! `TimerApi`, `UiApi`, `MathApi`, `FmApi`, `ServiceApi`, `HwBusApi`) bound via
//! `CapBinding` (`SECTION_CAP_IMPORTS = 0x0003`) into each package's GOT slots (`[r9, #slot*4]`),
//! alongside the composite `KrxApiTable` and structured `KrxSysTable` dispatch structures.
//! All calls use standard AAPCS-Thumb linkage (6–8 cycles per capability call, zero syscall trap).

pub const KRX_ABI_VERSION_3_0: u32 = 0x0003_0000;

// ── Normative Interface IDs (Appendix A: krx-idl) ───────────────────────────
// 0x0001–0x00FF reserved for kernel primitives; 0x0100+ for package services.
pub const INTERFACE_DISPLAY: u16 = 0x0001;
pub const INTERFACE_RADIO: u16 = 0x0002;
pub const INTERFACE_AUDIO: u16 = 0x0003;
pub const INTERFACE_STORAGE: u16 = 0x0004;
pub const INTERFACE_KEYPAD: u16 = 0x0005;
pub const INTERFACE_POWER: u16 = 0x0006;
pub const INTERFACE_SERIAL: u16 = 0x0007;
pub const INTERFACE_TIMER: u16 = 0x0008;
pub const INTERFACE_UI: u16 = 0x0009;
pub const INTERFACE_MATH: u16 = 0x000A;
pub const INTERFACE_FM: u16 = 0x000B;
pub const INTERFACE_SERVICE: u16 = 0x000C;
pub const INTERFACE_HW_BUS: u16 = 0x000D;

pub use crate::ipc::{KrxIpcMessage, ipc_flags, ipc_msg_type, ipc_target};

// Standard Service Package Interface IDs (Section 8.3)
pub const INTERFACE_SVC_AX25: u16 = 0x0100;
pub const INTERFACE_SVC_CW: u16 = 0x0101;
pub const INTERFACE_SVC_DTMF: u16 = 0x0102;
pub const INTERFACE_SVC_SPECTRUM: u16 = 0x0103;
pub const INTERFACE_SVC_DATA: u16 = 0x0104;
pub const INTERFACE_SVC_TRIGGER: u16 = 0x0105;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxSymbol {
    pub freq_word: u16,
    pub duration_us: u16,
}

pub const KRX_EV_NONE: u8 = 0;
pub const KRX_EV_KEY: u8 = 1;
pub const KRX_EV_ENCODER: u8 = 2;
pub const KRX_EV_TICK: u8 = 3;
pub const KRX_EV_BUS: u8 = 4;
pub const KRX_EV_USB: u8 = 5;
pub const KRX_EV_POWER: u8 = 6;
pub const KRX_EV_MODAL: u8 = 7;
pub const KRX_EV_IPC: u8 = 8;

pub const MODAL_RESULT_DISMISSED: u8 = 0;
pub const MODAL_RESULT_CONFIRMED: u8 = 1;
pub const MODAL_RESULT_TEXT_DONE: u8 = 2;
pub const MODAL_RESULT_SHEET_SELECTED: u8 = 3;
pub const MODAL_RESULT_CANCELED: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxEvent {
    pub kind: u8,
    pub key: u8,
    pub flags: u8,
    pub reserved: u8,
    pub payload: u32,
}

impl KrxEvent {
    #[inline(always)]
    pub const fn is_key(&self) -> bool {
        self.kind == KRX_EV_KEY
    }

    #[inline(always)]
    pub const fn is_encoder(&self) -> bool {
        self.kind == KRX_EV_ENCODER
    }

    #[inline(always)]
    pub const fn is_tick(&self) -> bool {
        self.kind == KRX_EV_TICK
    }

    #[inline(always)]
    pub const fn is_bus(&self) -> bool {
        self.kind == KRX_EV_BUS
    }

    #[inline(always)]
    pub const fn is_radio_bus(&self) -> bool {
        self.kind == KRX_EV_BUS && self.flags == (crate::ipc::Topic::RADIO.0 & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn is_usb(&self) -> bool {
        self.kind == KRX_EV_USB
    }

    #[inline(always)]
    pub const fn is_power(&self) -> bool {
        self.kind == KRX_EV_POWER
    }

    #[inline(always)]
    pub const fn is_modal(&self) -> bool {
        self.kind == KRX_EV_MODAL
    }

    #[inline(always)]
    pub const fn is_ipc(&self) -> bool {
        self.kind == KRX_EV_IPC
    }
}

/// Directory listing entry record (§8.2 / ORDS-01) with $O(1)$ power-of-two geometry (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxDirEntry {
    pub name: [u8; 24],
    pub size: u32,
    pub is_dir: u8,
    pub reserved: [u8; 3],
}

const _: () = assert!(core::mem::size_of::<KrxDirEntry>() == 32);

/// Process state descriptor record (§8.5 / KRX 3.2) with $O(1)$ power-of-two geometry (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxProcInfo {
    pub pid: u8,
    pub app_id: u8,
    pub state: u8,
    pub priority: u8,
    pub runtime_ticks: u32,
    pub memory_bytes: u16,
    pub name: [u8; 12],
    pub reserved: [u8; 10],
}

const _: () = assert!(core::mem::size_of::<KrxProcInfo>() == 32);

pub const KRX_OP_START: u32 = 0;
pub const KRX_OP_EVENT: u32 = 1;
pub const KRX_OP_RENDER: u32 = 2;
pub const KRX_OP_EXIT: u32 = 3;

pub type KrxNativeEntryFn =
    unsafe extern "C" fn(op: u32, api: *const KrxApiTable, arg: *const ()) -> u32;

// ── Modular Per-Capability Domain API Tables (Chapter 8.2, Append-Only) ──────

/// 1. `DisplayApi` Table (`interface_id = 0x0001`)
#[repr(C)]
pub struct DisplayApi {
    pub clear: unsafe extern "C" fn(),
    pub draw_pixel: unsafe extern "C" fn(x: i16, y: i16, color: u8),
    pub draw_line: unsafe extern "C" fn(x0: i16, y0: i16, x1: i16, y1: i16, color: u8),
    pub draw_rect: unsafe extern "C" fn(x: i16, y: i16, w: i16, h: i16, color: u8),
    pub fill_rect: unsafe extern "C" fn(x: i16, y: i16, w: i16, h: i16, color: u8),
    pub draw_bitmap: unsafe extern "C" fn(x: i16, y: i16, w: u8, h: u8, data: *const u8, len: u16),
    pub draw_str:
        unsafe extern "C" fn(x: i16, y: i16, text: *const u8, len: u16, font: u8, color: u8),
    pub draw_str_align: unsafe extern "C" fn(
        x: i16,
        y: i16,
        text: *const u8,
        len: u16,
        font: u8,
        color: u8,
        align: u8,
    ),
    pub commit_frame: unsafe extern "C" fn(),
    pub commit_rows: unsafe extern "C" fn(row_mask: u8),
    pub get_framebuffer: unsafe extern "C" fn() -> *mut u8,
    pub draw_str_max: unsafe extern "C" fn(
        x: i16,
        y: i16,
        text: *const u8,
        len: u16,
        font: u8,
        color: u8,
        max_w: i16,
    ),
    pub set_inverted: unsafe extern "C" fn(inverted: u8),
    pub set_contrast: unsafe extern "C" fn(contrast: u8),
    pub draw_line_h: unsafe extern "C" fn(x: i16, y: i16, len: i16, color: u8),
    pub draw_line_v: unsafe extern "C" fn(x: i16, y: i16, len: i16, color: u8),
    pub draw_dotted_line_h: unsafe extern "C" fn(x0: i16, x1: i16, y: i16, step: u8, color: u8),
    pub clear_corners: unsafe extern "C" fn(x: i16, y: i16, w: i16, h: i16),
    pub fill_rect_dither: unsafe extern "C" fn(x: i16, y: i16, w: i16, h: i16, progress: u8),
    pub invert_rect: unsafe extern "C" fn(x: i16, y: i16, w: i16, h: i16),
    pub str_width: unsafe extern "C" fn(text: *const u8, len: u16, font: u8) -> i16,
    pub draw_char: unsafe extern "C" fn(x: i16, y: i16, ch: u8, font: u8, color: u8) -> i16,
    pub draw_icon: unsafe extern "C" fn(x: i16, y: i16, icon_id: u8, color: u8),
    pub scroll: unsafe extern "C" fn(dx: i8, dy: i8, fill_byte: u8),
}

/// 2. `RadioApi` Table (`interface_id = 0x0002`)
#[repr(C)]
pub struct RadioApi {
    pub rf_tune: unsafe extern "C" fn(freq_hz: u32, bw: u8, step_hz: u32) -> u8,
    pub rf_get_freq: unsafe extern "C" fn() -> u32,
    pub rf_get_rssi: unsafe extern "C" fn() -> i16,
    pub rf_is_carrier: unsafe extern "C" fn() -> u8,
    pub rf_set_modulation: unsafe extern "C" fn(mod_type: u8),
    pub rf_set_squelch: unsafe extern "C" fn(level: u8),
    pub rf_set_tx: unsafe extern "C" fn(active: u8) -> u8,
    pub rf_set_tx_power: unsafe extern "C" fn(level: u8),
    pub rf_set_af_mode: unsafe extern "C" fn(mode: u8),
    pub rf_sweep:
        unsafe extern "C" fn(start_hz: u32, step_hz: u32, count: u16, out_rssi: *mut i16) -> u16,
    pub rf_symbol_tx_emit: unsafe extern "C" fn(symbols: *const KrxSymbol, count: u16) -> u8,
    pub rf_set_dual_tones:
        unsafe extern "C" fn(tone1_word: u16, tone1_gain: u8, tone2_word: u16, tone2_gain: u8),
    pub rf_fsk_init: unsafe extern "C" fn(baud: u16),
    pub rf_fsk_send: unsafe extern "C" fn(data: *const u8, len: u16) -> u8,
    pub rf_fsk_recv: unsafe extern "C" fn(buf: *mut u8, max_len: u16) -> u16,
    pub rf_set_mic_mute: unsafe extern "C" fn(muted: u8),
    pub rf_set_tx_carrier: unsafe extern "C" fn(active: u8) -> u8,
    pub rf_set_subaudible_ctcss: unsafe extern "C" fn(freq_tenths_hz: u16),
    pub rf_set_subaudible_dcs: unsafe extern "C" fn(code: u16, inverted: u8),
    pub rf_send_tail: unsafe extern "C" fn(),
    pub rf_get_bandwidth: unsafe extern "C" fn() -> u32,
    pub rf_get_modulation: unsafe extern "C" fn() -> u8,
    pub rf_get_tx_state: unsafe extern "C" fn() -> u8,
    pub rf_get_glitch: unsafe extern "C" fn() -> u8,
    pub rf_set_agc: unsafe extern "C" fn(table_idx: u8),
    pub rf_set_dual_tones_hz:
        unsafe extern "C" fn(tone1_hz: u16, tone1_gain: u8, tone2_hz: u16, tone2_gain: u8),
    pub rf_set_dtmf: unsafe extern "C" fn(digit: u8, gain: u8),
    pub rf_get_snr: unsafe extern "C" fn() -> u8,
    pub rf_get_noise: unsafe extern "C" fn() -> u8,
}

/// 3. `AudioApi` Table (`interface_id = 0x0003`)
#[repr(C)]
pub struct AudioApi {
    pub audio_play_tone: unsafe extern "C" fn(freq_hz: u16, duration_ms: u16),
    pub audio_play_beep: unsafe extern "C" fn(beep_type: u8),
    pub audio_set_path: unsafe extern "C" fn(on: u8),
    pub audio_capture_start: unsafe extern "C" fn(rate_hz: u16) -> u8,
    pub audio_capture_read: unsafe extern "C" fn(out_samples: *mut i16, max_samples: u16) -> u16,
    pub audio_capture_stop: unsafe extern "C" fn(),
    pub audio_set_volume: unsafe extern "C" fn(volume: u8),
    pub audio_get_volume: unsafe extern "C" fn() -> u8,
    pub audio_stop: unsafe extern "C" fn(),
    pub audio_is_playing: unsafe extern "C" fn() -> u8,
    pub audio_set_mic_gain: unsafe extern "C" fn(gain: u8),
    pub audio_read_mic_sample: unsafe extern "C" fn() -> u16,
    pub audio_is_capturing: unsafe extern "C" fn() -> u8,
}

/// 4. `StorageApi` Table (`interface_id = 0x0004`)
#[repr(C)]
pub struct StorageApi {
    pub storage_load_config: unsafe extern "C" fn(buf: *mut u8, max_len: u16) -> u16,
    pub storage_save_config: unsafe extern "C" fn(buf: *const u8, len: u16) -> u8,
    pub storage_read_asset:
        unsafe extern "C" fn(asset_id: u16, offset: u32, buf: *mut u8, len: u16) -> u16,
    pub storage_read_file:
        unsafe extern "C" fn(path: *const u8, path_len: u8, buf: *mut u8, max_len: u16) -> u16,
    pub storage_write_file:
        unsafe extern "C" fn(path: *const u8, path_len: u8, data: *const u8, len: u16) -> u8,
    pub storage_remove_file: unsafe extern "C" fn(path: *const u8, path_len: u8) -> u8,
    pub storage_file_size: unsafe extern "C" fn(path: *const u8, path_len: u8) -> u32,
    pub storage_list_dir: unsafe extern "C" fn(
        dir_path: *const u8,
        dir_len: u8,
        out_entries: *mut KrxDirEntry,
        max_entries: u8,
    ) -> u8,
    pub storage_file_exists: unsafe extern "C" fn(path: *const u8, path_len: u8) -> u8,
    pub storage_read_file_at: unsafe extern "C" fn(
        path: *const u8,
        path_len: u8,
        offset: u32,
        buf: *mut u8,
        max_len: u16,
    ) -> u16,
    pub storage_append_file:
        unsafe extern "C" fn(path: *const u8, path_len: u8, data: *const u8, len: u16) -> u8,
    pub storage_rename_file: unsafe extern "C" fn(
        old_path: *const u8,
        old_len: u8,
        new_path: *const u8,
        new_len: u8,
    ) -> u8,
    pub storage_create_dir: unsafe extern "C" fn(path: *const u8, path_len: u8) -> u8,
}

/// 5. `KeypadApi` Table (`interface_id = 0x0005`)
#[repr(C)]
pub struct KeypadApi {
    pub sys_poll_key: unsafe extern "C" fn() -> u8,
    pub sys_nav_dir: unsafe extern "C" fn(key: u8) -> i8,
    pub sys_key_matrix: unsafe extern "C" fn() -> u32,
    pub sys_is_key_pressed: unsafe extern "C" fn(key_code: u8) -> u8,
    pub sys_is_f_active: unsafe extern "C" fn() -> u8,
    pub sys_is_ptt_pressed: unsafe extern "C" fn() -> u8,
    pub sys_is_side1_pressed: unsafe extern "C" fn() -> u8,
    pub sys_is_side2_pressed: unsafe extern "C" fn() -> u8,
    pub sys_clear_key_buffer: unsafe extern "C" fn(),
    pub sys_is_keypad_locked: unsafe extern "C" fn() -> u8,
}

/// 6. `PowerApi` Table (`interface_id = 0x0006`)
#[repr(C)]
pub struct PowerApi {
    pub sys_battery_mv: unsafe extern "C" fn() -> u16,
    pub sys_battery_pct: unsafe extern "C" fn() -> u8,
    pub sys_battery_ma: unsafe extern "C" fn() -> u16,
    pub sys_battery_type: unsafe extern "C" fn() -> u8,
    pub sys_raw_battery_adc: unsafe extern "C" fn() -> u16,
    pub sys_mcu_temp: unsafe extern "C" fn() -> i16,
    pub sys_is_charging: unsafe extern "C" fn() -> u8,
    pub sys_is_low: unsafe extern "C" fn() -> u8,
    pub sys_is_critical: unsafe extern "C" fn() -> u8,
    pub sys_set_backlight: unsafe extern "C" fn(mode: u8, timeout_s: u8),
    pub sys_set_backlight_brightness: unsafe extern "C" fn(max: u8, min: u8),
    pub sys_wake_backlight: unsafe extern "C" fn(),
    pub sys_set_flashlight: unsafe extern "C" fn(on: u8),
    pub sys_cpu_freq_hz: unsafe extern "C" fn() -> u32,
}

/// 7. `SerialApi` Table (`interface_id = 0x0007`)
#[repr(C)]
pub struct SerialApi {
    pub serial_write: unsafe extern "C" fn(data: *const u8, len: u16) -> u16,
    pub serial_read: unsafe extern "C" fn(buf: *mut u8, max_len: u16) -> u16,
    pub serial_is_connected: unsafe extern "C" fn() -> u8,
    pub serial_flush: unsafe extern "C" fn(),
}

/// 8. `TimerApi` Table (`interface_id = 0x0008`)
#[repr(C)]
pub struct TimerApi {
    pub sys_ticks_ms: unsafe extern "C" fn() -> u32,
    pub sys_uptime_secs: unsafe extern "C" fn() -> u32,
    pub sys_delay_ms: unsafe extern "C" fn(ms: u32),
    pub sys_delay_us: unsafe extern "C" fn(us: u32),
    pub sys_rtc_get: unsafe extern "C" fn(out_time: *mut u8) -> u8,
}

pub const KRX_STATUS_VISIBLE: u8 = 1 << 0;
pub const KRX_STATUS_DIVIDER: u8 = 1 << 1;
pub const KRX_STATUS_HAS_SIGNAL: u8 = 1 << 2;
pub const KRX_STATUS_CUSTOM_TITLE: u8 = 1 << 3;

/// 64-byte ($2^6$) Power-of-Two Aligned C-ABI Status Bar Descriptor.
///
/// Supports multiple left icons and multiple center icons dynamically appended,
/// padded, and composited across foreground apps and guest executables.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxStatusBar {
    pub flags: u8,
    pub signal_level: u8,
    pub left_icon_count: u8,
    pub center_icon_count: u8,
    pub left_icons: [u8; 4],
    pub center_icons: [u8; 4],
    pub title: [u8; 16],
    pub badge: [u8; 8],
    pub center: [u8; 16],
    pub _reserved: [u8; 12],
}

impl Default for KrxStatusBar {
    fn default() -> Self {
        Self::empty()
    }
}

impl KrxStatusBar {
    pub const fn empty() -> Self {
        Self {
            flags: KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER,
            signal_level: 0,
            left_icon_count: 0,
            center_icon_count: 0,
            left_icons: [0; 4],
            center_icons: [0; 4],
            title: [0; 16],
            badge: [0; 8],
            center: [0; 16],
            _reserved: [0; 12],
        }
    }
}

/// 9. `UiApi` Table (`interface_id = 0x0009`)
#[repr(C)]
pub struct UiApi {
    pub ui_close_modal: unsafe extern "C" fn(),
    pub ui_has_modal: unsafe extern "C" fn() -> u8,
    pub ui_show_popup:
        unsafe extern "C" fn(tag: u8, title: *const u8, t_len: u8, msg: *const u8, m_len: u8),
    pub ui_show_confirm:
        unsafe extern "C" fn(tag: u8, title: *const u8, t_len: u8, msg: *const u8, m_len: u8),
    pub ui_prompt_text: unsafe extern "C" fn(
        tag: u8,
        title: *const u8,
        t_len: u8,
        initial: *const u8,
        i_len: u8,
        max_len: u8,
    ),
    pub ui_get_modal_text: unsafe extern "C" fn(buf: *mut u8, max_len: u8) -> u8,
    pub ui_show_toast: unsafe extern "C" fn(msg: *const u8, len: u8, duration_ms: u16),
    pub ui_is_animating: unsafe extern "C" fn() -> u8,
    pub ui_set_status_bar: unsafe extern "C" fn(sb: *const KrxStatusBar) -> u8,
    pub ui_get_status_bar: unsafe extern "C" fn(out_sb: *mut KrxStatusBar),
    pub ui_set_tray_icon: unsafe extern "C" fn(icon_id: u8) -> u8,
    pub ui_draw_progress_bar:
        unsafe extern "C" fn(x: i16, y: i16, w: u8, h: u8, percent: u8, border: u8),
    pub ui_draw_s_meter: unsafe extern "C" fn(x: i16, y: i16, rssi_dbm: i16, peak_hold: u8),
    pub ui_draw_tag_badge:
        unsafe extern "C" fn(x: i16, y: i16, text: *const u8, len: u8, style: u8),
    pub ui_draw_key_badge: unsafe extern "C" fn(
        x: i16,
        y: i16,
        key_str: *const u8,
        k_len: u8,
        label: *const u8,
        l_len: u8,
    ),
    pub ui_draw_capsule_button: unsafe extern "C" fn(
        x: i16,
        y: i16,
        w: u8,
        h: u8,
        label: *const u8,
        l_len: u8,
        selected: u8,
    ),
    pub ui_draw_scrollbar:
        unsafe extern "C" fn(x: u8, y: u8, h: u8, current: u16, total: u16, visible: u16),
    pub ui_draw_menu_item: unsafe extern "C" fn(
        y: i16,
        title: *const u8,
        t_len: u8,
        value: *const u8,
        v_len: u8,
        selected: u8,
        icon_id: u8,
    ),
    pub ui_draw_big_digit: unsafe extern "C" fn(x: i16, y: i16, digit: u8, color: u8),
    pub ui_draw_small_digit: unsafe extern "C" fn(x: i16, y: i16, digit: u8, color: u8),
    pub ui_draw_battery: unsafe extern "C" fn(x: i16, y: i16, pct: u8, charging: u8),
    pub ui_draw_signal: unsafe extern "C" fn(x: i16, y: i16, level: u8, color: u8),
    pub ui_draw_modal_box: unsafe extern "C" fn(w: u8, h: u8, out_origin: *mut u8),
    pub ui_show_confirm_labels: unsafe extern "C" fn(
        tag: u8,
        title: *const u8,
        t_len: u8,
        msg: *const u8,
        m_len: u8,
        confirm: *const u8,
        c_len: u8,
        cancel: *const u8,
        cl_len: u8,
    ),
}

/// 10. `MathApi` Table (`interface_id = 0x000A`)
#[repr(C)]
pub struct MathApi {
    pub sys_rand32: unsafe extern "C" fn() -> u32,
    pub crc32: unsafe extern "C" fn(data: *const u8, len: u16) -> u32,
    pub crc16: unsafe extern "C" fn(data: *const u8, len: u16) -> u16,
    pub idivmod: unsafe extern "C" fn(n: i32, d: i32) -> u64,
    pub uidivmod: unsafe extern "C" fn(n: u32, d: u32) -> u64,
    pub isqrt: unsafe extern "C" fn(val: u32) -> u32,
}

/// 11. `FmApi` Table (`interface_id = 0x000B`)
#[repr(C)]
pub struct FmApi {
    pub fm_tune: unsafe extern "C" fn(freq_100khz: u16),
    pub fm_set_volume: unsafe extern "C" fn(volume: u8),
    pub fm_mute: unsafe extern "C" fn(mute: u8),
    pub fm_get_rssi: unsafe extern "C" fn() -> u8,
}

/// 12. `ServiceApi` Table (`interface_id = 0x000C`)
#[repr(C)]
pub struct ServiceApi {
    pub svc_register:
        unsafe extern "C" fn(interface_id: u16, abi_version: u16, vtable_ptr: usize) -> u8,
    pub svc_resolve: unsafe extern "C" fn(interface_id: u16, min_abi: u16) -> usize,
    pub svc_release: unsafe extern "C" fn(interface_id: u16) -> u8,
    pub ipc_send: unsafe extern "C" fn(
        target_name: *const u8,
        name_len: u8,
        msg: *const u8,
        msg_len: u8,
    ) -> u8,
    pub ipc_poll: unsafe extern "C" fn(out_buf: *mut u8, max_len: u8) -> u8,
    pub sys_device_uid: unsafe extern "C" fn(out_uid12: *mut u8),
    pub sys_os_version: unsafe extern "C" fn() -> u32,
    pub sys_ram_available: unsafe extern "C" fn() -> u16,
    pub sys_stack_margin: unsafe extern "C" fn() -> u16,
    pub sys_proc_count: unsafe extern "C" fn() -> u8,
    pub sys_proc_info: unsafe extern "C" fn(idx: u8, out_info: *mut KrxProcInfo) -> u8,
    pub sys_proc_kill: unsafe extern "C" fn(pid: u8) -> u8,
    pub paged_call:
        unsafe extern "C" fn(target_block_id: u8, target_offset: u16, return_offset: u16) -> usize,
    pub paged_return: unsafe extern "C" fn() -> usize,
    pub leaf_call: unsafe extern "C" fn(leaf_id: u8, arg: u32) -> u32,
    pub lease_acquire: unsafe extern "C" fn(lease_kind: u8, timeout_ms: u32) -> u8,
    pub lease_release: unsafe extern "C" fn(lease_kind: u8) -> u8,
    pub mem_query:
        unsafe extern "C" fn(kind: u8, out_desc: *mut crate::proc::MemoryRegionDescriptor) -> u8,
    pub sys_post_event: unsafe extern "C" fn(event_id: u32) -> u8,
    pub sys_publish_event: unsafe extern "C" fn(topic: u16, data: *const u8, len: u8) -> u8,
    pub sys_stack_free_min: unsafe extern "C" fn() -> u16,
    pub ipc_send_msg: unsafe extern "C" fn(msg: *const KrxIpcMessage) -> u8,
    pub ipc_recv_msg: unsafe extern "C" fn(out_msg: *mut KrxIpcMessage) -> u8,
    pub ipc_reply:
        unsafe extern "C" fn(req: *const KrxIpcMessage, payload: *const u8, len: u8) -> u8,
    pub sys_find_proc: unsafe extern "C" fn(name: *const u8, len: u8) -> u8,
    pub sys_self_pid: unsafe extern "C" fn() -> u8,
}

/// 13. `HwBusApi` Table (`interface_id = 0x000D`)
#[repr(C)]
pub struct HwBusApi {
    pub hw_i2c_transfer: unsafe extern "C" fn(
        addr: u8,
        reg: u8,
        tx: *const u8,
        tx_len: u16,
        rx: *mut u8,
        rx_len: u16,
    ) -> u8,
    pub hw_spi_transfer:
        unsafe extern "C" fn(cs_pin: u8, tx: *const u8, rx: *mut u8, len: u16) -> u8,
    pub hw_gpio_set: unsafe extern "C" fn(pin: u8, high: u8),
    pub hw_gpio_get: unsafe extern "C" fn(pin: u8) -> u8,
    pub hw_rf_read_reg: unsafe extern "C" fn(reg: u8) -> u16,
    pub hw_rf_write_reg: unsafe extern "C" fn(reg: u8, val: u16),
    pub hw_flash_read: unsafe extern "C" fn(addr: u32, buf: *mut u8, len: u16) -> u8,
}

/// Composite C-ABI Dispatch Table passed in `r1` to `KrxNativeEntryFn` (§8.2 / KRX-ABI 2.5.0)
#[repr(C)]
pub struct KrxApiTable {
    pub abi_version: u32,
    pub table_size: u16,
    pub _pad0: u16,
    pub granted_caps: u32,

    // Modular Capability Domain API Table Pointers (§8.2 / KRX-ABI 2.5.0)
    pub display: *const DisplayApi,
    pub radio: *const RadioApi,
    pub audio: *const AudioApi,
    pub storage: *const StorageApi,
    pub keypad: *const KeypadApi,
    pub power: *const PowerApi,
    pub serial: *const SerialApi,
    pub timer: *const TimerApi,
    pub ui: *const UiApi,
    pub math: *const MathApi,
    pub fm: *const FmApi,
    pub service: *const ServiceApi,
    pub hw_bus: *const HwBusApi,
}

unsafe impl Sync for KrxApiTable {}
unsafe impl Send for KrxApiTable {}

// ── KRX Structured System ABI Specification (v3.1) ──────────────────────────

pub const KRX_ABI_VERSION_3_1: u32 = 0x0003_0100;

/// Normative KRX Structured System Dispatch Table (§4.2 / KRX-ABI 3.1)
///
/// Passed in `r1` to `KrxNativeEntryFn` for zero-overhead, $O(1)$ AAPCS-Thumb linkage.
/// Fits in exactly 64 bytes ($2^6$ on 32-bit Cortex-M0+), power-of-two aligned to flash erase sectors.
/// Exposes 13 discrete subsystem pointers matching the microkernel primitives 1:1.
#[repr(C)]
pub struct KrxSysTable {
    pub abi_version: u32,
    pub table_size: u16,
    pub _reserved: u16,
    pub granted_caps: u32,

    pub display: *const DisplayApi,
    pub radio: *const RadioApi,
    pub audio: *const AudioApi,
    pub storage: *const StorageApi,
    pub keypad: *const KeypadApi,
    pub power: *const PowerApi,
    pub serial: *const SerialApi,
    pub timer: *const TimerApi,
    pub ui: *const UiApi,
    pub math: *const MathApi,
    pub fm: *const FmApi,
    pub service: *const ServiceApi,
    pub hw_bus: *const HwBusApi,
}

unsafe impl Sync for KrxSysTable {}
unsafe impl Send for KrxSysTable {}

// ── Safe Fallback Stubs ──────────────────────────────────────────────────────

unsafe extern "C" fn noop_draw_pixel(_x: i16, _y: i16, _color: u8) {}
unsafe extern "C" fn noop_draw_line(_x0: i16, _y0: i16, _x1: i16, _y1: i16, _color: u8) {}
unsafe extern "C" fn noop_draw_rect(_x: i16, _y: i16, _w: i16, _h: i16, _color: u8) {}
unsafe extern "C" fn noop_fill_rect(_x: i16, _y: i16, _w: i16, _h: i16, _color: u8) {}
unsafe extern "C" fn noop_draw_bitmap(
    _x: i16,
    _y: i16,
    _w: u8,
    _h: u8,
    _data: *const u8,
    _len: u16,
) {
}
unsafe extern "C" fn noop_draw_str(
    _x: i16,
    _y: i16,
    _text: *const u8,
    _len: u16,
    _font: u8,
    _color: u8,
) {
}
unsafe extern "C" fn noop_draw_str_align(
    _x: i16,
    _y: i16,
    _text: *const u8,
    _len: u16,
    _font: u8,
    _color: u8,
    _align: u8,
) {
}
unsafe extern "C" fn noop_commit_frame() {}
unsafe extern "C" fn noop_commit_rows(_row_mask: u8) {}
unsafe extern "C" fn noop_get_framebuffer() -> *mut u8 {
    core::ptr::null_mut()
}
unsafe extern "C" fn noop_draw_str_max(
    _x: i16,
    _y: i16,
    _text: *const u8,
    _len: u16,
    _font: u8,
    _color: u8,
    _max_w: i16,
) {
}
unsafe extern "C" fn noop_set_inverted(_inverted: u8) {}
unsafe extern "C" fn noop_set_contrast(_contrast: u8) {}
unsafe extern "C" fn noop_draw_line_h(_x: i16, _y: i16, _len: i16, _color: u8) {}
unsafe extern "C" fn noop_draw_line_v(_x: i16, _y: i16, _len: i16, _color: u8) {}
unsafe extern "C" fn noop_draw_dotted_line_h(_x0: i16, _x1: i16, _y: i16, _step: u8, _color: u8) {}
unsafe extern "C" fn noop_clear_corners(_x: i16, _y: i16, _w: i16, _h: i16) {}
unsafe extern "C" fn noop_fill_rect_dither(_x: i16, _y: i16, _w: i16, _h: i16, _progress: u8) {}
unsafe extern "C" fn noop_invert_rect(_x: i16, _y: i16, _w: i16, _h: i16) {}
unsafe extern "C" fn noop_str_width(_text: *const u8, _len: u16, _font: u8) -> i16 {
    0
}
unsafe extern "C" fn noop_draw_char(_x: i16, _y: i16, _ch: u8, _font: u8, _color: u8) -> i16 {
    6
}
unsafe extern "C" fn noop_draw_icon(_x: i16, _y: i16, _icon_id: u8, _color: u8) {}
unsafe extern "C" fn noop_scroll(_dx: i8, _dy: i8, _fill_byte: u8) {}

unsafe extern "C" fn noop_rf_tune(_freq_hz: u32, _bw: u8, _step_hz: u32) -> u8 {
    1
}
unsafe extern "C" fn noop_rf_get_freq() -> u32 {
    0
}
unsafe extern "C" fn noop_rf_get_rssi() -> i16 {
    -140
}
unsafe extern "C" fn noop_rf_is_carrier() -> u8 {
    0
}
unsafe extern "C" fn noop_rf_set_modulation(_mod_type: u8) {}
unsafe extern "C" fn noop_rf_set_squelch(_level: u8) {}
unsafe extern "C" fn noop_rf_set_tx(_active: u8) -> u8 {
    1
}
unsafe extern "C" fn noop_rf_set_tx_power(_level: u8) {}
unsafe extern "C" fn noop_rf_set_af_mode(_mode: u8) {}
unsafe extern "C" fn noop_rf_sweep(
    _start_hz: u32,
    _step_hz: u32,
    _count: u16,
    _out_rssi: *mut i16,
) -> u16 {
    0
}
unsafe extern "C" fn noop_rf_symbol_tx_emit(_symbols: *const KrxSymbol, _count: u16) -> u8 {
    1
}
unsafe extern "C" fn noop_rf_set_dual_tones(_t1_w: u16, _t1_g: u8, _t2_w: u16, _t2_g: u8) {}
unsafe extern "C" fn noop_rf_fsk_init(_baud: u16) {}
unsafe extern "C" fn noop_rf_fsk_send(_data: *const u8, _len: u16) -> u8 {
    1
}
unsafe extern "C" fn noop_rf_fsk_recv(_buf: *mut u8, _max_len: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_rf_mute(_muted: u8) {}
unsafe extern "C" fn noop_rf_set_subaudible_ctcss(_freq_tenths_hz: u16) {}
unsafe extern "C" fn noop_rf_set_subaudible_dcs(_code: u16, _inverted: u8) {}
unsafe extern "C" fn noop_rf_send_tail() {}
unsafe extern "C" fn noop_rf_get_bandwidth() -> u32 {
    12_500
}
unsafe extern "C" fn noop_rf_get_modulation() -> u8 {
    0
}
unsafe extern "C" fn noop_rf_get_tx_state() -> u8 {
    0
}
unsafe extern "C" fn noop_rf_get_glitch() -> u8 {
    0
}
unsafe extern "C" fn noop_rf_set_agc(_table_idx: u8) {}

unsafe extern "C" fn noop_audio_play_tone(_freq_hz: u16, _duration_ms: u16) {}
unsafe extern "C" fn noop_audio_play_beep(_beep_type: u8) {}
unsafe extern "C" fn noop_audio_set_path(_on: u8) {}
unsafe extern "C" fn noop_audio_capture_start(_rate_hz: u16) -> u8 {
    1
}
unsafe extern "C" fn noop_audio_capture_read(_out_samples: *mut i16, _max_samples: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_audio_capture_stop() {}
unsafe extern "C" fn noop_audio_is_capturing() -> u8 {
    0
}
unsafe extern "C" fn noop_audio_set_volume(_volume: u8) {}
unsafe extern "C" fn noop_audio_get_volume() -> u8 {
    5
}
unsafe extern "C" fn noop_audio_stop() {}
unsafe extern "C" fn noop_audio_is_playing() -> u8 {
    0
}
unsafe extern "C" fn noop_audio_set_mic_gain(_gain: u8) {}
unsafe extern "C" fn noop_audio_read_mic_sample() -> u16 {
    0
}

unsafe extern "C" fn noop_storage_load_config(_buf: *mut u8, _max_len: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_storage_save_config(_buf: *const u8, _len: u16) -> u8 {
    1
}
unsafe extern "C" fn noop_storage_read_asset(
    _asset_id: u16,
    _offset: u32,
    _buf: *mut u8,
    _len: u16,
) -> u16 {
    0
}
unsafe extern "C" fn noop_storage_read_file(
    _path: *const u8,
    _path_len: u8,
    _buf: *mut u8,
    _max_len: u16,
) -> u16 {
    0
}
unsafe extern "C" fn noop_storage_write_file(
    _path: *const u8,
    _path_len: u8,
    _data: *const u8,
    _len: u16,
) -> u8 {
    1
}
unsafe extern "C" fn noop_storage_remove_file(_path: *const u8, _path_len: u8) -> u8 {
    1
}
unsafe extern "C" fn noop_storage_file_size(_path: *const u8, _path_len: u8) -> u32 {
    0
}
unsafe extern "C" fn noop_storage_list_dir(
    _dir_path: *const u8,
    _dir_len: u8,
    _out_entries: *mut KrxDirEntry,
    _max_entries: u8,
) -> u8 {
    0
}
unsafe extern "C" fn noop_storage_file_exists(_path: *const u8, _path_len: u8) -> u8 {
    0
}
unsafe extern "C" fn noop_storage_read_file_at(
    _path: *const u8,
    _path_len: u8,
    _offset: u32,
    _buf: *mut u8,
    _max_len: u16,
) -> u16 {
    0
}
unsafe extern "C" fn noop_storage_append_file(
    _path: *const u8,
    _path_len: u8,
    _data: *const u8,
    _len: u16,
) -> u8 {
    1
}

unsafe extern "C" fn noop_sys_poll_key() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_nav_dir(_key: u8) -> i8 {
    0
}
unsafe extern "C" fn noop_sys_key_matrix() -> u32 {
    0
}
unsafe extern "C" fn noop_sys_is_key_pressed(_key_code: u8) -> u8 {
    0
}
unsafe extern "C" fn noop_sys_is_f_active() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_is_ptt_pressed() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_is_side1_pressed() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_is_side2_pressed() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_clear_key_buffer() {}

unsafe extern "C" fn noop_sys_ticks_ms() -> u32 {
    0
}
unsafe extern "C" fn noop_sys_uptime_secs() -> u32 {
    0
}
unsafe extern "C" fn noop_sys_delay_ms(_ms: u32) {}
unsafe extern "C" fn noop_sys_delay_us(_us: u32) {}
unsafe extern "C" fn noop_sys_battery_mv() -> u16 {
    4000
}
unsafe extern "C" fn noop_sys_battery_pct() -> u8 {
    100
}
unsafe extern "C" fn noop_sys_battery_ma() -> u16 {
    0
}
unsafe extern "C" fn noop_sys_battery_type() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_raw_battery_adc() -> u16 {
    0
}
unsafe extern "C" fn noop_sys_mcu_temp() -> i16 {
    250
}
unsafe extern "C" fn noop_sys_is_charging() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_is_low() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_is_critical() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_set_backlight(_mode: u8, _timeout_s: u8) {}
unsafe extern "C" fn noop_sys_set_backlight_brightness(_max: u8, _min: u8) {}
unsafe extern "C" fn noop_sys_wake_backlight() {}
unsafe extern "C" fn noop_sys_set_flashlight(_on: u8) {}

unsafe extern "C" fn noop_serial_write(_data: *const u8, _len: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_serial_read(_buf: *mut u8, _max_len: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_serial_is_connected() -> u8 {
    0
}
unsafe extern "C" fn noop_serial_flush() {}

unsafe extern "C" fn noop_ui_close_modal() {}
unsafe extern "C" fn noop_ui_has_modal() -> u8 {
    0
}
unsafe extern "C" fn noop_ui_show_popup(
    _tag: u8,
    _title: *const u8,
    _t_len: u8,
    _msg: *const u8,
    _m_len: u8,
) {
}
unsafe extern "C" fn noop_ui_show_confirm(
    _tag: u8,
    _title: *const u8,
    _t_len: u8,
    _msg: *const u8,
    _m_len: u8,
) {
}
unsafe extern "C" fn noop_ui_prompt_text(
    _tag: u8,
    _title: *const u8,
    _t_len: u8,
    _initial: *const u8,
    _i_len: u8,
    _max_len: u8,
) {
}
unsafe extern "C" fn noop_ui_get_modal_text(_buf: *mut u8, _max_len: u8) -> u8 {
    0
}
unsafe extern "C" fn noop_ui_show_toast(_msg: *const u8, _len: u8, _duration_ms: u16) {}
unsafe extern "C" fn noop_ui_is_animating() -> u8 {
    0
}
unsafe extern "C" fn noop_ui_set_status_bar(_sb: *const KrxStatusBar) -> u8 {
    1
}
unsafe extern "C" fn noop_ui_get_status_bar(out_sb: *mut KrxStatusBar) {
    if !out_sb.is_null() {
        unsafe {
            *out_sb = KrxStatusBar::empty();
        }
    }
}
unsafe extern "C" fn noop_ui_set_tray_icon(_icon_id: u8) -> u8 {
    1
}
unsafe extern "C" fn noop_ui_draw_progress_bar(
    _x: i16,
    _y: i16,
    _w: u8,
    _h: u8,
    _percent: u8,
    _border: u8,
) {
}
unsafe extern "C" fn noop_ui_draw_s_meter(_x: i16, _y: i16, _rssi_dbm: i16, _peak_hold: u8) {}
unsafe extern "C" fn noop_ui_draw_tag_badge(
    _x: i16,
    _y: i16,
    _text: *const u8,
    _len: u8,
    _style: u8,
) {
}
unsafe extern "C" fn noop_ui_draw_key_badge(
    _x: i16,
    _y: i16,
    _key_str: *const u8,
    _k_len: u8,
    _label: *const u8,
    _l_len: u8,
) {
}
unsafe extern "C" fn noop_ui_draw_capsule_button(
    _x: i16,
    _y: i16,
    _w: u8,
    _h: u8,
    _label: *const u8,
    _l_len: u8,
    _selected: u8,
) {
}
unsafe extern "C" fn noop_ui_draw_scrollbar(
    _x: u8,
    _y: u8,
    _h: u8,
    _current: u16,
    _total: u16,
    _visible: u16,
) {
}
unsafe extern "C" fn noop_ui_draw_menu_item(
    _y: i16,
    _title: *const u8,
    _t_len: u8,
    _value: *const u8,
    _v_len: u8,
    _selected: u8,
    _icon_id: u8,
) {
}
unsafe extern "C" fn noop_ui_draw_big_digit(_x: i16, _y: i16, _digit: u8, _color: u8) {}
unsafe extern "C" fn noop_ui_draw_small_digit(_x: i16, _y: i16, _digit: u8, _color: u8) {}
unsafe extern "C" fn noop_ui_draw_battery(_x: i16, _y: i16, _pct: u8, _charging: u8) {}
unsafe extern "C" fn noop_ui_draw_signal(_x: i16, _y: i16, _level: u8, _color: u8) {}
unsafe extern "C" fn noop_ui_draw_modal_box(_w: u8, _h: u8, _out_origin: *mut u8) {}
unsafe extern "C" fn noop_ui_show_confirm_labels(
    _tag: u8,
    _title: *const u8,
    _t_len: u8,
    _msg: *const u8,
    _m_len: u8,
    _confirm: *const u8,
    _c_len: u8,
    _cancel: *const u8,
    _cl_len: u8,
) {
}

unsafe extern "C" fn noop_sys_rand32() -> u32 {
    0x5A5A_5A5A
}
unsafe extern "C" fn noop_crc32(_data: *const u8, _len: u16) -> u32 {
    0
}
unsafe extern "C" fn noop_crc16(_data: *const u8, _len: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_idivmod(_n: i32, _d: i32) -> u64 {
    0
}
unsafe extern "C" fn noop_uidivmod(_n: u32, _d: u32) -> u64 {
    0
}
unsafe extern "C" fn noop_isqrt(val: u32) -> u32 {
    isqrt(val)
}

unsafe extern "C" fn noop_fm_tune(_freq_100khz: u16) {}
unsafe extern "C" fn noop_fm_set_volume(_volume: u8) {}
unsafe extern "C" fn noop_fm_mute(_mute: u8) {}
unsafe extern "C" fn noop_fm_get_rssi() -> u8 {
    0
}

unsafe extern "C" fn noop_svc_register(
    _interface_id: u16,
    _abi_version: u16,
    _vtable_ptr: usize,
) -> u8 {
    1
}
unsafe extern "C" fn noop_svc_resolve(_interface_id: u16, _min_abi: u16) -> usize {
    0
}
unsafe extern "C" fn noop_svc_release(_interface_id: u16) -> u8 {
    1
}
unsafe extern "C" fn noop_ipc_send(
    _target_name: *const u8,
    _name_len: u8,
    _msg: *const u8,
    _msg_len: u8,
) -> u8 {
    1
}
unsafe extern "C" fn noop_ipc_poll(_out_buf: *mut u8, _max_len: u8) -> u8 {
    0xFF
}
unsafe extern "C" fn noop_sys_device_uid(_out_uid12: *mut u8) {}
unsafe extern "C" fn noop_sys_os_version() -> u32 {
    KRX_ABI_VERSION_3_0
}
unsafe extern "C" fn noop_sys_ram_available() -> u16 {
    4096
}
unsafe extern "C" fn noop_sys_stack_margin() -> u16 {
    1024
}
unsafe extern "C" fn noop_sys_proc_count() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_proc_info(_idx: u8, _out_info: *mut KrxProcInfo) -> u8 {
    1
}
unsafe extern "C" fn noop_sys_proc_kill(_pid: u8) -> u8 {
    1
}
unsafe extern "C" fn noop_paged_call(
    _target_block_id: u8,
    _target_offset: u16,
    _return_offset: u16,
) -> usize {
    0
}
unsafe extern "C" fn noop_paged_return() -> usize {
    0
}
unsafe extern "C" fn noop_leaf_call(_leaf_id: u8, _arg: u32) -> u32 {
    0xFFFF_FFFF
}
unsafe extern "C" fn noop_lease_acquire(_lease_kind: u8, _timeout_ms: u32) -> u8 {
    1
}
unsafe extern "C" fn noop_lease_release(_lease_kind: u8) -> u8 {
    1
}
unsafe extern "C" fn noop_mem_query(
    _kind: u8,
    _out_desc: *mut crate::proc::MemoryRegionDescriptor,
) -> u8 {
    1
}
unsafe extern "C" fn noop_sys_post_event(_event_id: u32) -> u8 {
    0
}
unsafe extern "C" fn noop_sys_publish_event(_topic: u16, _data: *const u8, _len: u8) -> u8 {
    0
}
unsafe extern "C" fn noop_sys_stack_free_min() -> u16 {
    1024
}
unsafe extern "C" fn noop_ipc_send_msg(_msg: *const KrxIpcMessage) -> u8 {
    1
}
unsafe extern "C" fn noop_ipc_recv_msg(_out_msg: *mut KrxIpcMessage) -> u8 {
    1
}
unsafe extern "C" fn noop_ipc_reply(
    _req: *const KrxIpcMessage,
    _payload: *const u8,
    _len: u8,
) -> u8 {
    1
}
unsafe extern "C" fn noop_sys_find_proc(_name: *const u8, _len: u8) -> u8 {
    0
}
unsafe extern "C" fn noop_sys_self_pid() -> u8 {
    1
}

unsafe extern "C" fn noop_hw_i2c_transfer(
    _addr: u8,
    _reg: u8,
    _tx: *const u8,
    _tx_len: u16,
    _rx: *mut u8,
    _rx_len: u16,
) -> u8 {
    1
}
unsafe extern "C" fn noop_hw_spi_transfer(
    _cs_pin: u8,
    _tx: *const u8,
    _rx: *mut u8,
    _len: u16,
) -> u8 {
    1
}
unsafe extern "C" fn noop_hw_gpio_set(_pin: u8, _high: u8) {}
unsafe extern "C" fn noop_hw_gpio_get(_pin: u8) -> u8 {
    0
}
unsafe extern "C" fn noop_hw_rf_read_reg(_reg: u8) -> u16 {
    0
}
unsafe extern "C" fn noop_hw_rf_write_reg(_reg: u8, _val: u16) {}
unsafe extern "C" fn noop_hw_flash_read(_addr: u32, _buf: *mut u8, _len: u16) -> u8 {
    1
}
unsafe extern "C" fn noop_rf_set_dual_tones_hz(
    _tone1_hz: u16,
    _tone1_gain: u8,
    _tone2_hz: u16,
    _tone2_gain: u8,
) {
}
unsafe extern "C" fn noop_rf_set_dtmf(_digit: u8, _gain: u8) {}
unsafe extern "C" fn noop_rf_get_snr() -> u8 {
    0
}
unsafe extern "C" fn noop_rf_get_noise() -> u8 {
    0
}
unsafe extern "C" fn noop_storage_rename_file(
    _old_path: *const u8,
    _old_len: u8,
    _new_path: *const u8,
    _new_len: u8,
) -> u8 {
    1
}
unsafe extern "C" fn noop_storage_create_dir(_path: *const u8, _path_len: u8) -> u8 {
    1
}
unsafe extern "C" fn noop_sys_is_keypad_locked() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_cpu_freq_hz() -> u32 {
    48_000_000
}
unsafe extern "C" fn noop_sys_rtc_get(_out_time: *mut u8) -> u8 {
    1
}

pub static STUB_DISPLAY_VTABLE: DisplayApi = DisplayApi {
    clear: noop_commit_frame,
    draw_pixel: noop_draw_pixel,
    draw_line: noop_draw_line,
    draw_rect: noop_draw_rect,
    fill_rect: noop_fill_rect,
    draw_bitmap: noop_draw_bitmap,
    draw_str: noop_draw_str,
    draw_str_align: noop_draw_str_align,
    commit_frame: noop_commit_frame,
    commit_rows: noop_commit_rows,
    get_framebuffer: noop_get_framebuffer,
    draw_str_max: noop_draw_str_max,
    set_inverted: noop_set_inverted,
    set_contrast: noop_set_contrast,
    draw_line_h: noop_draw_line_h,
    draw_line_v: noop_draw_line_v,
    draw_dotted_line_h: noop_draw_dotted_line_h,
    clear_corners: noop_clear_corners,
    fill_rect_dither: noop_fill_rect_dither,
    invert_rect: noop_invert_rect,
    str_width: noop_str_width,
    draw_char: noop_draw_char,
    draw_icon: noop_draw_icon,
    scroll: noop_scroll,
};

pub static STUB_RADIO_VTABLE: RadioApi = RadioApi {
    rf_tune: noop_rf_tune,
    rf_get_freq: noop_rf_get_freq,
    rf_get_rssi: noop_rf_get_rssi,
    rf_is_carrier: noop_rf_is_carrier,
    rf_set_modulation: noop_rf_set_modulation,
    rf_set_squelch: noop_rf_set_squelch,
    rf_set_tx: noop_rf_set_tx,
    rf_set_tx_power: noop_rf_set_tx_power,
    rf_set_af_mode: noop_rf_set_af_mode,
    rf_sweep: noop_rf_sweep,
    rf_symbol_tx_emit: noop_rf_symbol_tx_emit,
    rf_set_dual_tones: noop_rf_set_dual_tones,
    rf_fsk_init: noop_rf_fsk_init,
    rf_fsk_send: noop_rf_fsk_send,
    rf_fsk_recv: noop_rf_fsk_recv,
    rf_set_mic_mute: noop_rf_mute,
    rf_set_tx_carrier: noop_rf_set_tx,
    rf_set_subaudible_ctcss: noop_rf_set_subaudible_ctcss,
    rf_set_subaudible_dcs: noop_rf_set_subaudible_dcs,
    rf_send_tail: noop_rf_send_tail,
    rf_get_bandwidth: noop_rf_get_bandwidth,
    rf_get_modulation: noop_rf_get_modulation,
    rf_get_tx_state: noop_rf_get_tx_state,
    rf_get_glitch: noop_rf_get_glitch,
    rf_set_agc: noop_rf_set_agc,
    rf_set_dual_tones_hz: noop_rf_set_dual_tones_hz,
    rf_set_dtmf: noop_rf_set_dtmf,
    rf_get_snr: noop_rf_get_snr,
    rf_get_noise: noop_rf_get_noise,
};

pub static STUB_AUDIO_VTABLE: AudioApi = AudioApi {
    audio_play_tone: noop_audio_play_tone,
    audio_play_beep: noop_audio_play_beep,
    audio_set_path: noop_audio_set_path,
    audio_capture_start: noop_audio_capture_start,
    audio_capture_read: noop_audio_capture_read,
    audio_capture_stop: noop_audio_capture_stop,
    audio_set_volume: noop_audio_set_volume,
    audio_get_volume: noop_audio_get_volume,
    audio_stop: noop_audio_stop,
    audio_is_playing: noop_audio_is_playing,
    audio_set_mic_gain: noop_audio_set_mic_gain,
    audio_read_mic_sample: noop_audio_read_mic_sample,
    audio_is_capturing: noop_audio_is_capturing,
};

pub static STUB_STORAGE_VTABLE: StorageApi = StorageApi {
    storage_load_config: noop_storage_load_config,
    storage_save_config: noop_storage_save_config,
    storage_read_asset: noop_storage_read_asset,
    storage_read_file: noop_storage_read_file,
    storage_write_file: noop_storage_write_file,
    storage_remove_file: noop_storage_remove_file,
    storage_file_size: noop_storage_file_size,
    storage_list_dir: noop_storage_list_dir,
    storage_file_exists: noop_storage_file_exists,
    storage_read_file_at: noop_storage_read_file_at,
    storage_append_file: noop_storage_append_file,
    storage_rename_file: noop_storage_rename_file,
    storage_create_dir: noop_storage_create_dir,
};

pub static STUB_KEYPAD_VTABLE: KeypadApi = KeypadApi {
    sys_poll_key: noop_sys_poll_key,
    sys_nav_dir: noop_sys_nav_dir,
    sys_key_matrix: noop_sys_key_matrix,
    sys_is_key_pressed: noop_sys_is_key_pressed,
    sys_is_f_active: noop_sys_is_f_active,
    sys_is_ptt_pressed: noop_sys_is_ptt_pressed,
    sys_is_side1_pressed: noop_sys_is_side1_pressed,
    sys_is_side2_pressed: noop_sys_is_side2_pressed,
    sys_clear_key_buffer: noop_sys_clear_key_buffer,
    sys_is_keypad_locked: noop_sys_is_keypad_locked,
};

pub static STUB_POWER_VTABLE: PowerApi = PowerApi {
    sys_battery_mv: noop_sys_battery_mv,
    sys_battery_pct: noop_sys_battery_pct,
    sys_battery_ma: noop_sys_battery_ma,
    sys_battery_type: noop_sys_battery_type,
    sys_raw_battery_adc: noop_sys_raw_battery_adc,
    sys_mcu_temp: noop_sys_mcu_temp,
    sys_is_charging: noop_sys_is_charging,
    sys_is_low: noop_sys_is_low,
    sys_is_critical: noop_sys_is_critical,
    sys_set_backlight: noop_sys_set_backlight,
    sys_set_backlight_brightness: noop_sys_set_backlight_brightness,
    sys_wake_backlight: noop_sys_wake_backlight,
    sys_set_flashlight: noop_sys_set_flashlight,
    sys_cpu_freq_hz: noop_sys_cpu_freq_hz,
};

pub static STUB_SERIAL_VTABLE: SerialApi = SerialApi {
    serial_write: noop_serial_write,
    serial_read: noop_serial_read,
    serial_is_connected: noop_serial_is_connected,
    serial_flush: noop_serial_flush,
};

pub static STUB_TIMER_VTABLE: TimerApi = TimerApi {
    sys_ticks_ms: noop_sys_ticks_ms,
    sys_uptime_secs: noop_sys_uptime_secs,
    sys_delay_ms: noop_sys_delay_ms,
    sys_delay_us: noop_sys_delay_us,
    sys_rtc_get: noop_sys_rtc_get,
};

pub static STUB_UI_VTABLE: UiApi = UiApi {
    ui_close_modal: noop_ui_close_modal,
    ui_has_modal: noop_ui_has_modal,
    ui_show_popup: noop_ui_show_popup,
    ui_show_confirm: noop_ui_show_confirm,
    ui_prompt_text: noop_ui_prompt_text,
    ui_get_modal_text: noop_ui_get_modal_text,
    ui_show_toast: noop_ui_show_toast,
    ui_is_animating: noop_ui_is_animating,
    ui_set_status_bar: noop_ui_set_status_bar,
    ui_get_status_bar: noop_ui_get_status_bar,
    ui_set_tray_icon: noop_ui_set_tray_icon,
    ui_draw_progress_bar: noop_ui_draw_progress_bar,
    ui_draw_s_meter: noop_ui_draw_s_meter,
    ui_draw_tag_badge: noop_ui_draw_tag_badge,
    ui_draw_key_badge: noop_ui_draw_key_badge,
    ui_draw_capsule_button: noop_ui_draw_capsule_button,
    ui_draw_scrollbar: noop_ui_draw_scrollbar,
    ui_draw_menu_item: noop_ui_draw_menu_item,
    ui_draw_big_digit: noop_ui_draw_big_digit,
    ui_draw_small_digit: noop_ui_draw_small_digit,
    ui_draw_battery: noop_ui_draw_battery,
    ui_draw_signal: noop_ui_draw_signal,
    ui_draw_modal_box: noop_ui_draw_modal_box,
    ui_show_confirm_labels: noop_ui_show_confirm_labels,
};

pub static STUB_MATH_VTABLE: MathApi = MathApi {
    sys_rand32: noop_sys_rand32,
    crc32: noop_crc32,
    crc16: noop_crc16,
    idivmod: noop_idivmod,
    uidivmod: noop_uidivmod,
    isqrt: noop_isqrt,
};

pub static STUB_FM_VTABLE: FmApi = FmApi {
    fm_tune: noop_fm_tune,
    fm_set_volume: noop_fm_set_volume,
    fm_mute: noop_fm_mute,
    fm_get_rssi: noop_fm_get_rssi,
};

pub static STUB_SERVICE_VTABLE: ServiceApi = ServiceApi {
    svc_register: noop_svc_register,
    svc_resolve: noop_svc_resolve,
    svc_release: noop_svc_release,
    ipc_send: noop_ipc_send,
    ipc_poll: noop_ipc_poll,
    sys_device_uid: noop_sys_device_uid,
    sys_os_version: noop_sys_os_version,
    sys_ram_available: noop_sys_ram_available,
    sys_stack_margin: noop_sys_stack_margin,
    sys_proc_count: noop_sys_proc_count,
    sys_proc_info: noop_sys_proc_info,
    sys_proc_kill: noop_sys_proc_kill,
    paged_call: noop_paged_call,
    paged_return: noop_paged_return,
    leaf_call: noop_leaf_call,
    lease_acquire: noop_lease_acquire,
    lease_release: noop_lease_release,
    mem_query: noop_mem_query,
    sys_post_event: noop_sys_post_event,
    sys_publish_event: noop_sys_publish_event,
    sys_stack_free_min: noop_sys_stack_free_min,
    ipc_send_msg: noop_ipc_send_msg,
    ipc_recv_msg: noop_ipc_recv_msg,
    ipc_reply: noop_ipc_reply,
    sys_find_proc: noop_sys_find_proc,
    sys_self_pid: noop_sys_self_pid,
};

pub static STUB_HW_BUS_VTABLE: HwBusApi = HwBusApi {
    hw_i2c_transfer: noop_hw_i2c_transfer,
    hw_spi_transfer: noop_hw_spi_transfer,
    hw_gpio_set: noop_hw_gpio_set,
    hw_gpio_get: noop_hw_gpio_get,
    hw_rf_read_reg: noop_hw_rf_read_reg,
    hw_rf_write_reg: noop_hw_rf_write_reg,
    hw_flash_read: noop_hw_flash_read,
};

impl KrxApiTable {
    /// Constructs a safe default table where all pointers reference safe no-op stubs.
    pub const fn safe_stub(granted_caps: u32) -> Self {
        Self {
            abi_version: KRX_ABI_VERSION_3_0,
            table_size: core::mem::size_of::<Self>() as u16,
            _pad0: 0,
            granted_caps,
            display: &raw const STUB_DISPLAY_VTABLE,
            radio: &raw const STUB_RADIO_VTABLE,
            audio: &raw const STUB_AUDIO_VTABLE,
            storage: &raw const STUB_STORAGE_VTABLE,
            keypad: &raw const STUB_KEYPAD_VTABLE,
            power: &raw const STUB_POWER_VTABLE,
            serial: &raw const STUB_SERIAL_VTABLE,
            timer: &raw const STUB_TIMER_VTABLE,
            ui: &raw const STUB_UI_VTABLE,
            math: &raw const STUB_MATH_VTABLE,
            fm: &raw const STUB_FM_VTABLE,
            service: &raw const STUB_SERVICE_VTABLE,
            hw_bus: &raw const STUB_HW_BUS_VTABLE,
        }
    }
}

impl KrxSysTable {
    /// Constructs a safe default system table where all pointers reference safe no-op stubs.
    pub const fn safe_stub(granted_caps: u32) -> Self {
        Self {
            abi_version: KRX_ABI_VERSION_3_1,
            table_size: core::mem::size_of::<Self>() as u16,
            _reserved: 0,
            granted_caps,
            display: &raw const STUB_DISPLAY_VTABLE,
            radio: &raw const STUB_RADIO_VTABLE,
            audio: &raw const STUB_AUDIO_VTABLE,
            storage: &raw const STUB_STORAGE_VTABLE,
            keypad: &raw const STUB_KEYPAD_VTABLE,
            power: &raw const STUB_POWER_VTABLE,
            serial: &raw const STUB_SERIAL_VTABLE,
            timer: &raw const STUB_TIMER_VTABLE,
            ui: &raw const STUB_UI_VTABLE,
            math: &raw const STUB_MATH_VTABLE,
            fm: &raw const STUB_FM_VTABLE,
            service: &raw const STUB_SERVICE_VTABLE,
            hw_bus: &raw const STUB_HW_BUS_VTABLE,
        }
    }
}

/// $O(1)$ Bitwise Integer Square Root for Cortex-M0+ (§8.2 / KRX-ABI 2.5.0).
#[inline(never)]
pub const fn isqrt(mut n: u32) -> u32 {
    let mut res = 0;
    let mut one = 1 << 30;
    while one > n {
        one >>= 2;
    }
    while one != 0 {
        if n >= res + one {
            n -= res + one;
            res = (res >> 1) + one;
        } else {
            res >>= 1;
        }
        one >>= 2;
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_modal_events_and_math() {
        let ev = KrxEvent {
            kind: KRX_EV_MODAL,
            key: 42,
            flags: MODAL_RESULT_CONFIRMED,
            reserved: 0,
            payload: 0,
        };
        assert!(ev.is_modal());
        assert!(!ev.is_key());

        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(25), 5);
        assert_eq!(isqrt(100), 10);
        assert_eq!(isqrt(10000), 100);
        assert_eq!(isqrt(1000000), 1000);
    }

    #[test]
    fn test_abi_geometries() {
        assert_eq!(core::mem::size_of::<KrxDirEntry>(), 32);
        assert_eq!(core::mem::align_of::<KrxDirEntry>(), 4);

        assert_eq!(core::mem::size_of::<KrxProcInfo>(), 32);
        assert_eq!(core::mem::align_of::<KrxProcInfo>(), 4);

        #[cfg(target_pointer_width = "32")]
        assert_eq!(core::mem::size_of::<KrxApiTable>(), 64);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(core::mem::size_of::<KrxApiTable>(), 120);
        assert_eq!(
            core::mem::align_of::<KrxApiTable>(),
            core::mem::align_of::<usize>()
        );

        #[cfg(target_pointer_width = "32")]
        assert_eq!(core::mem::size_of::<KrxSysTable>(), 64);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(core::mem::size_of::<KrxSysTable>(), 120);
        assert_eq!(
            core::mem::align_of::<KrxSysTable>(),
            core::mem::align_of::<usize>()
        );
    }

    #[test]
    fn test_krx_sys_table_dispatch() {
        let sys_table = KrxSysTable::safe_stub(0xCAFE_BABE);
        assert_eq!(sys_table.abi_version, KRX_ABI_VERSION_3_1);
        assert_eq!(sys_table.granted_caps, 0xCAFE_BABE);

        unsafe {
            assert!(!sys_table.display.is_null());
            assert!(!sys_table.radio.is_null());
            assert!(!sys_table.audio.is_null());
            assert!(!sys_table.storage.is_null());
            assert!(!sys_table.keypad.is_null());
            assert!(!sys_table.power.is_null());
            assert!(!sys_table.serial.is_null());
            assert!(!sys_table.timer.is_null());
            assert!(!sys_table.ui.is_null());
            assert!(!sys_table.math.is_null());
            assert!(!sys_table.fm.is_null());
            assert!(!sys_table.service.is_null());
            assert!(!sys_table.hw_bus.is_null());

            let display = &*sys_table.display;
            (display.clear)();
            assert_eq!((display.str_width)(b"Hello".as_ptr(), 5, 0), 0);

            let radio = &*sys_table.radio;
            assert_eq!((radio.rf_tune)(433_000_000, 25, 12_500), 1);
            assert_eq!((radio.rf_get_rssi)(), -140);

            let storage = &*sys_table.storage;
            assert_eq!((storage.storage_file_exists)(b"/test".as_ptr(), 5), 0);
            assert_eq!(
                (storage.storage_write_file)(b"/test".as_ptr(), 5, b"data".as_ptr(), 4),
                1
            );

            let audio = &*sys_table.audio;
            (audio.audio_play_tone)(1000, 100);
            assert_eq!((audio.audio_is_capturing)(), 0);

            let timer = &*sys_table.timer;
            assert_eq!((timer.sys_uptime_secs)(), 0);

            let power = &*sys_table.power;
            assert_eq!((power.sys_battery_mv)(), 4000);
        }
    }

    #[test]
    fn test_abi_safe_stub_dispatch() {
        let table = KrxApiTable::safe_stub(0x1234);
        assert_eq!(table.abi_version, KRX_ABI_VERSION_3_0);
        assert_eq!(table.granted_caps, 0x1234);

        unsafe {
            // Test modular VTable pointer dereferencing
            assert!(!table.radio.is_null());
            let rad = &*table.radio;
            assert_eq!((rad.rf_tune)(145_000_000, 25, 12_500), 1);

            assert!(!table.power.is_null());
            let pwr = &*table.power;
            assert_eq!((pwr.sys_battery_mv)(), 4000);
            assert_eq!((pwr.sys_battery_ma)(), 0);
            assert_eq!((pwr.sys_battery_type)(), 0);
            assert_eq!((pwr.sys_mcu_temp)(), 250);
            assert_eq!((pwr.sys_is_charging)(), 0);

            assert!(!table.storage.is_null());
            let stor = &*table.storage;
            assert_eq!((stor.storage_file_size)(core::ptr::null(), 0), 0);
            assert_eq!((stor.storage_remove_file)(core::ptr::null(), 0), 1);

            assert!(!table.service.is_null());
            let svc = &*table.service;
            assert_eq!((svc.sys_os_version)(), KRX_ABI_VERSION_3_0);
            assert_eq!((svc.sys_ram_available)(), 4096);
            assert_eq!((svc.sys_stack_margin)(), 1024);
            assert_eq!((svc.sys_proc_count)(), 0);
            assert_eq!((svc.svc_register)(0x0100, 1, 0), 1);
            assert_eq!((svc.svc_release)(0x0100), 1);
            assert_eq!((svc.paged_call)(0, 0, 0), 0);
            assert_eq!((svc.paged_return)(), 0);
            assert_eq!((svc.leaf_call)(1, 0), 0xFFFF_FFFF);
            assert_eq!((svc.lease_acquire)(1, 1000), 1);
            assert_eq!((svc.lease_release)(1), 1);
        }
    }

    #[test]
    fn test_krx_event_methods_and_kinds() {
        let ev_key = KrxEvent {
            kind: KRX_EV_KEY,
            key: 15,
            flags: 0,
            reserved: 0,
            payload: 0,
        };
        assert!(ev_key.is_key());
        assert!(!ev_key.is_bus());

        let ev_bus = KrxEvent {
            kind: KRX_EV_BUS,
            key: 1,
            flags: (crate::ipc::Topic::RADIO.0 & 0xFF) as u8,
            reserved: 0,
            payload: 145_000_000,
        };
        assert!(ev_bus.is_bus());
        assert!(ev_bus.is_radio_bus());
        assert!(!ev_bus.is_key());
    }

    #[test]
    fn test_radio_vtable_silicon_primitives_and_stubs() {
        let table = KrxApiTable::safe_stub(0xFFFF_FFFF);
        assert!(!table.radio.is_null());
        let radio = unsafe { &*table.radio };

        unsafe {
            // Radio control and mic clamping primitives
            (radio.rf_set_mic_mute)(1);
            (radio.rf_set_mic_mute)(0);
            assert_eq!((radio.rf_set_tx_carrier)(1), 1);
            assert_eq!((radio.rf_set_tx_carrier)(0), 1);
            assert_eq!((radio.rf_tune)(433_920_000, 0, 25_000), 1);
            assert_eq!((radio.rf_get_freq)(), 0);
            assert_eq!((radio.rf_get_rssi)(), -140);
            assert_eq!((radio.rf_is_carrier)(), 0);

            // Audio capture and FSK primitives
            assert!(!table.audio.is_null());
            let audio = &*table.audio;
            assert_eq!((audio.audio_capture_start)(8000), 1);
            let mut samples = [0i16; 16];
            assert_eq!((audio.audio_capture_read)(samples.as_mut_ptr(), 16), 0);
            (audio.audio_capture_stop)();

            // Hardware register access primitives (CapHwBus)
            assert!(!table.hw_bus.is_null());
            let bus = &*table.hw_bus;
            assert_eq!((audio.audio_is_capturing)(), 0);
            assert_eq!((bus.hw_rf_read_reg)(0x0C), 0);
            (bus.hw_rf_write_reg)(0x0C, 0);
            assert_eq!((bus.hw_flash_read)(0, core::ptr::null_mut(), 0), 1);

            // Extended radio & telemetry primitives
            (radio.rf_set_dual_tones_hz)(697, 80, 1209, 80);
            (radio.rf_set_dtmf)(b'1', 80);
            assert_eq!((radio.rf_get_snr)(), 0);
            assert_eq!((radio.rf_get_noise)(), 0);

            // Storage extension primitives
            assert!(!table.storage.is_null());
            let stor = &*table.storage;
            assert_eq!(
                (stor.storage_rename_file)(core::ptr::null(), 0, core::ptr::null(), 0),
                1
            );
            assert_eq!((stor.storage_create_dir)(core::ptr::null(), 0), 1);

            // Power, Keypad and Timer extensions
            assert!(!table.power.is_null());
            assert_eq!(((*table.power).sys_cpu_freq_hz)(), 48_000_000);
            assert!(!table.keypad.is_null());
            assert_eq!(((*table.keypad).sys_is_keypad_locked)(), 0);
            assert!(!table.timer.is_null());
            assert_eq!(((*table.timer).sys_rtc_get)(core::ptr::null_mut()), 1);
        }
    }
}
