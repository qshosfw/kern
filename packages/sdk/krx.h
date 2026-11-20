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

// ── Lifecycle Opcodes ────────────────────────────────────────────────────────

#define KRX_OP_START  0u
#define KRX_OP_EVENT  1u
#define KRX_OP_RENDER 2u
#define KRX_OP_EXIT   3u

#define COLOR_CLEAR      0u
#define COLOR_FILL       1u
#define COLOR_INVERT     2u
#define COLOR_DARK_GRAY  3u
#define COLOR_LIGHT_GRAY 4u

#include "krx_ops.h"

// ── Single-Entry System Call Table (16 B on ARM32) ───────────────────────────

typedef u32 (*KernCallFn)(u32 op, u32 a0, u32 a1, u32 a2);

typedef struct {
    u32 abi_version;
    u16 table_size;
    u16 _pad0;
    u32 granted_caps;
    KernCallFn call;
} KrxApiTable;

static inline u32 krx_call(const KrxApiTable* api, u32 op, u32 a0, u32 a1, u32 a2) {
    return api->call(op, a0, a1, a2);
}

static inline u32 krx_pack_xy(i16 x, i16 y) {
    return ((u32)(u16)x) | (((u32)(u16)y) << 16);
}

// ── Compact Request Descriptors (Pointers-First, matches kernel::proc::abi) ──

typedef struct {
    const u8* text;
    i16 x;
    i16 y;
    u16 len;
    i16 arg;
    u8  font;
    u8  color;
    u8  flags;
    u8  _pad;
} KrxTextReq;

typedef struct {
    const u8* path;
    u8*       buf;
    u32       offset;
    u16       len;
    u8        path_len;
    u8        extra;
} KrxFsReq;

typedef struct {
    const u8* title;
    const u8* msg;
    const u8* ok;
    const u8* cancel;
    u8  tag;
    u8  kind;
    u8  t_len;
    u8  m_len;
    u8  o_len;
    u8  c_len;
    u8  max_len;
    u8  _pad;
} KrxModalReq;

typedef struct {
    const u8* tx;
    u8*       rx;
    u32       addr;
    u16       tx_len;
    u16       rx_len;
} KrxBusReq;

typedef struct {
    const u8* target;
    const u8* msg;
    u8  target_len;
    u8  msg_len;
    u8  _pad[2];
} KrxIpcSendReq;

typedef struct {
    i16* out_rssi;
    u32  start_hz;
    u32  step_hz;
    u16  count;
    u16  _pad;
} KrxSweepReq;

// ── UI Status Bar & Widget Descriptors ───────────────────────────────────────

#define KRX_STATUS_VISIBLE       (1u << 0)
#define KRX_STATUS_DIVIDER       (1u << 1)
#define KRX_STATUS_HAS_SIGNAL    (1u << 2)
#define KRX_STATUS_CUSTOM_TITLE  (1u << 3)
#define KRX_STATUS_PARTIAL       (1u << 4)

#define KRX_ICON_CUSTOM_0        0x80u
#define KRX_ICON_CUSTOM_1        0x81u
#define KRX_ICON_NONE            0xFFu

#define KRX_CUSTOM_TRAY_PERSIST  (1u << 0)
#define KRX_CUSTOM_TRAY_ACTIVE   (1u << 1)

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

#define KRX_WIDGET_TABBED_SHEET    1u
#define KRX_WIDGET_ROUNDED_BOX     2u
#define KRX_WIDGET_NEGATED_HEADER  3u
#define KRX_WIDGET_SHEET_FRAME     4u
#define KRX_WIDGET_PROGRESS_BAR    5u
#define KRX_WIDGET_S_METER         6u
#define KRX_WIDGET_TAG_BADGE       7u
#define KRX_WIDGET_KEY_BADGE       8u
#define KRX_WIDGET_CAPSULE_BUTTON  9u
#define KRX_WIDGET_SCROLLBAR       10u
#define KRX_WIDGET_MENU_ITEM       11u
#define KRX_WIDGET_BIG_DIGIT       12u
#define KRX_WIDGET_SMALL_DIGIT     13u
#define KRX_WIDGET_BATTERY         14u
#define KRX_WIDGET_SIGNAL          15u
#define KRX_WIDGET_MODAL_BOX       16u

#define KRX_TAB_FLAG_INTEGRATED_86 (1u << 0)
#define KRX_TAB_FLAG_BOTTOM_BORDER (1u << 1)

typedef struct {
    const u8* p0;
    const u8* p1;
    i16 x;
    i16 y;
    u16 len0;
    u16 len1;
    u16 extra;
    u8  w;
    u8  h;
    u8  a;
    u8  b;
    u8  _pad[2];
} KrxWidget;

// ── Domain Data Structures ───────────────────────────────────────────────────

/* ORDS-01 channel record (64 B): one layout for `/user/channels.ch`, `.vfo` snapshots
 * and the live VFO exchanged through OP_RF_VFO. */
#define KRX_TONE_DCS             0x8000u  /* subaudible word: DCS code, else CTCSS deci-Hz */
#define KRX_TONE_DCS_INVERTED    0x4000u
#define KRX_TONE_VALUE_MASK      0x3FFFu

#define KRX_CH_BUSY_LOCKOUT      (1u << 0)
#define KRX_CH_SCAN_SKIP         (1u << 1)
#define KRX_CH_REVERSE           (1u << 2)
#define KRX_CH_PRIORITY_1        (1u << 3)
#define KRX_CH_PRIORITY_2        (1u << 4)
#define KRX_CH_TX_INHIBIT        (1u << 5)
#define KRX_CH_TONE_BCLO         (1u << 6)
#define KRX_CH_PTT_ID_START      (1u << 7)
#define KRX_CH_PTT_ID_END        (1u << 8)
#define KRX_CH_TALKAROUND        (1u << 9)
#define KRX_CH_FAVORITE          (1u << 10)
#define KRX_CH_CALL_MATCH_MUTE   (1u << 11)
#define KRX_CH_FREQ_LOCK         (1u << 12)
#define KRX_CH_WIDE_FILTER       (1u << 13)
#define KRX_CH_LNA_BOOST         (1u << 14)
#define KRX_CH_VOX_ENABLE        (1u << 15)

typedef struct {
    u32 rx_frequency_hz;
    u32 tx_frequency_hz;   /* 0 = simplex */
    u16 step_hz_x10;
    u16 bandwidth_50hz;
    u8  modulation;
    u8  squelch_level;
    u16 rx_subaudible;
    u16 tx_subaudible;
    u16 zone_mask;
    u16 flags;             /* KRX_CH_* */
    u16 power_mw;
    u16 proto_ref;
    u16 uid;
    u8  proto;
    u8  scram_mode;
    u8  scram_param;
    u8  scram_hop;
    u8  compand;
    u8  mic_gain;
    u8  ste;
    u8  roger;
    u8  tx_limit;          /* time-out in 15 s units, 0 = global */
    u8  audio_flags;
    u8  name[16];
    u8  user_tag[8];
    u16 record_crc;
} KrxChannel;

_Static_assert(sizeof(KrxChannel) == 64, "KrxChannel is the 64 B ORDS-01 record");

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

#define KRX_SYS_INFO_FLAG_HAS_CERT       (1u << 0)
#define KRX_SYS_INFO_FLAG_RDP_PROTECTED  (1u << 1)
#define KRX_SYS_INFO_FLAG_CALIB_VALID    (1u << 2)

#define KRX_QS1_NONE       0u
#define KRX_QS1_EMPTY      1u
#define KRX_QS1_INVALID    2u
#define KRX_QS1_UNVERIFIED 3u
#define KRX_QS1_VERIFIED   4u

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

// Event Kinds & Keys
#define KRX_EV_NONE         0u
#define KRX_EV_KEY          1u
#define KRX_EV_ENCODER      2u
#define KRX_EV_TICK         3u
#define KRX_EV_BUS          4u
#define KRX_EV_USB          5u
#define KRX_EV_POWER        6u
#define KRX_EV_MODAL        7u
#define KRX_EV_IPC          8u
#define KRX_IPC_ACTION_FLAG 0xACu

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

#define MODAL_RESULT_DISMISSED      0u
#define MODAL_RESULT_CONFIRMED      1u
#define MODAL_RESULT_TEXT_DONE      2u
#define MODAL_RESULT_SHEET_SELECTED 3u
#define MODAL_RESULT_CANCELED       4u

#define KRX_SYS_EV_CLOSE_APP        17u

typedef struct __attribute__((packed)) {
    u8 kind;
    u8 key;
    u8 flags;
    u8 reserved;
    u32 payload;
} KrxEvent;

#define KRX_LEAF_KIND_DONE       0u
#define KRX_LEAF_KIND_CALL_LEAF  1u
#define KRX_LEAF_KIND_YIELD      2u
#define KRX_LEAF_KIND_ERROR      3u

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

#define KRX_PAGED_INVOKE(api, block_id, fn_type, offset, ret_offset, ...) \
    ({ \
        u32 _target = krx_call((api), OP_PROC_PAGED, SUB_PROC_PAGED_CALL, ((u32)(block_id)) | (((u32)(offset)) << 16), (u32)(ret_offset)); \
        fn_type _fn = (fn_type)_target; \
        u32 _res = _fn(__VA_ARGS__); \
        krx_call((api), OP_PROC_PAGED, SUB_PROC_PAGED_RETURN, 0u, 0u); \
        _res; \
    })

// ── Inline Helpers: SYS / PROC ───────────────────────────────────────────────

static inline u16 krx_strlen(const char* s) {
    u16 len = 0;
    while (s[len]) len++;
    return len;
}

static inline u8 krx_sys_info(const KrxApiTable* api, KrxSysInfo* out) {
    return (u8)krx_call(api, OP_SYS_INFO, (u32)out, 0u, 0u);
}

static inline u32 krx_sys_os_version(const KrxApiTable* api) {
    return krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_OS_VERSION, 0u, 0u);
}

static inline u16 krx_sys_stack_free(const KrxApiTable* api) {
    return (u16)krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_STACK_FREE, 0u, 0u);
}

static inline u32 krx_sys_cpu_freq_hz(const KrxApiTable* api) {
    return krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_CPU_HZ, 0u, 0u);
}

static inline u32 krx_sys_rand32(const KrxApiTable* api) {
    return krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_RAND32, 0u, 0u);
}

static inline u32 krx_crc32(const KrxApiTable* api, const u8* data, u16 len) {
    return krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_CRC32, (u32)data, (u32)len);
}

static inline u32 krx_udivmod(const KrxApiTable* api, u32 n, u32 d, u32* rem) {
    return krx_call(api, OP_SYS_DIV, n, d, (u32)rem);
}

static inline void krx_sys_reboot(const KrxApiTable* api, u8 mode) {
    krx_call(api, OP_SYS_REBOOT, (u32)mode, 0u, 0u);
}

static inline u8 krx_sys_flush_config(const KrxApiTable* api) {
    return (u8)krx_call(api, OP_SYS_FLUSH, 0u, 0u, 0u);
}

static inline u32 krx_leaf_call(const KrxApiTable* api, u8 leaf_id, u32 arg) {
    return krx_call(api, OP_PROC_LEAF, (u32)leaf_id, arg, 0u);
}

// ── Inline Helpers: FS ───────────────────────────────────────────────────────

typedef struct {
    u8  depth;
    u8  _pad[3];
    u32 size;
    u16 l1[8];
} KrxFileExtents;

static inline u16 krx_fs_extents(
    const KrxApiTable* api, const u8* path, u8 path_len, KrxFileExtents* out
) {
    KrxFsReq r;
    r.path = path; r.buf = (u8*)out; r.offset = 0u; r.len = sizeof(KrxFileExtents);
    r.path_len = path_len; r.extra = 0u;
    return (u16)krx_call(api, OP_FS_READ, SUB_FS_READ_EXTENTS, (u32)&r, 0u);
}

static inline u16 krx_fs_read_at(
    const KrxApiTable* api, const u8* path, u8 path_len, u32 offset, u8* buf, u16 max_len
) {
    KrxFsReq r;
    r.path = path; r.buf = buf; r.offset = offset; r.len = max_len; r.path_len = path_len; r.extra = 0u;
    return (u16)krx_call(api, OP_FS_READ, SUB_FS_READ_FILE_AT, (u32)&r, 0u);
}

static inline u16 krx_fs_read(
    const KrxApiTable* api, const u8* path, u8 path_len, u8* buf, u16 max_len
) {
    return krx_fs_read_at(api, path, path_len, 0u, buf, max_len);
}

static inline u8 krx_fs_write(
    const KrxApiTable* api, const u8* path, u8 path_len, const u8* data, u16 len
) {
    KrxFsReq r;
    r.path = path; r.buf = (u8*)data; r.offset = 0u; r.len = len; r.path_len = path_len; r.extra = 0u;
    return (u8)krx_call(api, OP_FS_WRITE, SUB_FS_WRITE_FILE, (u32)&r, 0u);
}

static inline u8 krx_fs_append(
    const KrxApiTable* api, const u8* path, u8 path_len, const u8* data, u16 len
) {
    KrxFsReq r;
    r.path = path; r.buf = (u8*)data; r.offset = 0u; r.len = len; r.path_len = path_len; r.extra = 0u;
    return (u8)krx_call(api, OP_FS_WRITE, SUB_FS_WRITE_APPEND, (u32)&r, 0u);
}

static inline u32 krx_fs_size(const KrxApiTable* api, const u8* path, u8 path_len) {
    return krx_call(api, OP_FS_META, SUB_FS_META_SIZE, (u32)path, (u32)path_len);
}

static inline u8 krx_fs_exists(const KrxApiTable* api, const u8* path, u8 path_len) {
    return (u8)krx_call(api, OP_FS_META, SUB_FS_META_EXISTS, (u32)path, (u32)path_len);
}

static inline u8 krx_fs_remove(const KrxApiTable* api, const u8* path, u8 path_len) {
    return (u8)krx_call(api, OP_FS_META, SUB_FS_META_REMOVE, (u32)path, (u32)path_len);
}

static inline u8 krx_fs_mkdir(const KrxApiTable* api, const u8* path, u8 path_len) {
    return (u8)krx_call(api, OP_FS_META, SUB_FS_META_MKDIR, (u32)path, (u32)path_len);
}

// ── Inline Helpers: RF & AUD ─────────────────────────────────────────────────

/* Read VFO `slot` (0 = A, 1 = B, 2 = C) as a full channel record. Returns the memory
 * channel number + 1 the VFO is showing, 0 for a free VFO, or -1 on a bad argument. */
static inline i32 krx_vfo_get(const KrxApiTable* api, u8 slot, KrxChannel* out) {
    u32 r = krx_call(api, OP_RF_VFO, SUB_RF_VFO_GET_CHANNEL, (u32)slot, (u32)out);
    return (r & 0xFFu) ? -1 : (i32)(r >> 8);
}

/* Apply a full channel record to VFO `slot`; `memory` is the channel number + 1 it was
 * loaded from, or 0 for a free VFO. Retunes the hardware when the slot is live. */
static inline u8 krx_vfo_set(const KrxApiTable* api, u8 slot, u16 memory, const KrxChannel* ch) {
    return (u8)krx_call(api, OP_RF_VFO, SUB_RF_VFO_SET_CHANNEL, (u32)slot | ((u32)memory << 8), (u32)ch);
}

static inline u8 krx_rf_tune(const KrxApiTable* api, u32 freq_hz, u8 bw, u32 step_hz) {
    return (u8)krx_call(api, OP_RF_TUNE, freq_hz, (u32)bw, step_hz);
}

static inline u32 krx_rf_get_freq(const KrxApiTable* api) {
    return krx_call(api, OP_RF_GET, SUB_RF_GET_FREQ, 0u, 0u);
}

static inline u8 krx_prop_get(const KrxApiTable* api, u8 prop) {
    return (u8)krx_call(api, OP_PROP_GET, (u32)prop, 0u, 0u);
}

/* Returns 0 when stored; non-zero when the id, bounds or capability refuse it. */
static inline u8 krx_prop_set(const KrxApiTable* api, u8 prop, u8 val) {
    return (u8)krx_call(api, OP_PROP_SET, (u32)prop, (u32)val, 0u);
}

// ── Inline Helpers: GFX ──────────────────────────────────────────────────────

static inline void krx_clear(const KrxApiTable* api) {
    krx_call(api, OP_GFX_CLEAR, 0u, 0u, 0u);
}

static inline void krx_draw_pixel(const KrxApiTable* api, i16 x, i16 y, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_PIXEL | ((u32)color << 8), krx_pack_xy(x, y), 0u);
}

static inline void krx_draw_line(const KrxApiTable* api, i16 x0, i16 y0, i16 x1, i16 y1, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_LINE | ((u32)color << 8), krx_pack_xy(x0, y0), krx_pack_xy(x1, y1));
}

static inline void krx_draw_rect(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_RECT | ((u32)color << 8), krx_pack_xy(x, y), krx_pack_xy(w, h));
}

static inline void krx_fill_rect(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FILL_RECT | ((u32)color << 8), krx_pack_xy(x, y), krx_pack_xy(w, h));
}

static inline void krx_draw_line_h(const KrxApiTable* api, i16 x, i16 y, i16 len, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_HLINE | ((u32)color << 8), krx_pack_xy(x, y), (u32)(u16)len);
}

static inline void krx_draw_line_v(const KrxApiTable* api, i16 x, i16 y, i16 len, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_VLINE | ((u32)color << 8), krx_pack_xy(x, y), (u32)(u16)len);
}

static inline void krx_clear_corners(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_CLR_CORNER, krx_pack_xy(x, y), krx_pack_xy(w, h));
}

static inline void krx_fill_rect_dither(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h, u8 prog) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_DITHER | ((u32)prog << 16), krx_pack_xy(x, y), krx_pack_xy(w, h));
}

static inline void krx_invert_rect(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_INVERT, krx_pack_xy(x, y), krx_pack_xy(w, h));
}

static inline i16 krx_draw_char(const KrxApiTable* api, i16 x, i16 y, u8 ch, u8 font, u8 color) {
    return (i16)krx_call(
        api, OP_GFX_DRAW, SUB_GFX_DRAW_CHAR | ((u32)color << 8) | ((u32)font << 16), krx_pack_xy(x, y), (u32)ch
    );
}

static inline void krx_draw_icon(const KrxApiTable* api, i16 x, i16 y, u8 icon_id, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_ICON | ((u32)color << 8), krx_pack_xy(x, y), (u32)icon_id);
}

static inline void krx_scroll(const KrxApiTable* api, i8 dx, i8 dy, u8 fill) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_SCROLL | ((u32)fill << 8), ((u32)(u8)dx) | (((u32)(u8)dy) << 8), 0u);
}

static inline void krx_draw_dotted_line_h(
    const KrxApiTable* api, i16 x0, i16 x1, i16 y, u8 step, u8 color
) {
    krx_call(
        api, OP_GFX_DRAW, SUB_GFX_DRAW_DOTTED_H | ((u32)color << 8) | ((u32)step << 16),
        krx_pack_xy(x0, y), (u32)(u16)x1
    );
}

static inline void krx_draw_str_align(
    const KrxApiTable* api, i16 x, i16 y, const u8* text, u16 len, u8 font, u8 color, u8 align
) {
    KrxTextReq r;
    r.text = text; r.x = x; r.y = y; r.len = len; r.arg = (i16)align; r.font = font; r.color = color; r.flags = 0u; r._pad = 0u;
    krx_call(api, OP_GFX_TEXT, (u32)&r, 0u, 0u);
}

static inline void krx_draw_str_ex(
    const KrxApiTable* api, i16 x, i16 y, const u8* text, u16 len, u8 font, u8 color
) {
    krx_draw_str_align(api, x, y, text, len, font, color, 0u);
}

static inline void krx_draw_str_max(
    const KrxApiTable* api, i16 x, i16 y, const u8* text, u16 len, u8 font, u8 color, i16 max_w
) {
    KrxTextReq r;
    r.text = text; r.x = x; r.y = y; r.len = len; r.arg = max_w; r.font = font; r.color = color; r.flags = 2u; r._pad = 0u;
    krx_call(api, OP_GFX_TEXT, (u32)&r, 0u, 0u);
}

static inline i16 krx_str_width(const KrxApiTable* api, const u8* text, u16 len, u8 font) {
    KrxTextReq r;
    r.text = text; r.x = 0; r.y = 0; r.len = len; r.arg = 0; r.font = font; r.color = 1u; r.flags = 1u; r._pad = 0u;
    return (i16)krx_call(api, OP_GFX_TEXT, (u32)&r, 0u, 0u);
}

static inline void krx_draw_str(const KrxApiTable* api, i16 x, i16 y, const char* str) {
    krx_draw_str_ex(api, x, y, (const u8*)str, krx_strlen(str), 0u, 1u);
}

static inline void krx_commit_frame(const KrxApiTable* api) {
    krx_call(api, OP_GFX_COMMIT, 0u, 0u, 0u);
}

static inline u8* krx_get_framebuffer(const KrxApiTable* api) {
    return (u8*)krx_call(api, OP_GFX_FB, 0u, 0u, 0u);
}

static inline void krx_draw_bitmap(
    const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, const u8* data, u16 len
) {
    KrxWidget d;
    d.p0 = data; d.x = x; d.y = y; d.len0 = len; d.w = w; d.h = h;
    krx_call(api, OP_GFX_CTL, SUB_GFX_CTL_BITMAP, (u32)&d, 0u);
}

// ── Inline Helpers: UI & Widgets ─────────────────────────────────────────────

static inline void krx_ui_close_modal(const KrxApiTable* api) {
    krx_call(api, OP_UI_MODAL, SUB_UI_MODAL_CLOSE, 0u, 0u);
}

static inline void krx_ui_set_modal_hold(const KrxApiTable* api, u16 hold_ms) {
    krx_call(api, OP_UI_MODAL, SUB_UI_MODAL_SET_HOLD, (u32)hold_ms, 0u);
}

static inline void krx_ui_show_confirm_labels(
    const KrxApiTable* api, u8 tag,
    const u8* title, u8 t_len, const u8* msg, u8 m_len,
    const u8* ok, u8 o_len, const u8* cancel, u8 c_len
) {
    KrxModalReq r;
    r.title = title; r.msg = msg; r.ok = ok; r.cancel = cancel;
    r.tag = tag; r.kind = 1u; r.t_len = t_len; r.m_len = m_len;
    r.o_len = o_len; r.c_len = c_len; r.max_len = 0u;
    krx_call(api, OP_UI_MODAL, SUB_UI_MODAL_SHOW, (u32)&r, 0u);
}

static inline void krx_ui_show_toast(const KrxApiTable* api, const u8* msg, u8 len, u16 dur_ms) {
    krx_call(api, OP_UI_MODAL, SUB_UI_MODAL_TOAST, (u32)msg, ((u32)len) | (((u32)dur_ms) << 16));
}

static inline u8 krx_ui_set_status_bar(const KrxApiTable* api, const KrxStatusBar* sb) {
    return (u8)krx_call(api, OP_UI_STATUS, SUB_UI_STATUS_SET_BAR, (u32)sb, 0u);
}

static inline u8 krx_ui_set_tray_icon(const KrxApiTable* api, u8 icon_id) {
    return (u8)krx_call(api, OP_UI_STATUS, SUB_UI_STATUS_TRAY_ICON, (u32)icon_id, 0u);
}

static inline u8 krx_draw_tabbed_sheet(
    const KrxApiTable* api, u8 x, u8 y, u8 w, u8 h,
    u8 selected, u8 tab_count, u8 flags, const char* packed_titles, u16 packed_len
) {
    KrxWidget d;
    d.p0 = (const u8*)packed_titles; d.x = (i16)x; d.y = (i16)y;
    d.len0 = packed_len; d.extra = (u16)tab_count; d.w = w; d.h = h; d.a = selected; d.b = flags;
    return (u8)krx_call(api, OP_UI_WIDGET, KRX_WIDGET_TABBED_SHEET, (u32)&d, 0u);
}

static inline u8 krx_draw_rounded_box(const KrxApiTable* api, u8 x, u8 y, u8 w, u8 h) {
    KrxWidget d;
    d.p0 = 0; d.len0 = 0u; d.x = (i16)x; d.y = (i16)y; d.w = w; d.h = h;
    return (u8)krx_call(api, OP_UI_WIDGET, KRX_WIDGET_ROUNDED_BOX, (u32)&d, 0u);
}

static inline void krx_ui_draw_progress_bar(
    const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, u8 pct, u8 border
) {
    KrxWidget d;
    d.p0 = 0; d.len0 = 0u; d.x = x; d.y = y; d.w = w; d.h = h; d.a = pct; d.b = border;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_PROGRESS_BAR, (u32)&d, 0u);
}

static inline void krx_ui_draw_s_meter(
    const KrxApiTable* api, i16 x, i16 y, i16 rssi_dbm, u8 peak_hold
) {
    KrxWidget d;
    d.p0 = 0; d.len0 = 0u; d.x = x; d.y = y; d.extra = (u16)rssi_dbm; d.a = peak_hold;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_S_METER, (u32)&d, 0u);
}

static inline void krx_ui_draw_tag_badge(
    const KrxApiTable* api, i16 x, i16 y, const u8* text, u8 len, u8 style
) {
    KrxWidget d;
    d.p0 = text; d.x = x; d.y = y; d.len0 = (u16)len; d.a = style;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_TAG_BADGE, (u32)&d, 0u);
}

static inline void krx_ui_draw_key_badge(
    const KrxApiTable* api, i16 x, i16 y, const u8* key_s, u8 k_len, const u8* lbl, u8 l_len
) {
    KrxWidget d;
    d.p0 = key_s; d.p1 = lbl; d.x = x; d.y = y; d.len0 = (u16)k_len; d.len1 = (u16)l_len;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_KEY_BADGE, (u32)&d, 0u);
}

static inline void krx_ui_draw_capsule_button(
    const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, const u8* lbl, u8 l_len, u8 sel
) {
    KrxWidget d;
    d.p0 = lbl; d.x = x; d.y = y; d.len0 = (u16)l_len; d.w = w; d.h = h; d.a = sel;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_CAPSULE_BUTTON, (u32)&d, 0u);
}

static inline void krx_ui_draw_scrollbar(
    const KrxApiTable* api, u8 x, u8 y, u8 h, u16 cur, u16 total, u16 vis
) {
    KrxWidget d;
    d.p0 = 0; d.x = (i16)x; d.y = (i16)y; d.len0 = cur; d.len1 = vis; d.extra = total; d.h = h;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_SCROLLBAR, (u32)&d, 0u);
}

#define KRX_MENU_COMPACT     10u
#define KRX_MENU_COMFORTABLE 13u
#define KRX_MENU_FLAG_SEL     (1u << 0)
#define KRX_MENU_FLAG_ARROW   (1u << 1)
#define KRX_MENU_FLAG_OUTLINE (1u << 2)

static inline void krx_ui_draw_menu_item_ex(
    const KrxApiTable* api, i16 y, u8 row_h, const u8* title, u8 t_len,
    const u8* val, u8 v_len, u8 flags, u8 icon
) {
    KrxWidget d;
    d.p0 = title; d.p1 = val; d.x = 0; d.y = y; d.len0 = (u16)t_len; d.len1 = (u16)v_len;
    d.w = 0u; d.h = row_h; d.a = flags; d.b = icon;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_MENU_ITEM, (u32)&d, 0u);
}

static inline void krx_ui_draw_menu_item(
    const KrxApiTable* api, i16 y, const u8* title, u8 t_len, const u8* val, u8 v_len, u8 sel, u8 icon
) {
    krx_ui_draw_menu_item_ex(api, y, KRX_MENU_COMPACT, title, t_len, val, v_len, sel, icon);
}

static inline void krx_ui_draw_big_digit(const KrxApiTable* api, i16 x, i16 y, u8 dig, u8 col) {
    KrxWidget d;
    d.p0 = 0; d.len0 = 0u; d.x = x; d.y = y; d.a = dig; d.b = col;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_BIG_DIGIT, (u32)&d, 0u);
}

static inline void krx_ui_draw_small_digit(const KrxApiTable* api, i16 x, i16 y, u8 dig, u8 col) {
    KrxWidget d;
    d.p0 = 0; d.len0 = 0u; d.x = x; d.y = y; d.a = dig; d.b = col;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_SMALL_DIGIT, (u32)&d, 0u);
}

static inline void krx_ui_draw_battery(const KrxApiTable* api, i16 x, i16 y, u8 pct, u8 chg) {
    KrxWidget d;
    d.p0 = 0; d.len0 = 0u; d.x = x; d.y = y; d.a = pct; d.b = chg;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_BATTERY, (u32)&d, 0u);
}

static inline void krx_ui_draw_signal(const KrxApiTable* api, i16 x, i16 y, u8 lvl, u8 col) {
    KrxWidget d;
    d.p0 = 0; d.len0 = 0u; d.x = x; d.y = y; d.a = lvl; d.b = col;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_SIGNAL, (u32)&d, 0u);
}

static inline void krx_ui_draw_modal_box(const KrxApiTable* api, u8 w, u8 h, u8* out_origin) {
    KrxWidget d;
    d.p0 = (const u8*)out_origin; d.len0 = 0u; d.x = 0; d.y = 0; d.w = w; d.h = h;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_MODAL_BOX, (u32)&d, 0u);
}

// ── Inline Helpers: IO (Key / Power / Serial / Time) ─────────────────────────

static inline u8 krx_poll_key(const KrxApiTable* api) {
    return (u8)krx_call(api, OP_IO_KEY, SUB_IO_KEY_POLL, 0u, 0u);
}

static inline u32 krx_key_matrix(const KrxApiTable* api) {
    return krx_call(api, OP_IO_KEY, SUB_IO_KEY_MATRIX, 0u, 0u);
}

static inline u16 krx_pwr_u16(const KrxApiTable* api, u8 sub) {
    return (u16)krx_call(api, OP_IO_POWER, (u32)sub, 0u, 0u);
}

static inline void krx_wake_backlight(const KrxApiTable* api) {
    krx_call(api, OP_IO_POWER, SUB_IO_POWER_BL_WAKE, 0u, 0u);
}

static inline u16 krx_serial_write(const KrxApiTable* api, const u8* data, u16 len) {
    return (u16)krx_call(api, OP_IO_SERIAL, SUB_IO_SERIAL_WRITE, (u32)data, (u32)len);
}

static inline void krx_print(const KrxApiTable* api, const char* str) {
    if (api) {
        krx_serial_write(api, (const u8*)str, krx_strlen(str));
    }
}

static inline u32 krx_ticks_ms(const KrxApiTable* api) {
    return krx_call(api, OP_IO_TIME, SUB_IO_TIME_TICKS_MS, 0u, 0u);
}

static inline u32 krx_uptime_secs(const KrxApiTable* api) {
    return krx_call(api, OP_IO_TIME, SUB_IO_TIME_UPTIME_S, 0u, 0u);
}

static inline void krx_delay_ms(const KrxApiTable* api, u32 ms) {
    krx_call(api, OP_IO_TIME, SUB_IO_TIME_DELAY_MS, ms, 0u);
}

#endif // KRX_H
