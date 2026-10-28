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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxEvent {
    pub kind: u8,
    pub key: u8,
    pub flags: u8,
    pub reserved: u8,
    pub payload: u32,
}

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
    pub sys_is_charging: unsafe extern "C" fn() -> u8,
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
    pub sys_stack_free: unsafe extern "C" fn() -> u16,
    pub sys_stack_free_min: unsafe extern "C" fn() -> u16,
    pub sys_device_uid: unsafe extern "C" fn(out_uid12: *mut u8),
    pub page_fault: unsafe extern "C" fn(block_id: u8, offset: u16) -> usize,
    pub svc_register:
        unsafe extern "C" fn(interface_id: u16, abi_version: u16, vtable_ptr: usize) -> u8,
    pub svc_resolve: unsafe extern "C" fn(interface_id: u16, min_abi: u16) -> usize,
    pub ipc_handoff: unsafe extern "C" fn(
        target_name: *const u8,
        name_len: u8,
        msg: *const u8,
        msg_len: u8,
    ) -> u8,
    pub ipc_handoff_read: unsafe extern "C" fn(out_buf: *mut u8, max_len: u8) -> u8,
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

// ── Safe Fallback Stubs ──────────────────────────────────────────────────────

unsafe extern "C" fn noop_draw_pixel(_x: i16, _y: i16, _color: u8) {}
unsafe extern "C" fn noop_draw_line(_x0: i16, _y0: i16, _x1: i16, _y1: i16, _color: u8) {}
unsafe extern "C" fn noop_draw_rect(_x: i16, _y: i16, _w: i16, _h: i16, _color: u8) {}
unsafe extern "C" fn noop_fill_rect(_x: i16, _y: i16, _w: i16, _h: i16, _color: u8) {}
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
unsafe extern "C" fn noop_rf_set_tx(_active: u8) -> u8 {
    1
}
unsafe extern "C" fn noop_rf_sweep(
    _start_hz: u32,
    _step_hz: u32,
    _count: u16,
    _out_rssi: *mut i16,
) -> u16 {
    0
}

unsafe extern "C" fn noop_audio_play_tone(_freq_hz: u16, _duration_ms: u16) {}
unsafe extern "C" fn noop_audio_capture_start(_rate_hz: u16) -> u8 {
    1
}
unsafe extern "C" fn noop_audio_capture_read(_out_samples: *mut i16, _max_samples: u16) -> u16 {
    0
}
unsafe extern "C" fn noop_audio_capture_stop() {}
unsafe extern "C" fn noop_rf_symbol_tx_emit(_symbols: *const KrxSymbol, _count: u16) -> u8 {
    1
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

unsafe extern "C" fn noop_sys_poll_key() -> u8 {
    0
}
unsafe extern "C" fn noop_sys_nav_dir(_key: u8) -> i8 {
    0
}
unsafe extern "C" fn noop_sys_ticks_ms() -> u32 {
    0
}
unsafe extern "C" fn noop_sys_delay_ms(_ms: u32) {}
unsafe extern "C" fn noop_sys_battery_mv() -> u16 {
    4000
}
unsafe extern "C" fn noop_sys_battery_pct() -> u8 {
    100
}
unsafe extern "C" fn noop_sys_rand32() -> u32 {
    0x5A5A_5A5A
}
unsafe extern "C" fn noop_sys_stack_free() -> u16 {
    1024
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

impl KrxApiTable {
    /// Constructs a safe default table where all pointers reference safe no-op stubs.
    pub const fn safe_stub(granted_caps: u32) -> Self {
        Self {
            abi_version: KRX_ABI_VERSION_3_0,
            table_size: core::mem::size_of::<Self>() as u16,
            granted_caps,
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
