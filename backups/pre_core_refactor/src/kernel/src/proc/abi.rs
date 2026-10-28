//! KRX-ABI 3.0 Stable C-ABI Table Specification (§8 / KRX-ABI 3.0).
//!
//! Provides the normative, packed C-ABI dispatch table (`KrxApiTable`) for
//! dynamically loaded native and paged ARM Thumb-1 userland executables.
//! All calls use standard AAPCS-Thumb linkage.

pub const KRX_ABI_VERSION_3_0: u32 = 0x0003_0000;

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
