//! KRX-ABI 2.5.0 / 3.0 Modular Capability VTables & Stable C-ABI Specification (§8).
//!
//! Defines the normative, append-only per-capability domain VTables (`CapDisplayVTable`,
//! `CapRadioVTable`, `CapAudioVTable`, `CapStorageVTable`, `CapKeypadVTable`,
//! `CapPowerVTable`, `CapSerialVTable`, `CapTimerVTable`, `CapUiVTable`,
//! `CapMathVTable`, `CapFmVTable`, `CapServiceVTable`, `CapHwBusVTable`) bound via
//! `CapBinding` (`SECTION_CAP_IMPORTS = 0x0003`) into each package's GOT slots (`[r9, #slot*4]`),
//! alongside the composite `KrxApiTable` dispatch structure.
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

pub const KRX_MODAL_DISMISSED: u8 = 0;
pub const KRX_MODAL_CONFIRMED: u8 = 1;
pub const KRX_MODAL_CANCELLED: u8 = 2;
pub const KRX_MODAL_TEXT_SUBMITTED: u8 = 3;

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

// ── Modular Per-Capability Domain VTables (Chapter 8.2, Append-Only) ────────

/// 1. `CapDisplay` VTable (`interface_id = 0x0001`)
#[repr(C)]
pub struct CapDisplayVTable {
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
}

/// 2. `CapRadio` VTable (`interface_id = 0x0002`)
#[repr(C)]
pub struct CapRadioVTable {
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
}

/// 3. `CapAudio` VTable (`interface_id = 0x0003`)
#[repr(C)]
pub struct CapAudioVTable {
    pub audio_play_tone: unsafe extern "C" fn(freq_hz: u16, duration_ms: u16),
    pub audio_play_beep: unsafe extern "C" fn(beep_type: u8),
    pub audio_set_path: unsafe extern "C" fn(on: u8),
    pub audio_capture_start: unsafe extern "C" fn(rate_hz: u16) -> u8,
    pub audio_capture_read: unsafe extern "C" fn(out_samples: *mut i16, max_samples: u16) -> u16,
    pub audio_capture_stop: unsafe extern "C" fn(),
}

/// 4. `CapStorage` VTable (`interface_id = 0x0004`)
#[repr(C)]
pub struct CapStorageVTable {
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
}

/// 5. `CapKeypad` VTable (`interface_id = 0x0005`)
#[repr(C)]
pub struct CapKeypadVTable {
    pub sys_poll_key: unsafe extern "C" fn() -> u8,
    pub sys_nav_dir: unsafe extern "C" fn(key: u8) -> i8,
    pub sys_key_matrix: unsafe extern "C" fn() -> u32,
}

/// 6. `CapPower` VTable (`interface_id = 0x0006`)
#[repr(C)]
pub struct CapPowerVTable {
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
}

/// 7. `CapSerial` VTable (`interface_id = 0x0007`)
#[repr(C)]
pub struct CapSerialVTable {
    pub serial_write: unsafe extern "C" fn(data: *const u8, len: u16) -> u16,
    pub serial_read: unsafe extern "C" fn(buf: *mut u8, max_len: u16) -> u16,
    pub serial_is_connected: unsafe extern "C" fn() -> u8,
}

/// 8. `CapTimer` VTable (`interface_id = 0x0008`)
#[repr(C)]
pub struct CapTimerVTable {
    pub sys_ticks_ms: unsafe extern "C" fn() -> u32,
    pub sys_uptime_secs: unsafe extern "C" fn() -> u32,
    pub sys_delay_ms: unsafe extern "C" fn(ms: u32),
}

/// 9. `CapUi` VTable (`interface_id = 0x0009`)
#[repr(C)]
pub struct CapUiVTable {
    pub ui_close_modal: unsafe extern "C" fn(),
    pub ui_has_modal: unsafe extern "C" fn() -> u8,
}

/// 10. `CapMath` VTable (`interface_id = 0x000A`)
#[repr(C)]
pub struct CapMathVTable {
    pub sys_rand32: unsafe extern "C" fn() -> u32,
    pub crc32: unsafe extern "C" fn(data: *const u8, len: u16) -> u32,
    pub crc16: unsafe extern "C" fn(data: *const u8, len: u16) -> u16,
    pub idivmod: unsafe extern "C" fn(n: i32, d: i32) -> u64,
    pub uidivmod: unsafe extern "C" fn(n: u32, d: u32) -> u64,
}

/// 11. `CapFm` VTable (`interface_id = 0x000B`)
#[repr(C)]
pub struct CapFmVTable {
    pub fm_tune: unsafe extern "C" fn(freq_100khz: u16),
    pub fm_set_volume: unsafe extern "C" fn(volume: u8),
    pub fm_mute: unsafe extern "C" fn(mute: u8),
    pub fm_get_rssi: unsafe extern "C" fn() -> u8,
}

/// 12. `CapService` VTable (`interface_id = 0x000C`)
#[repr(C)]
pub struct CapServiceVTable {
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
}

/// 13. `CapHwBus` VTable (`interface_id = 0x000D`)
#[repr(C)]
pub struct CapHwBusVTable {
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
}

/// Composite C-ABI Dispatch Table passed in `r1` to `KrxNativeEntryFn`
#[repr(C)]
pub struct KrxApiTable {
    pub abi_version: u32,
    pub table_size: u16,
    pub granted_caps: u32,

    // Modular Capability Domain VTable Pointers (§8.2 / KRX-ABI 2.5.0)
    pub display: *const CapDisplayVTable,
    pub radio: *const CapRadioVTable,
    pub audio: *const CapAudioVTable,
    pub storage: *const CapStorageVTable,
    pub keypad: *const CapKeypadVTable,
    pub power: *const CapPowerVTable,
    pub serial: *const CapSerialVTable,
    pub timer: *const CapTimerVTable,
    pub ui: *const CapUiVTable,
    pub math: *const CapMathVTable,
    pub fm: *const CapFmVTable,
    pub service: *const CapServiceVTable,
    pub hw_bus: *const CapHwBusVTable,

    // Display Services (Cap::DisplayDirect)
    pub draw_pixel: unsafe extern "C" fn(x: i16, y: i16, color: u8),
    pub draw_line: unsafe extern "C" fn(x0: i16, y0: i16, x1: i16, y1: i16, color: u8),
    pub draw_rect: unsafe extern "C" fn(x: i16, y: i16, w: i16, h: i16, color: u8),
    pub fill_rect: unsafe extern "C" fn(x: i16, y: i16, w: i16, h: i16, color: u8),
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

    // Radio Services (Cap::RadioRx / Cap::RadioTx)
    pub rf_tune: unsafe extern "C" fn(freq_hz: u32, bw: u8, step_hz: u32) -> u8,
    pub rf_get_freq: unsafe extern "C" fn() -> u32,
    pub rf_get_rssi: unsafe extern "C" fn() -> i16,
    pub rf_set_tx: unsafe extern "C" fn(active: u8) -> u8,
    pub rf_sweep:
        unsafe extern "C" fn(start_hz: u32, step_hz: u32, count: u16, out_rssi: *mut i16) -> u16,

    // Audio & Modulation Services (Cap::AudioOutput / Cap::AudioCapture)
    pub audio_play_tone: unsafe extern "C" fn(freq_hz: u16, duration_ms: u16),
    pub audio_capture_start: unsafe extern "C" fn(rate_hz: u16) -> u8,
    pub audio_capture_read: unsafe extern "C" fn(out_samples: *mut i16, max_samples: u16) -> u16,
    pub audio_capture_stop: unsafe extern "C" fn(),
    pub rf_symbol_tx_emit: unsafe extern "C" fn(symbols: *const KrxSymbol, count: u16) -> u8,

    // Storage Services (Cap::StorageRead / Cap::StorageWrite)
    pub storage_load_config: unsafe extern "C" fn(buf: *mut u8, max_len: u16) -> u16,
    pub storage_save_config: unsafe extern "C" fn(buf: *const u8, len: u16) -> u8,
    pub storage_read_asset:
        unsafe extern "C" fn(asset_id: u16, offset: u32, buf: *mut u8, len: u16) -> u16,

    // Input & System Services (Cap::Keypad / Cap::PowerManage / Cap::Timer)
    pub sys_poll_key: unsafe extern "C" fn() -> u8,
    pub sys_nav_dir: unsafe extern "C" fn(key: u8) -> i8,
    pub sys_ticks_ms: unsafe extern "C" fn() -> u32,
    pub sys_delay_ms: unsafe extern "C" fn(ms: u32),
    pub sys_battery_mv: unsafe extern "C" fn() -> u16,
    pub sys_battery_pct: unsafe extern "C" fn() -> u8,
    pub sys_rand32: unsafe extern "C" fn() -> u32,
    pub sys_stack_free: unsafe extern "C" fn() -> u16,

    // Hardware Bus Services (Cap::BusAccess / Cap::RawPeripheral)
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
}

unsafe impl Sync for KrxApiTable {}
unsafe impl Send for KrxApiTable {}

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

unsafe extern "C" fn noop_sys_poll_key() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_nav_dir(_key: u8) -> i8 {
    0
}
unsafe extern "C" fn noop_sys_key_matrix() -> u32 {
    0
}
unsafe extern "C" fn noop_sys_ticks_ms() -> u32 {
    0
}
unsafe extern "C" fn noop_sys_uptime_secs() -> u32 {
    0
}
unsafe extern "C" fn noop_sys_delay_ms(_ms: u32) {}
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

unsafe extern "C" fn noop_serial_write(_data: *const u8, _len: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_serial_read(_buf: *mut u8, _max_len: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_serial_is_connected() -> u8 {
    0
}

unsafe extern "C" fn noop_ui_close_modal() {}
unsafe extern "C" fn noop_ui_has_modal() -> u8 {
    0
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

unsafe extern "C" fn noop_fm_tune(_freq_100khz: u16) {}
unsafe extern "C" fn noop_fm_set_volume(_volume: u8) {}
unsafe extern "C" fn noop_fm_mute(_mute: u8) {}
unsafe extern "C" fn noop_fm_get_rssi() -> u8 {
    0
}

unsafe extern "C" fn noop_sys_stack_free() -> u16 {
    1024
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

pub static STUB_DISPLAY_VTABLE: CapDisplayVTable = CapDisplayVTable {
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
};

pub static STUB_RADIO_VTABLE: CapRadioVTable = CapRadioVTable {
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
};

pub static STUB_AUDIO_VTABLE: CapAudioVTable = CapAudioVTable {
    audio_play_tone: noop_audio_play_tone,
    audio_play_beep: noop_audio_play_beep,
    audio_set_path: noop_audio_set_path,
    audio_capture_start: noop_audio_capture_start,
    audio_capture_read: noop_audio_capture_read,
    audio_capture_stop: noop_audio_capture_stop,
};

pub static STUB_STORAGE_VTABLE: CapStorageVTable = CapStorageVTable {
    storage_load_config: noop_storage_load_config,
    storage_save_config: noop_storage_save_config,
    storage_read_asset: noop_storage_read_asset,
    storage_read_file: noop_storage_read_file,
    storage_write_file: noop_storage_write_file,
    storage_remove_file: noop_storage_remove_file,
    storage_file_size: noop_storage_file_size,
    storage_list_dir: noop_storage_list_dir,
};

pub static STUB_KEYPAD_VTABLE: CapKeypadVTable = CapKeypadVTable {
    sys_poll_key: noop_sys_poll_key,
    sys_nav_dir: noop_sys_nav_dir,
    sys_key_matrix: noop_sys_key_matrix,
};

pub static STUB_POWER_VTABLE: CapPowerVTable = CapPowerVTable {
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
};

pub static STUB_SERIAL_VTABLE: CapSerialVTable = CapSerialVTable {
    serial_write: noop_serial_write,
    serial_read: noop_serial_read,
    serial_is_connected: noop_serial_is_connected,
};

pub static STUB_TIMER_VTABLE: CapTimerVTable = CapTimerVTable {
    sys_ticks_ms: noop_sys_ticks_ms,
    sys_uptime_secs: noop_sys_uptime_secs,
    sys_delay_ms: noop_sys_delay_ms,
};

pub static STUB_UI_VTABLE: CapUiVTable = CapUiVTable {
    ui_close_modal: noop_ui_close_modal,
    ui_has_modal: noop_ui_has_modal,
};

pub static STUB_MATH_VTABLE: CapMathVTable = CapMathVTable {
    sys_rand32: noop_sys_rand32,
    crc32: noop_crc32,
    crc16: noop_crc16,
    idivmod: noop_idivmod,
    uidivmod: noop_uidivmod,
};

pub static STUB_FM_VTABLE: CapFmVTable = CapFmVTable {
    fm_tune: noop_fm_tune,
    fm_set_volume: noop_fm_set_volume,
    fm_mute: noop_fm_mute,
    fm_get_rssi: noop_fm_get_rssi,
};

pub static STUB_SERVICE_VTABLE: CapServiceVTable = CapServiceVTable {
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
};

pub static STUB_HW_BUS_VTABLE: CapHwBusVTable = CapHwBusVTable {
    hw_i2c_transfer: noop_hw_i2c_transfer,
    hw_spi_transfer: noop_hw_spi_transfer,
    hw_gpio_set: noop_hw_gpio_set,
    hw_gpio_get: noop_hw_gpio_get,
    hw_rf_read_reg: noop_hw_rf_read_reg,
    hw_rf_write_reg: noop_hw_rf_write_reg,
};

impl KrxApiTable {
    /// Constructs a safe default table where all pointers reference safe no-op stubs.
    pub const fn safe_stub(granted_caps: u32) -> Self {
        Self {
            abi_version: KRX_ABI_VERSION_3_0,
            table_size: core::mem::size_of::<Self>() as u16,
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
            draw_pixel: noop_draw_pixel,
            draw_line: noop_draw_line,
            draw_rect: noop_draw_rect,
            fill_rect: noop_fill_rect,
            draw_str: noop_draw_str,
            draw_str_align: noop_draw_str_align,
            commit_frame: noop_commit_frame,
            commit_rows: noop_commit_rows,
            get_framebuffer: noop_get_framebuffer,
            rf_tune: noop_rf_tune,
            rf_get_freq: noop_rf_get_freq,
            rf_get_rssi: noop_rf_get_rssi,
            rf_set_tx: noop_rf_set_tx,
            rf_sweep: noop_rf_sweep,
            audio_play_tone: noop_audio_play_tone,
            audio_capture_start: noop_audio_capture_start,
            audio_capture_read: noop_audio_capture_read,
            audio_capture_stop: noop_audio_capture_stop,
            rf_symbol_tx_emit: noop_rf_symbol_tx_emit,
            storage_load_config: noop_storage_load_config,
            storage_save_config: noop_storage_save_config,
            storage_read_asset: noop_storage_read_asset,
            sys_poll_key: noop_sys_poll_key,
            sys_nav_dir: noop_sys_nav_dir,
            sys_ticks_ms: noop_sys_ticks_ms,
            sys_delay_ms: noop_sys_delay_ms,
            sys_battery_mv: noop_sys_battery_mv,
            sys_battery_pct: noop_sys_battery_pct,
            sys_rand32: noop_sys_rand32,
            sys_stack_free: noop_sys_stack_free,
            hw_i2c_transfer: noop_hw_i2c_transfer,
            hw_spi_transfer: noop_hw_spi_transfer,
            hw_gpio_set: noop_hw_gpio_set,
            hw_gpio_get: noop_hw_gpio_get,
            hw_rf_read_reg: noop_hw_rf_read_reg,
            hw_rf_write_reg: noop_hw_rf_write_reg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_geometries() {
        assert_eq!(core::mem::size_of::<KrxDirEntry>(), 32);
        assert_eq!(core::mem::align_of::<KrxDirEntry>(), 4);

        assert_eq!(core::mem::size_of::<KrxProcInfo>(), 32);
        assert_eq!(core::mem::align_of::<KrxProcInfo>(), 4);
    }

    #[test]
    fn test_abi_safe_stub_dispatch() {
        let table = KrxApiTable::safe_stub(0x1234);
        assert_eq!(table.abi_version, KRX_ABI_VERSION_3_0);
        assert_eq!(table.granted_caps, 0x1234);

        unsafe {
            // Test direct convenience stubs
            assert_eq!((table.rf_tune)(145_000_000, 25, 12_500), 1);
            assert_eq!((table.sys_battery_mv)(), 4000);
            assert_eq!((table.sys_battery_pct)(), 100);

            // Test modular VTable pointer dereferencing
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
            assert_eq!((bus.hw_rf_read_reg)(0x0C), 0);
            (bus.hw_rf_write_reg)(0x0C, 0);
        }
    }
}
