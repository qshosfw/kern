#ifndef KRX_H
#define KRX_H

typedef unsigned char  u8;
typedef signed char    i8;
typedef short          i16;
typedef unsigned short u16;
typedef int            i32;
typedef unsigned int   u32;
typedef long long      i64;
typedef unsigned long long u64;

#define KRX_OP_START  0
#define KRX_OP_EVENT  1
#define KRX_OP_RENDER 2
#define KRX_OP_EXIT   3

#define COLOR_CLEAR      0
#define COLOR_FILL       1
#define COLOR_INVERT     2
#define COLOR_DARK_GRAY  3
#define COLOR_LIGHT_GRAY 4

// 1. CapDisplayVTable (24 APIs)
typedef struct {
    void (*clear)(void);
    void (*draw_pixel)(i16 x, i16 y, u8 color);
    void (*draw_line)(i16 x0, i16 y0, i16 x1, i16 y1, u8 color);
    void (*draw_rect)(i16 x, i16 y, i16 w, i16 h, u8 color);
    void (*fill_rect)(i16 x, i16 y, i16 w, i16 h, u8 color);
    void (*draw_bitmap)(i16 x, i16 y, u8 w, u8 h, const u8* data, u16 len);
    void (*draw_str)(i16 x, i16 y, const u8* text, u16 len, u8 font, u8 color);
    void (*draw_str_align)(i16 x, i16 y, const u8* text, u16 len, u8 font, u8 color, u8 align);
    void (*commit_frame)(void);
    void (*commit_rows)(u8 row_mask);
    u8* (*get_framebuffer)(void);
    void (*draw_str_max)(i16 x, i16 y, const u8* text, u16 len, u8 font, u8 color, i16 max_w);
    void (*set_inverted)(u8 inverted);
    void (*set_contrast)(u8 contrast);
    void (*draw_line_h)(i16 x, i16 y, i16 len, u8 color);
    void (*draw_line_v)(i16 x, i16 y, i16 len, u8 color);
    void (*draw_dotted_line_h)(i16 x0, i16 x1, i16 y, u8 step, u8 color);
    void (*clear_corners)(i16 x, i16 y, i16 w, i16 h);
    void (*fill_rect_dither)(i16 x, i16 y, i16 w, i16 h, u8 progress);
    void (*invert_rect)(i16 x, i16 y, i16 w, i16 h);
    i16 (*str_width)(const u8* text, u16 len, u8 font);
    i16 (*draw_char)(i16 x, i16 y, u8 ch, u8 font, u8 color);
    void (*draw_icon)(i16 x, i16 y, u8 icon_id, u8 color);
    void (*scroll)(i8 dx, i8 dy, u8 fill_byte);
} CapDisplayVTable;

typedef struct {
    u8  kind;
    u8  _pad;
    u16 value;
} KrxSubaudible;

typedef struct {
    u32 rx_freq_hz;
    u32 tx_offset_hz;
    u32 step_hz;
    KrxSubaudible rx_subaudible;
    KrxSubaudible tx_subaudible;
    u8  modulation;
    u8  bandwidth;
    u8  power;
    u8  tx_shift;
    u8  squelch_level;
    u8  reverse;
    u8  busy_lock;
    u8  _reserved;
} KrxVfoConfig;

typedef struct {
    u8  active_vfo;
    u8  listening_vfo;
    u8  dual_watch;
    u8  cross_band;
    u8  bclo_policy;
    u8  ptt_active;
    u8  squelch_open;
    u8  subaudible_matched;
    u8  tot_locked_out;
    u8  lock_active;
    u8  tx_perm;
    u8  _reserved;
    u16 tot_remaining_s;
    i16 rssi_dbm;
} KrxRadioState;

// 2. CapRadioVTable (37 APIs)
typedef struct {
    u8 (*rf_tune)(u32 freq_hz, u8 bw, u32 step_hz);
    u32 (*rf_get_freq)(void);
    i16 (*rf_get_rssi)(void);
    u8 (*rf_is_carrier)(void);
    void (*rf_set_modulation)(u8 mod_type);
    void (*rf_set_squelch)(u8 level);
    u8 (*rf_set_tx)(u8 active);
    void (*rf_set_tx_power)(u8 level);
    void (*rf_set_af_mode)(u8 mode);
    u16 (*rf_sweep)(u32 start_hz, u32 step_hz, u16 count, i16* out_rssi);
    u8 (*rf_symbol_tx_emit)(const void* symbols, u16 count);
    void (*rf_set_dual_tones)(u16 tone1_word, u8 tone1_gain, u16 tone2_word, u8 tone2_gain);
    void (*rf_fsk_init)(u16 baud);
    u8 (*rf_fsk_send)(const u8* data, u16 len);
    u16 (*rf_fsk_recv)(u8* buf, u16 max_len);
    void (*rf_set_mic_mute)(u8 muted);
    u8 (*rf_set_tx_carrier)(u8 active);
    void (*rf_set_subaudible_ctcss)(u16 freq_tenths_hz);
    void (*rf_set_subaudible_dcs)(u16 code, u8 inverted);
    void (*rf_send_tail)(void);
    u32 (*rf_get_bandwidth)(void);
    u8 (*rf_get_modulation)(void);
    u8 (*rf_get_tx_state)(void);
    u8 (*rf_get_glitch)(void);
    void (*rf_set_agc)(u8 table_idx);
    void (*rf_set_dual_tones_hz)(u16 tone1_hz, u8 tone1_gain, u16 tone2_hz, u8 tone2_gain);
    void (*rf_set_dtmf)(u8 digit, u8 gain);
    u8 (*rf_get_snr)(void);
    u8 (*rf_get_noise)(void);
    u8 (*rf_get_vfo_config)(u8 slot, KrxVfoConfig* out);
    u8 (*rf_set_vfo_config)(u8 slot, const KrxVfoConfig* cfg);
    u8 (*rf_get_radio_state)(KrxRadioState* out);
    u8 (*rf_set_watch_mode)(u8 active_vfo, u8 dual_watch, u8 cross_band, u8 bclo);
    u8 (*rf_set_rx_tx_subaudible)(u8 is_tx, u8 kind, u16 value);
    u8 (*rf_lock_vfo)(u16 resources, u8 slot, u16 timeout_ms, u8 exclusive);
    u8 (*rf_unlock_vfo)(void);
    u8 (*rf_trigger_burst)(u16 freq_hz, u16 duration_ms);
} CapRadioVTable;

// 3. CapAudioVTable (13 APIs)
typedef struct {
    void (*audio_play_tone)(u16 freq_hz, u16 duration_ms);
    void (*audio_play_beep)(u8 beep_type);
    void (*audio_set_path)(u8 on);
    u8 (*audio_capture_start)(u16 rate_hz);
    u16 (*audio_capture_read)(i16* out_samples, u16 max_samples);
    void (*audio_capture_stop)(void);
    void (*audio_set_volume)(u8 volume);
    u8 (*audio_get_volume)(void);
    void (*audio_stop)(void);
    u8 (*audio_is_playing)(void);
    void (*audio_set_mic_gain)(u8 gain);
    u16 (*audio_read_mic_sample)(void);
    u8 (*audio_is_capturing)(void);
} CapAudioVTable;

// 4. CapStorageVTable (13 APIs)
typedef struct {
    u16 (*storage_load_config)(u8* buf, u16 max_len);
    u8 (*storage_save_config)(const u8* buf, u16 len);
    u16 (*storage_read_asset)(u16 asset_id, u32 offset, u8* buf, u16 len);
    u16 (*storage_read_file)(const u8* path, u8 path_len, u8* buf, u16 max_len);
    u8 (*storage_write_file)(const u8* path, u8 path_len, const u8* data, u16 len);
    u8 (*storage_remove_file)(const u8* path, u8 path_len);
    u32 (*storage_file_size)(const u8* path, u8 path_len);
    u8 (*storage_list_dir)(const u8* dir_path, u8 dir_len, void* out_entries, u8 max_entries);
    u8 (*storage_file_exists)(const u8* path, u8 path_len);
    u16 (*storage_read_file_at)(const u8* path, u8 path_len, u32 offset, u8* buf, u16 max_len);
    u8 (*storage_append_file)(const u8* path, u8 path_len, const u8* data, u16 len);
    u8 (*storage_rename_file)(const u8* old_path, u8 old_len, const u8* new_path, u8 new_len);
    u8 (*storage_create_dir)(const u8* path, u8 path_len);
    u8 (*storage_format)(u8 mode);
} CapStorageVTable;

// 5. CapKeypadVTable (10 APIs)
typedef struct {
    u8 (*sys_poll_key)(void);
    i8 (*sys_nav_dir)(u8 key);
    u32 (*sys_key_matrix)(void);
    u8 (*sys_is_key_pressed)(u8 key_code);
    u8 (*sys_is_f_active)(void);
    u8 (*sys_is_ptt_pressed)(void);
    u8 (*sys_is_side1_pressed)(void);
    u8 (*sys_is_side2_pressed)(void);
    void (*sys_clear_key_buffer)(void);
    u8 (*sys_is_keypad_locked)(void);
} CapKeypadVTable;

// 6. CapPowerVTable (14 APIs)
typedef struct {
    u16 (*sys_battery_mv)(void);
    u8 (*sys_battery_pct)(void);
    u16 (*sys_battery_ma)(void);
    u8 (*sys_battery_type)(void);
    u16 (*sys_raw_battery_adc)(void);
    i16 (*sys_mcu_temp)(void);
    u8 (*sys_is_charging)(void);
    u8 (*sys_is_low)(void);
    u8 (*sys_is_critical)(void);
    void (*sys_set_backlight)(u8 mode, u8 timeout_s);
    void (*sys_set_backlight_brightness)(u8 max, u8 min);
    void (*sys_wake_backlight)(void);
    void (*sys_set_flashlight)(u8 on);
    u32 (*sys_cpu_freq_hz)(void);
    void (*sys_reboot)(u8 mode);
    u8 (*sys_flush_config)(void);
} CapPowerVTable;

// 7. CapSerialVTable (4 APIs)
typedef struct {
    u16 (*serial_write)(const u8* data, u16 len);
    u16 (*serial_read)(u8* buf, u16 max_len);
    u8 (*serial_is_connected)(void);
    void (*serial_flush)(void);
} CapSerialVTable;

// 8. CapTimerVTable (5 APIs)
typedef struct {
    u32 (*sys_ticks_ms)(void);
    u32 (*sys_uptime_secs)(void);
    void (*sys_delay_ms)(u32 ms);
    void (*sys_delay_us)(u32 us);
    u8 (*sys_rtc_get)(u8* out_time);
} CapTimerVTable;

// Status Bar Flags
#define KRX_STATUS_VISIBLE       (1 << 0)
#define KRX_STATUS_DIVIDER       (1 << 1)
#define KRX_STATUS_HAS_SIGNAL    (1 << 2)
#define KRX_STATUS_CUSTOM_TITLE  (1 << 3)
#define KRX_STATUS_PARTIAL       (1 << 4)

#define KRX_ICON_CUSTOM_0        0x80
#define KRX_ICON_CUSTOM_1        0x81
#define KRX_ICON_NONE            0xFF

#define KRX_CUSTOM_TRAY_PERSIST  (1 << 0)
#define KRX_CUSTOM_TRAY_ACTIVE   (1 << 1)

// 64-byte ($2^6) KrxStatusBar
typedef struct __attribute__((packed)) {
    u8 flags;
    u8 signal_level;
    u8 left_icon_count;
    u8 center_icon_count;
    u8 left_icons[4];
    u8 center_icons[4];
    u8 title[16];
    u8 badge[8];
    u8 center[16];
    u8 custom_icon_0[5];
    u8 custom_icon_1[5];
    u8 tray_icon_id;
    u8 custom_flags;
} KrxStatusBar;

#define KRX_WIDGET_TABBED_SHEET    1
#define KRX_WIDGET_ROUNDED_BOX     2
#define KRX_WIDGET_NEGATED_HEADER  3
#define KRX_WIDGET_SHEET_FRAME     4

#define KRX_TAB_FLAG_INTEGRATED_86 (1u << 0)
#define KRX_TAB_FLAG_BOTTOM_BORDER (1u << 1)

typedef struct {
    i16 x;
    i16 y;
    u8 w;
    u8 h;
    u8 a;
    u8 b;
    const u8* p0;
    u16 len0;
    u16 extra;
} KrxWidget;

// 9. CapUiVTable (26 APIs)
typedef struct {
    void (*ui_close_modal)(void);
    u8 (*ui_has_modal)(void);
    void (*ui_show_popup)(u8 tag, const u8* title, u8 t_len, const u8* msg, u8 m_len);
    void (*ui_show_confirm)(u8 tag, const u8* title, u8 t_len, const u8* msg, u8 m_len);
    void (*ui_prompt_text)(u8 tag, const u8* title, u8 t_len, const u8* initial, u8 i_len, u8 max_len);
    u8 (*ui_get_modal_text)(u8* buf, u8 max_len);
    void (*ui_show_toast)(const u8* msg, u8 len, u16 duration_ms);
    u8 (*ui_is_animating)(void);
    u8 (*ui_set_status_bar)(const KrxStatusBar* sb);
    void (*ui_get_status_bar)(KrxStatusBar* out_sb);
    u8 (*ui_set_tray_icon)(u8 icon_id);
    void (*ui_draw_progress_bar)(i16 x, i16 y, u8 w, u8 h, u8 percent, u8 border);
    void (*ui_draw_s_meter)(i16 x, i16 y, i16 rssi_dbm, u8 peak_hold);
    void (*ui_draw_tag_badge)(i16 x, i16 y, const u8* text, u8 len, u8 style);
    void (*ui_draw_key_badge)(i16 x, i16 y, const u8* key_str, u8 k_len, const u8* label, u8 l_len);
    void (*ui_draw_capsule_button)(i16 x, i16 y, u8 w, u8 h, const u8* label, u8 l_len, u8 selected);
    void (*ui_draw_scrollbar)(u8 x, u8 y, u8 h, u16 current, u16 total, u16 visible);
    void (*ui_draw_menu_item)(i16 y, const u8* title, u8 t_len, const u8* value, u8 v_len, u8 selected, u8 icon_id);
    void (*ui_draw_big_digit)(i16 x, i16 y, u8 digit, u8 color);
    void (*ui_draw_small_digit)(i16 x, i16 y, u8 digit, u8 color);
    void (*ui_draw_battery)(i16 x, i16 y, u8 pct, u8 charging);
    void (*ui_draw_signal)(i16 x, i16 y, u8 level, u8 color);
    void (*ui_draw_modal_box)(u8 w, u8 h, u8* out_origin);
    void (*ui_show_confirm_labels)(u8 tag, const u8* title, u8 t_len, const u8* msg, u8 m_len, const u8* confirm, u8 c_len, const u8* cancel, u8 cl_len);
    void (*ui_set_modal_hold)(u16 hold_ms);
    u8 (*ui_draw_widget)(u8 kind, const KrxWidget* desc);
} CapUiVTable;

// 10. CapMathVTable (6 APIs)
typedef struct {
    u32 (*sys_rand32)(void);
    u32 (*crc32)(const u8* data, u16 len);
    u16 (*crc16)(const u8* data, u16 len);
    u64 (*idivmod)(i32 n, i32 d);
    u64 (*uidivmod)(u32 n, u32 d);
    u32 (*isqrt)(u32 val);
} CapMathVTable;

// 11. CapFmVTable (4 APIs)
typedef struct {
    void (*fm_tune)(u16 freq_100khz);
    void (*fm_set_volume)(u8 volume);
    void (*fm_mute)(u8 mute);
    u8 (*fm_get_rssi)(void);
} CapFmVTable;

#define KRX_SYS_INFO_FLAG_HAS_CERT       (1u << 0)
#define KRX_SYS_INFO_FLAG_RDP_PROTECTED  (1u << 1)
#define KRX_SYS_INFO_FLAG_CALIB_VALID    (1u << 2)

#define KRX_QS1_NONE       0u
#define KRX_QS1_EMPTY      1u
#define KRX_QS1_INVALID    2u
#define KRX_QS1_UNVERIFIED 3u
#define KRX_QS1_VERIFIED   4u

// Canonical hardware, firmware, and provisioning identity (128B = 2^7)
typedef struct {
    u16 flash_kb;
    u16 sram_kb;
    u16 flags;
    u8  mac[6];
    u8  fw_major;
    u8  fw_minor;
    u8  fw_patch;
    u8  target_id;
    char model[12];
    char mcu[12];
    char boot_ver[8];
    char git_hash[22];
    char build_date[12];
    char serial[15];
    u8  qs1_len;
    char qs1[17];
    u8  mcu_uid[12];
    u8  qs1_status;
} KrxSysInfo;

// Process & Kernel Resource Control Opcodes + Flags
#define KRX_PROC_FLAG_PERSISTENT  (1u << 0)
#define KRX_PROC_FLAG_SUSPENDED   (1u << 1)
#define KRX_PROC_FLAG_FOREGROUND  (1u << 2)
#define KRX_PROC_FLAG_HAS_LEASE   (1u << 3)
#define KRX_PROC_FLAG_KILLABLE    (1u << 4)

typedef struct {
    u8  pid;
    u8  app_id;
    u8  state;
    u8  priority;
    u32 runtime_ticks;
    u16 memory_bytes;
    u8  name[12];
    u8  exec_type;
    u8  flags;
    u16 stack_watermark;
    u16 ipc_events;
    u8  page_faults;
    u8  slice_overruns;
    u8  hibernations;
    u8  fault_count;
} KrxProcInfo;

#define PROC_CTL_KILL         0u
#define PROC_CTL_SET_STATE    1u
#define PROC_CTL_SET_PRIORITY 2u
#define PROC_CTL_FOCUS_APP    3u
#define PROC_CTL_REVOKE_LEASE 4u
#define PROC_CTL_RESET_STATS  5u

#define PROC_CTL_SYS_OVERVIEW 0x80u
#define PROC_CTL_LEASE_ENTRY  0x81u
#define PROC_CTL_SVC_ENTRY    0x82u
#define PROC_CTL_IPC_ENTRY    0x83u
#define PROC_CTL_PROC_EXT     0x84u

// 12. CapServiceVTable (28 APIs)
typedef struct {
    u8 (*svc_register)(u16 interface_id, u16 abi_version, u32 vtable_ptr);
    u32 (*svc_resolve)(u16 interface_id, u16 min_abi);
    u8 (*svc_release)(u16 interface_id);
    u8 (*ipc_send)(const u8* target_name, u8 name_len, const u8* msg, u8 msg_len);
    u8 (*ipc_poll)(u8* out_buf, u8 max_len);
    void (*sys_device_uid)(u8* out_uid12);
    u32 (*sys_os_version)(void);
    u16 (*sys_ram_available)(void);
    u16 (*sys_stack_margin)(void);
    u8 (*sys_proc_count)(void);
    u8 (*sys_proc_info)(u8 idx, void* out_info);
    u8 (*sys_proc_kill)(u8 pid);
    u32 (*paged_call)(u8 target_block_id, u16 target_offset, u16 return_offset);
    u32 (*paged_return)(void);
    u32 (*leaf_call)(u8 leaf_id, u32 arg);
    u8 (*lease_acquire)(u8 lease_kind, u32 timeout_ms);
    u8 (*lease_release)(u8 lease_kind);
    u8 (*mem_query)(u8 kind, void* out_desc);
    u8 (*sys_post_event)(u32 event_id);
    u8 (*sys_publish_event)(u16 topic, const u8* data, u8 len);
    u16 (*sys_stack_free_min)(void);
    u8 (*ipc_send_msg)(const void* msg);
    u8 (*ipc_recv_msg)(void* out_msg);
    u8 (*ipc_reply)(const void* req, const u8* payload, u8 len);
    u8 (*sys_find_proc)(const u8* name, u8 len);
    u8 (*sys_self_pid)(void);
    u8 (*sys_info)(KrxSysInfo* out_info);
    u8 (*sys_proc_ctl)(u16 op_and_id, u32 arg, u8* buf, u16 len);
} CapServiceVTable;

// 13. CapHwBusVTable (7 APIs)
typedef struct {
    u8 (*hw_i2c_transfer)(u8 addr, u8 reg, const u8* tx, u16 tx_len, u8* rx, u16 rx_len);
    u8 (*hw_spi_transfer)(u8 cs_pin, const u8* tx, u8* rx, u16 len);
    void (*hw_gpio_set)(u8 pin, u8 high);
    u8 (*hw_gpio_get)(u8 pin);
    u16 (*hw_rf_read_reg)(u8 reg);
    void (*hw_rf_write_reg)(u8 reg, u16 val);
    u8 (*hw_flash_read)(u32 addr, u8* buf, u16 len);
} CapHwBusVTable;

// Composite KrxApiTable passed in r1
typedef struct {
    u32 abi_version;
    u16 table_size;
    u16 _pad0;
    u32 granted_caps;

    const CapDisplayVTable* display;
    const CapRadioVTable* radio;
    const CapAudioVTable* audio;
    const CapStorageVTable* storage;
    const CapKeypadVTable* keypad;
    const CapPowerVTable* power;
    const CapSerialVTable* serial;
    const CapTimerVTable* timer;
    const CapUiVTable* ui;
    const CapMathVTable* math;
    const CapFmVTable* fm;
    const CapServiceVTable* service;
    const CapHwBusVTable* hw_bus;
} KrxApiTable;

typedef CapDisplayVTable DisplayApi;
typedef CapRadioVTable RadioApi;
typedef CapAudioVTable AudioApi;
typedef CapStorageVTable StorageApi;
typedef CapKeypadVTable KeypadApi;
typedef CapPowerVTable PowerApi;
typedef CapSerialVTable SerialApi;
typedef CapTimerVTable TimerApi;
typedef CapUiVTable UiApi;
typedef CapMathVTable MathApi;
typedef CapFmVTable FmApi;
typedef CapServiceVTable ServiceApi;
typedef CapHwBusVTable HwBusApi;
typedef KrxApiTable KrxSysTable;

// Event Kinds (§6.1 / KRX-ABI 2.5.0)
#define KRX_EV_NONE     0
#define KRX_EV_KEY      1
#define KRX_EV_ENCODER  2
#define KRX_EV_TICK     3
#define KRX_EV_BUS      4
#define KRX_EV_USB      5
#define KRX_EV_POWER    6
#define KRX_EV_MODAL    7
#define KRX_EV_IPC      8
#define KRX_IPC_ACTION_FLAG 0xAC

// Key Codes (1:1 Hardware Matrix & Microkernel Mapping)
typedef enum {
    KEY_PTT   = 0,
    KEY_SIDE1 = 1,
    KEY_SIDE2 = 2,
    KEY_UP    = 3,
    KEY_DOWN  = 4,
    KEY_MENU  = 5,
    KEY_EXIT  = 6,
    KEY_F     = 7,
    KEY_0     = 8,
    KEY_1     = 9,
    KEY_2     = 10,
    KEY_3     = 11,
    KEY_4     = 12,
    KEY_5     = 13,
    KEY_6     = 14,
    KEY_7     = 15,
    KEY_8     = 16,
    KEY_9     = 17,
    KEY_STAR  = 18,
    KEY_HASH  = 19,
} KrxKeyCode;

// Modal Results
#define MODAL_RESULT_DISMISSED      0
#define MODAL_RESULT_CONFIRMED      1
#define MODAL_RESULT_TEXT_DONE      2
#define MODAL_RESULT_SHEET_SELECTED 3
#define MODAL_RESULT_CANCELED       4

#define KRX_SYS_EV_CLOSE_APP        17u

// Structured Event Packet (passed in r2 on KRX_OP_EVENT)
typedef struct __attribute__((packed)) {
    u8 kind;
    u8 key;
    u8 flags;
    u8 reserved;
    u32 payload;
} KrxEvent;

// P0 Structured Leaf Overlays & Continuations (§5.1 / KRX 3.2)
#define KRX_LEAF_KIND_DONE       0
#define KRX_LEAF_KIND_CALL_LEAF  1
#define KRX_LEAF_KIND_YIELD      2
#define KRX_LEAF_KIND_ERROR      3

typedef struct __attribute__((packed)) {
    u8 kind;
    u8 target_or_code;
    u16 reserved;
    u32 payload;
} KrxLeafContinuation;

typedef struct __attribute__((packed)) {
    u8 leaf_id;
    u8 flags;
    u16 reserved;
    u32 arg;
    KrxLeafContinuation continuation;
} KrxLeafContext;

// P1 Demand Paging Helpers (KRX-ABI 2.5.0 Chapter 7)
#define KRX_PAGED_INVOKE(api, block_id, fn_type, offset, ret_offset, ...) \
    ({ \
        u32 _target = (api)->service->paged_call((block_id), (offset), (ret_offset)); \
        fn_type _fn = (fn_type)_target; \
        u32 _res = _fn(__VA_ARGS__); \
        (api)->service->paged_return(); \
        _res; \
    })

static inline u16 krx_strlen(const char* s) {
    u16 len = 0;
    while (s[len]) len++;
    return len;
}

static inline void krx_draw_str(const KrxApiTable* api, i16 x, i16 y, const char* str) {
    if (api && api->display) {
        api->display->draw_str(x, y, (const u8*)str, krx_strlen(str), 0, 1);
    }
}

static inline void krx_print(const KrxApiTable* api, const char* str) {
    if (api && api->serial) {
        api->serial->serial_write((const u8*)str, krx_strlen(str));
    }
}

static inline u8 krx_draw_tabbed_sheet(
    const KrxApiTable* api,
    u8 x, u8 y, u8 w, u8 h,
    u8 selected, u8 tab_count, u8 flags,
    const char* packed_titles, u16 packed_len
) {
    KrxWidget w_desc = {
        (i16)x, (i16)y, w, h, selected, flags,
        (const u8*)packed_titles, packed_len, (u16)tab_count
    };
    return (api && api->ui) ? api->ui->ui_draw_widget(KRX_WIDGET_TABBED_SHEET, &w_desc) : 1;
}

static inline u8 krx_draw_rounded_box(const KrxApiTable* api, u8 x, u8 y, u8 w, u8 h) {
    KrxWidget w_desc = { (i16)x, (i16)y, w, h, 0, 0, (const u8*)0, 0, 0 };
    return (api && api->ui) ? api->ui->ui_draw_widget(KRX_WIDGET_ROUNDED_BOX, &w_desc) : 1;
}

static inline u8 krx_draw_negated_header(
    const KrxApiTable* api, i16 y, const char* title, u16 len
) {
    KrxWidget w_desc = { 0, y, 0, 0, 0, 0, (const u8*)title, len, 0 };
    return (api && api->ui) ? api->ui->ui_draw_widget(KRX_WIDGET_NEGATED_HEADER, &w_desc) : 1;
}

static inline u8 krx_draw_sheet_frame(
    const KrxApiTable* api, i16 y, u8 h, const char* title, u16 len
) {
    KrxWidget w_desc = { 0, y, 0, h, 0, 0, (const u8*)title, len, 0 };
    return (api && api->ui) ? api->ui->ui_draw_widget(KRX_WIDGET_SHEET_FRAME, &w_desc) : 1;
}

#endif // KRX_H
