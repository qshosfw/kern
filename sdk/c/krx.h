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

#define KRX_FONT_MUMATRIX 0u
#define KRX_FONT_TOMTHUMB 1u
#define KRX_ALIGN_LEFT    0u
#define KRX_ALIGN_CENTER  1u
#define KRX_ALIGN_RIGHT   2u

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
    i16* out_rssi;
    u32  start_hz;
    u32  step_hz;
    u16  count;
    u16  _pad;
} KrxSweepReq;

// ── Canonical ABI Status Codes (`0 = Ok`, `-code` on error) ──────────────────

#define KRX_ERR_OK                0u
#define KRX_ERR_INVALID_ARG       1u
#define KRX_ERR_BAD_PTR           2u
#define KRX_ERR_BAD_LENGTH        3u
#define KRX_ERR_PERMISSION_DENIED 4u
#define KRX_ERR_BUSY              5u
#define KRX_ERR_NOT_FOUND         6u
#define KRX_ERR_ALREADY_EXISTS    7u
#define KRX_ERR_NO_SPACE          8u
#define KRX_ERR_CORRUPT_DATA      9u
#define KRX_ERR_IO_ERROR          10u
#define KRX_ERR_TIMEOUT           11u
#define KRX_ERR_LOW_POWER         12u
#define KRX_ERR_SAFETY_BLOCKED    13u
#define KRX_ERR_UNSUPPORTED       14u
#define KRX_ERR_BAD_SESSION       15u
#define KRX_ERR_INCOMPATIBLE_ABI  16u
#define KRX_ERR_FAULTED           17u
#define KRX_ERR_MAX               17u

static inline u8 krx_is_err(u32 raw) {
    i32 s = (i32)raw;
    return (u8)(s < 0 && s >= -(i32)KRX_ERR_MAX);
}

static inline u8 krx_err_code(u32 raw) {
    return krx_is_err(raw) ? (u8)(-(i32)raw) : KRX_ERR_OK;
}

typedef struct {
    const u8* title;
    const u8* unit;
    i32 value;
    i32 min;
    i32 max;
    i32 step;
    u8  tag;
    u8  title_len;
    u8  unit_len;
    u8  decimals;
} KrxNumReq;

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
#define KRX_WIDGET_DETAIL_ROW      17u

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

/* Fixed 64 B channel record: one layout for `/user/channels.ch`, `.vfo` snapshots
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

_Static_assert(sizeof(KrxChannel) == 64, "KrxChannel must be 64 bytes");

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

#define KRX_IPC_FLAG_REPLY    (1u << 0)
#define KRX_IPC_FLAG_URGENT   (1u << 1)
#define KRX_IPC_TARGET_FG     0x00u
#define KRX_IPC_TARGET_KERNEL 0x01u
#define KRX_IPC_TARGET_BCAST  0xFFu
#define KRX_IPC_MSG_STRING    0x0010u
#define KRX_IPC_MSG_APP_DATA  0x0031u

typedef struct {
    u8  sender_pid;
    u8  target_pid;
    u16 msg_type;
    u8  payload_len;
    u8  flags;
    u16 seq;
    u8  payload[24];
} KrxIpcMessage;

_Static_assert(sizeof(KrxIpcMessage) == 32, "KrxIpcMessage must be 32 bytes");

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
#define MODAL_RESULT_NUMBER_DONE    5u

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
        u32 _target = krx_call((api), OP_PROC_EXEC, SUB_PROC_EXEC_PAGED_CALL, ((u32)(block_id)) | (((u32)(offset)) << 16), (u32)(ret_offset)); \
        fn_type _fn = (fn_type)_target; \
        u32 _res = _fn(__VA_ARGS__); \
        krx_call((api), OP_PROC_EXEC, SUB_PROC_EXEC_PAGED_RET, 0u, 0u); \
        _res; \
    })

// ── Inline Helpers: SYS / PROP / PROC / IPC ──────────────────────────────────

static inline u16 krx_strlen(const char* s) {
    u16 len = 0;
    while (s[len]) len++;
    return len;
}

static inline u32 krx_prop_get32(const KrxApiTable* api, u8 prop) {
    return krx_call(api, OP_SYS_PROP, (u32)prop | ((u32)SUB_SYS_PROP_GET << 8), 0u, 0u);
}

static inline u32 krx_prop_set32(const KrxApiTable* api, u8 prop, u32 val) {
    return krx_call(api, OP_SYS_PROP, (u32)prop | ((u32)SUB_SYS_PROP_SET << 8), val, 0u);
}

static inline u8 krx_prop_get(const KrxApiTable* api, u8 prop) {
    return (u8)krx_prop_get32(api, prop);
}

/* Live control: changes the running system only and is handed back when the
 * app exits. Display, audio, and live RF properties; 0 = applied. */
static inline u8 krx_prop_set(const KrxApiTable* api, u8 prop, u8 val) {
    return (u8)krx_prop_set32(api, prop, (u32)val);
}

/* User preference: persisted to /pref. Needs KRX_CAP_PREFERENCES; 0 = stored. */
static inline u8 krx_pref_set(const KrxApiTable* api, u8 prop, u8 val) {
    return (u8)krx_call(api, OP_SYS_PROP, (u32)prop | ((u32)SUB_SYS_PROP_PREF << 8), (u32)val, 0u);
}

static inline u8 krx_sys_info(const KrxApiTable* api, KrxSysInfo* out) {
    return (u8)krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_INFO, (u32)out, 0u);
}

static inline u32 krx_sys_os_version(const KrxApiTable* api) {
    return krx_prop_get32(api, KRX_PROP_OS_VERSION);
}

static inline u16 krx_sys_stack_free(const KrxApiTable* api) {
    return (u16)krx_prop_get32(api, KRX_PROP_STACK_FREE);
}

static inline u32 krx_sys_cpu_freq_hz(const KrxApiTable* api) {
    return krx_prop_get32(api, KRX_PROP_CPU_HZ);
}

static inline u32 krx_sys_rand32(const KrxApiTable* api) {
    return krx_prop_get32(api, KRX_PROP_RAND32);
}

static inline u32 krx_crc32(const KrxApiTable* api, const u8* data, u16 len) {
    return krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_CRC32, (u32)data, (u32)len);
}

static inline u32 krx_udivmod(const KrxApiTable* api, u32 n, u32 d, u32* rem) {
    return krx_call(api, OP_SYS_DIV, n, d, (u32)rem);
}

static inline void krx_sys_reboot(const KrxApiTable* api, u8 mode) {
    krx_call(api, OP_SYS_CTL, SUB_SYS_CTL_REBOOT, (u32)mode, 0u);
}

static inline u32 krx_sys_event(const KrxApiTable* api, u32 event_id) {
    return krx_call(api, OP_SYS_CTL, SUB_SYS_CTL_EVENT, event_id, 0u);
}

static inline u8 krx_sys_flush_config(const KrxApiTable* api) {
    return (u8)krx_call(api, OP_SYS_QUERY, SUB_SYS_QUERY_FLUSH, 0u, 0u);
}

typedef struct {
    u8  block_id;
    u8  flags;
    u16 reserved;
    u32 arg;
    u8  cont_kind;
    u8  cont_target;
    u16 cont_reserved;
    u32 cont_payload;
} KrxBlockContext;

typedef u32 (*KrxBlockEntryFn)(u32 op, const KrxApiTable* api, const void* param);

static __attribute__((noinline, unused)) u32 krx_leaf_call(const KrxApiTable* api, u8 block_id, u32 arg) {
    u32 addr = krx_call(api, OP_PROC_EXEC, SUB_PROC_EXEC_PAGED_FAULT, (u32)block_id, 0u);
    if (!addr) return 0xFFFFFFFFu;
    u32 ctx[4] = { (u32)block_id, arg, 0u, 0u };
    return ((KrxBlockEntryFn)addr)(KRX_OP_START, api, (const void*)ctx);
}

static inline u8 krx_ipc_send(
    const KrxApiTable* api, u8 target_pid, u16 msg_type, u8 flags, const u8* payload, u8 len
) {
    KrxIpcMessage m;
    u8 n = len > 24u ? 24u : len;
    m.sender_pid = 0u;
    m.target_pid = target_pid;
    m.msg_type = msg_type;
    m.payload_len = n;
    m.flags = flags;
    m.seq = 0u;
    for (u8 i = 0u; i < 24u; i++) {
        m.payload[i] = (payload && i < n) ? payload[i] : 0u;
    }
    return (u8)krx_call(api, OP_IPC, SUB_IPC_SEND, (u32)&m, 0u);
}

static inline u8 krx_ipc_recv(const KrxApiTable* api, KrxIpcMessage* out) {
    return (u8)krx_call(api, OP_IPC, SUB_IPC_RECV, (u32)out, 0u);
}

static inline u8 krx_ipc_reply(
    const KrxApiTable* api, u8 sender_pid, u16 msg_type, const u8* payload, u8 len
) {
    return krx_ipc_send(api, sender_pid, msg_type, KRX_IPC_FLAG_REPLY, payload, len);
}

static inline u8 krx_ipc_send_str(const KrxApiTable* api, u8 target_pid, const char* str) {
    return krx_ipc_send(api, target_pid, KRX_IPC_MSG_STRING, 0u, (const u8*)str, (u8)krx_strlen(str));
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

static inline u8 krx_fs_rename(
    const KrxApiTable* api, const u8* old_path, u8 old_len, const u8* new_path, u8 new_len
) {
    KrxFsReq r;
    r.path = old_path; r.buf = (u8*)new_path; r.offset = 0u;
    r.len = (u16)new_len; r.path_len = old_len; r.extra = 0u;
    return (u8)krx_call(api, OP_FS_META, SUB_FS_META_RENAME, (u32)&r, 0u);
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

#define KRX_RF_WIDTH_12K5 0u
#define KRX_RF_WIDTH_25K  1u
#define KRX_RF_WIDTH_6K25 2u

/* Returns 1 for an invalid width or a step outside 10..655350 Hz in 10 Hz units. */
static inline u8 krx_rf_tune(const KrxApiTable* api, u32 freq_hz, u8 bw, u32 step_hz) {
    return (u8)krx_call(api, OP_RF_TUNE, freq_hz, (u32)bw, step_hz);
}

static inline u32 krx_rf_get_freq(const KrxApiTable* api) {
    return krx_prop_get32(api, KRX_PROP_RF_FREQ);
}

// ── Inline Helpers: GFX ──────────────────────────────────────────────────────

static inline void krx_clear(const KrxApiTable* api) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_CLEAR, 0u, 0u);
}

static inline void krx_draw_pixel(const KrxApiTable* api, i16 x, i16 y, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FILL_RECT | ((u32)color << 8), krx_pack_xy(x, y), krx_pack_xy(1, 1));
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
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FILL_RECT | ((u32)color << 8), krx_pack_xy(x, y), krx_pack_xy(len, 1));
}

static inline void krx_draw_line_v(const KrxApiTable* api, i16 x, i16 y, i16 len, u8 color) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FILL_RECT | ((u32)color << 8), krx_pack_xy(x, y), krx_pack_xy(1, len));
}

static inline void krx_clear_corners(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_CLR_CORNER, krx_pack_xy(x, y), krx_pack_xy(w, h));
}

static inline void krx_fill_rect_dither(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h, u8 prog) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_DITHER | ((u32)prog << 16), krx_pack_xy(x, y), krx_pack_xy(w, h));
}

static inline void krx_invert_rect(const KrxApiTable* api, i16 x, i16 y, i16 w, i16 h) {
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FILL_RECT | (COLOR_INVERT << 8), krx_pack_xy(x, y), krx_pack_xy(w, h));
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
    krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_COMMIT, 0u, 0u);
}

static inline u8* krx_get_framebuffer(const KrxApiTable* api) {
    return (u8*)krx_call(api, OP_GFX_DRAW, SUB_GFX_DRAW_FB, 0u, 0u);
}

static inline void krx_draw_bitmap(
    const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, const u8* data, u16 len
) {
    u32 req_bytes = (((u32)w + 7u) >> 3) * (u32)h;
    if (!data || len < req_bytes || !req_bytes) return;
    u32 a0 = (u32)SUB_GFX_DRAW_BITMAP | ((u32)COLOR_FILL << 8) | ((u32)w << 16) | ((u32)h << 24);
    krx_call(api, OP_GFX_DRAW, a0, krx_pack_xy(x, y), (u32)data);
}

// ── Inline Helpers: UI & Widgets ─────────────────────────────────────────────

static inline void krx_ui_close_modal(const KrxApiTable* api) {
    krx_prop_set32(api, KRX_PROP_MODAL_STATE, 0u);
}

static inline void krx_ui_set_modal_hold(const KrxApiTable* api, u16 hold_ms) {
    krx_prop_set32(api, KRX_PROP_MODAL_HOLD_MS, (u32)hold_ms);
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

static inline u32 krx_ui_prompt_number(const KrxApiTable* api, const KrxNumReq* req) {
    return krx_call(api, OP_UI_MODAL, SUB_UI_MODAL_PROMPT_NUM, (u32)req, 0u);
}

static inline u8 krx_ui_set_status_bar(const KrxApiTable* api, const KrxStatusBar* sb) {
    return (u8)krx_call(api, OP_UI_MODAL, SUB_UI_MODAL_SET_BAR, (u32)sb, 0u);
}

static inline u8 krx_ui_set_tray_icon(const KrxApiTable* api, u8 icon_id) {
    return (u8)krx_prop_set32(api, KRX_PROP_TRAY_ICON, (u32)icon_id);
}

/* Package-side chrome. Titles are a bounded, NUL-separated byte string.
 * `anim_prog` in 0..=16 interpolates the active tab pill and merged baseline gap
 * smoothly from `prev_selected` to `selected` (matching `gfx::TabbedSheet`). */
static __attribute__((noinline, unused)) u8 krx_draw_tabbed_sheet_anim(
    const KrxApiTable* api, u8 x, u8 y, u8 w, u8 h,
    u8 selected, u8 prev_selected, u8 anim_prog,
    u8 tab_count, u8 flags, const char* packed_titles, u16 packed_len
) {
    if (!w || h < 9 || !packed_titles) return 1;
    if (!tab_count) tab_count = 1;
    if (tab_count > 8) tab_count = 8;
    if (selected >= tab_count) selected = tab_count - 1;
    if (prev_selected >= tab_count) prev_selected = selected;

    struct Tab { u16 offset; u8 len; u8 width; } tabs[8];
    unsigned pos = 0;
    for (unsigned i = 0; i < tab_count; i++) {
        struct Tab* tab = &tabs[i];
        tab->offset = (u16)pos;
        while (pos < packed_len && packed_titles[pos]) pos++;
        unsigned tlen = pos - tab->offset;
        if (tlen > 128u) tlen = 128u;
        tab->len = (u8)tlen;
        int width = 8 + krx_str_width(api, (const u8*)packed_titles + tab->offset, tab->len, 0);
        tab->width = width > 248 ? 248 : width < 12 ? 12 : (u8)width;
        if (pos < packed_len) pos++;
    }

    int limit = x + w;
    if ((flags & KRX_TAB_FLAG_INTEGRATED_86) && limit > 86) limit = 86;

    // Keep the selected tab visible. Reserve space for each overflow arrow.
    unsigned start = 0, end;
    for (;;) {
        int cursor = x + (start ? 7 : 0);
        for (end = start; end < tab_count; end++) {
            int needed = tabs[end].width + (end + 1 < tab_count ? 7 : 0);
            if (cursor + needed > limit) break;
            cursor += tabs[end].width + 2;
        }
        if (end == start) end++;
        if (selected < end) break;
        start++;
    }

    int base_tx = x + (start ? 7 : 0);
    int active_tx = base_tx;
    int active_tw = tabs[selected].width;
    int prev_tx = base_tx;
    int cur_tx = base_tx;
    for (unsigned i = start; i < end; i++) {
        if (i == selected) active_tx = cur_tx;
        if (i == prev_selected) prev_tx = cur_tx;
        cur_tx += tabs[i].width + 2;
    }

    if (anim_prog < 16u && prev_selected != selected &&
        prev_selected >= start && prev_selected < end) {
        int p = (int)anim_prog;
        active_tx = prev_tx + (((active_tx - prev_tx) * p) >> 4);
        active_tw = (int)tabs[prev_selected].width +
                    (((active_tw - (int)tabs[prev_selected].width) * p) >> 4);
    }

    int baseline = y + 9;
    krx_draw_line_v(api, x, baseline, h - 9, COLOR_FILL);
    krx_draw_line_v(api, x + w - 1, baseline, h - 9, COLOR_FILL);
    if (flags & KRX_TAB_FLAG_BOTTOM_BORDER) {
        krx_draw_line_h(api, x + 1, y + h - 1, w > 2 ? w - 2 : 0, COLOR_FILL);
        krx_draw_pixel(api, x, y + h - 1, COLOR_CLEAR);
        krx_draw_pixel(api, x + w - 1, y + h - 1, COLOR_CLEAR);
    }

    // Merged baseline with smoothly gliding active gap.
    if (active_tx > x) krx_draw_line_h(api, x, baseline, (u8)(active_tx - x), COLOR_FILL);
    krx_draw_line_h(api, (i16)active_tx, baseline, (u8)active_tw, COLOR_CLEAR);
    int right_start = active_tx + active_tw;
    if (right_start < x + w) {
        krx_draw_line_h(api, (i16)right_start, baseline, (u8)(x + w - right_start), COLOR_FILL);
    }

    // Draw inactive tabs first, then draw the elevated active pill on top for crisp gliding.
    int tab_x = base_tx;
    for (unsigned i = start; i < end; i++) {
        if (i != selected) {
            const struct Tab* tab = &tabs[i];
            int tab_y = y + 1;
            int right = tab_x + tab->width - 1;
            krx_draw_line_h(api, tab_x + 1, tab_y, tab->width - 2, COLOR_FILL);
            krx_draw_pixel(api, tab_x, tab_y, COLOR_CLEAR);
            krx_draw_pixel(api, right, tab_y, COLOR_CLEAR);
            krx_draw_line_v(api, tab_x, tab_y + 1, 7, COLOR_FILL);
            krx_draw_line_v(api, right, tab_y + 1, 7, COLOR_FILL);
            krx_draw_str_max(api, tab_x + 4, tab_y + 6, (const u8*)packed_titles + tab->offset,
                             tab->len, 0, COLOR_FILL, tab->width - 7);
        }
        tab_x += tabs[i].width + 2;
    }

    {
        const struct Tab* sel_tab = &tabs[selected];
        int right = active_tx + active_tw - 1;
        if (active_tw > 2) {
            krx_fill_rect(api, (i16)(active_tx + 1), (i16)(y + 1), (u8)(active_tw - 2), 9u, COLOR_CLEAR);
            krx_draw_line_h(api, (i16)(active_tx + 1), y, (u8)(active_tw - 2), COLOR_FILL);
        }
        krx_draw_pixel(api, (i16)active_tx, y, COLOR_CLEAR);
        krx_draw_pixel(api, (i16)right, y, COLOR_CLEAR);
        krx_draw_line_v(api, (i16)active_tx, y + 1, 9, COLOR_FILL);
        krx_draw_line_v(api, (i16)right, y + 1, 9, COLOR_FILL);
        if (active_tw > 7) {
            krx_draw_str_max(api, (i16)(active_tx + 4), y + 6,
                             (const u8*)packed_titles + sel_tab->offset,
                             sel_tab->len, 0, COLOR_FILL, (u8)(active_tw - 7));
        }
    }

    if (start) krx_draw_icon(api, x, y + 1, KRX_ICON_ARROW_LEFT, COLOR_FILL);
    if (end < tab_count) krx_draw_icon(api, cur_tx - 1, y + 1, KRX_ICON_ARROW_RIGHT, COLOR_FILL);
    return 0;
}

static inline u8 krx_draw_tabbed_sheet(
    const KrxApiTable* api, u8 x, u8 y, u8 w, u8 h,
    u8 selected, u8 tab_count, u8 flags, const char* packed_titles, u16 packed_len
) {
    return krx_draw_tabbed_sheet_anim(
        api, x, y, w, h, selected, selected, 16u, tab_count, flags, packed_titles, packed_len
    );
}

static inline u8 krx_draw_rounded_box(const KrxApiTable* api, u8 x, u8 y, u8 w, u8 h) {
    if (!w || !h) return 1u;
    krx_draw_rect(api, (i16)x, (i16)y, w, h, COLOR_FILL);
    krx_clear_corners(api, (i16)x, (i16)y, w, h);
    return 0u;
}

static inline void krx_ui_draw_progress_bar(
    const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, u8 pct, u8 border
) {
    if (w < 4u || h < 2u) return;
    if (pct > 100u) pct = 100u;
    krx_fill_rect(api, x, y, w, h, COLOR_CLEAR);
    if (border) {
        krx_draw_rect(api, x, y, w, h, COLOR_FILL);
        if (h >= 5u) krx_clear_corners(api, x, y, w, h);
    }
    u8 inset = border ? 2u : 0u;
    u8 max_fw = w > (inset * 2u) ? (u8)(w - inset * 2u) : 0u;
    u8 fh = h > (inset * 2u) ? (u8)(h - inset * 2u) : 1u;
    u8 fw = (u8)krx_udivmod(api, (u32)max_fw * (u32)pct, 100u, (u32*)0);
    if (fw > 0u) {
        krx_fill_rect(api, (i16)(x + inset), (i16)(y + inset), fw, fh, COLOR_FILL);
    }
}

static inline void krx_ui_draw_s_meter(
    const KrxApiTable* api, i16 x, i16 y, i16 rssi_dbm, u8 peak_hold
) {
    (void)peak_hold;
    i32 adj = (i32)rssi_dbm + 127;
    u8 pct = adj <= 0 ? 0u : adj >= 73 ? 100u : (u8)((adj * 100) >> 6);
    krx_ui_draw_progress_bar(api, x, y, 60u, 5u, pct, 1u);
}

static inline void krx_ui_draw_tag_badge(
    const KrxApiTable* api, i16 x, i16 y, const u8* text, u8 len, u8 style
) {
    if (!text || !len) return;
    i16 tw = krx_str_width(api, text, len, KRX_FONT_TOMTHUMB) + 4;
    if (tw < 6) tw = 6;
    if (style == 1u) {
        krx_fill_rect(api, x, y, (u8)tw, 7u, COLOR_FILL);
        krx_draw_str_ex(api, (i16)(x + 2), (i16)(y + 6), text, len, KRX_FONT_TOMTHUMB, COLOR_CLEAR);
    } else {
        krx_draw_rect(api, x, y, (u8)tw, 7u, COLOR_FILL);
        krx_draw_str_ex(api, (i16)(x + 2), (i16)(y + 6), text, len, KRX_FONT_TOMTHUMB, COLOR_FILL);
    }
}

static inline void krx_ui_draw_key_badge(
    const KrxApiTable* api, i16 x, i16 y, const u8* key_s, u8 k_len, const u8* lbl, u8 l_len
) {
    if (!key_s || !k_len) return;
    i16 kw = krx_str_width(api, key_s, k_len, KRX_FONT_TOMTHUMB) + 4;
    if (kw < 5) kw = 5;
    krx_fill_rect(api, x, y, (u8)kw, 7u, COLOR_FILL);
    krx_clear_corners(api, x, y, (u8)kw, 7u);
    krx_draw_str_ex(api, (i16)(x + 2), (i16)(y + 6), key_s, k_len, KRX_FONT_TOMTHUMB, COLOR_CLEAR);
    if (lbl && l_len) {
        krx_draw_str_ex(api, (i16)(x + kw + 2), (i16)(y + 6), lbl, l_len, KRX_FONT_TOMTHUMB, COLOR_FILL);
    }
}

static inline void krx_ui_draw_capsule_button(
    const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, const u8* lbl, u8 l_len, u8 sel
) {
    if (w < 4u || h < 5u) return;
    if (sel) {
        krx_fill_rect(api, x, y, w, h, COLOR_FILL);
    } else {
        krx_fill_rect(api, x, y, w, h, COLOR_CLEAR);
        krx_draw_rect(api, x, y, w, h, COLOR_FILL);
    }
    krx_clear_corners(api, x, y, w, h);
    if (lbl && l_len) {
        krx_draw_str_align(
            api, (i16)(x + (w >> 1)), (i16)(y + h - 2),
            lbl, l_len, KRX_FONT_TOMTHUMB, sel ? COLOR_CLEAR : COLOR_FILL, KRX_ALIGN_CENTER
        );
    }
}

static inline void krx_ui_draw_scrollbar(
    const KrxApiTable* api, u8 x, u8 y, u8 h, u16 cur, u16 total, u16 vis
) {
    if (total == 0u || vis == 0u || total <= vis || h < 4u) return;
    krx_draw_line_v(api, (i16)(x + 1u), (i16)y, h, COLOR_FILL);
    u32 th = krx_udivmod(api, (u32)h * (u32)vis + ((u32)total >> 1), (u32)total, (u32*)0);
    if (th < 4u) th = 4u;
    if (th > h) th = h;
    u32 max_off = (u32)(total - vis);
    u32 travel = (u32)h - th;
    u32 pos = cur >= max_off ? max_off : (u32)cur;
    u32 ty = pos >= max_off ? travel : krx_udivmod(api, pos * travel + (max_off >> 1), max_off, (u32*)0);
    krx_fill_rect(api, (i16)x, (i16)(y + ty), 3u, (u8)th, COLOR_FILL);
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
    d.extra = 0u; d.w = 0u; d.h = row_h; d.a = flags; d.b = icon;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_MENU_ITEM, (u32)&d, 0u);
}

static inline void krx_ui_draw_menu_item(
    const KrxApiTable* api, i16 y, const u8* title, u8 t_len, const u8* val, u8 v_len, u8 sel, u8 icon
) {
    krx_ui_draw_menu_item_ex(api, y, KRX_MENU_COMPACT, title, t_len, val, v_len, sel, icon);
}

static inline void krx_ui_draw_big_digit(const KrxApiTable* api, i16 x, i16 y, u8 dig, u8 col) {
    u8 ch = (u8)('0' + (dig % 10u));
    krx_draw_str_ex(api, x, (i16)(y + 8), &ch, 1u, KRX_FONT_MUMATRIX, col);
}

static inline void krx_ui_draw_small_digit(const KrxApiTable* api, i16 x, i16 y, u8 dig, u8 col) {
    u8 ch = (u8)('0' + (dig % 10u));
    krx_draw_str_ex(api, x, (i16)(y + 5), &ch, 1u, KRX_FONT_TOMTHUMB, col);
}

static inline void krx_ui_draw_battery(const KrxApiTable* api, i16 x, i16 y, u8 pct, u8 chg) {
    (void)chg;
    if (pct > 100u) pct = 100u;
    krx_draw_rect(api, x, y, 12u, 6u, COLOR_FILL);
    krx_fill_rect(api, (i16)(x + 12), (i16)(y + 2), 1u, 2u, COLOR_FILL);
    u8 fw = (u8)(((u32)pct * 8u + 50u) >> 7);
    if (fw > 8u) fw = 8u;
    if (fw > 0u) krx_fill_rect(api, (i16)(x + 2), (i16)(y + 2), fw, 2u, COLOR_FILL);
}

static inline void krx_ui_draw_signal(const KrxApiTable* api, i16 x, i16 y, u8 lvl, u8 col) {
    krx_draw_pixel(api, x, y, col);
    krx_draw_line_v(api, (i16)(x + 1), (i16)(y + 1), 4u, col);
    krx_draw_pixel(api, (i16)(x + 2), y, col);
    u8 bars = lvl > 5u ? 5u : lvl;
    for (u8 i = 1u; i <= bars; i++) {
        krx_draw_line_v(api, (i16)(x + 1 + i * 2u), (i16)(y + 5 - i), i, col);
    }
}

static inline void krx_ui_draw_modal_box(const KrxApiTable* api, u8 w, u8 h, u8* out_origin) {
    u8 ox = w < 128u ? (u8)((128u - w) >> 1) : 0u;
    u8 oy = h < 64u ? (u8)((64u - h) >> 1) : 0u;
    if (ox > 0u && oy > 0u) {
        krx_fill_rect(api, (i16)(ox - 1u), (i16)(oy - 1u), (u8)(w + 2u), (u8)(h + 2u), COLOR_CLEAR);
    }
    krx_draw_rounded_box(api, ox, oy, w, h);
    if (out_origin) {
        out_origin[0] = ox;
        out_origin[1] = oy;
    }
}

static inline void krx_ui_draw_detail_row(
    const KrxApiTable* api, i16 y,
    const u8* title_packed, u16 t_len, const u8* sub, u16 s_len, u8 sel
) {
    u8 bg = sel ? COLOR_FILL : COLOR_CLEAR;
    u8 tc = sel ? COLOR_CLEAR : COLOR_FILL;
    krx_fill_rect(api, 2, y, 121u, 21u, bg);
    if (sel) krx_clear_corners(api, 2, y, 121u, 21u);
    if (title_packed && t_len) {
        u16 n = 0u;
        while (n < t_len && title_packed[n]) n++;
        krx_draw_str_max(api, 5, (i16)(y + 9), title_packed, (u8)(n > 255u ? 255u : n), KRX_FONT_MUMATRIX, tc, 115u);
    }
    if (sub && s_len) {
        krx_draw_str_max(api, 5, (i16)(y + 18), sub, (u8)(s_len > 255u ? 255u : s_len), KRX_FONT_TOMTHUMB, tc, 115u);
    }
}

// ── Inline Helpers: IO (Key / Power / Serial / Time) ─────────────────────────

static inline u8 krx_poll_key(const KrxApiTable* api) {
    return (u8)krx_call(api, OP_IO_KEY, SUB_IO_KEY_POLL, 0u, 0u);
}

static inline u32 krx_key_matrix(const KrxApiTable* api) {
    return krx_call(api, OP_IO_KEY, SUB_IO_KEY_MATRIX, 0u, 0u);
}

static inline void krx_wake_backlight(const KrxApiTable* api) {
    krx_prop_set32(api, KRX_PROP_BL_WAKE, 1u);
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
    return krx_prop_get32(api, KRX_PROP_TICKS_MS);
}

static inline u32 krx_uptime_secs(const KrxApiTable* api) {
    return krx_udivmod(api, krx_ticks_ms(api), 1000u, (u32*)0);
}

static inline void krx_delay_ms(const KrxApiTable* api, u32 ms) {
    krx_call(api, OP_SYS_CTL, SUB_SYS_CTL_DELAY_MS, ms, 0u);
}

#endif // KRX_H
