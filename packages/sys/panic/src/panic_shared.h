#ifndef PANIC_SHARED_H
#define PANIC_SHARED_H

#include <krx.h>

#define FONT_TOMTHUMB 0u
#define ALIGN_LEFT    0u
#define ALIGN_CENTER  1u
#define ALIGN_RIGHT   2u

#define BOOT_IDENT_0  0xC0u
#define BOOT_IDENT_1  0xDEu
#define BOOT_IDENT_2  0x02u
#define BOOT_IDENT_3  0xFDu

#define FAULT_EXTRA_IDENT 0xF007CAFEu

// ResetReason FourCCs (LE u32)
#define REASON_HARD 0x44524148u
#define REASON_APPF 0x46505041u
#define REASON_WDOG 0x474F4457u
#define REASON_TAGS 0x53474154u
#define REASON_BROW 0x574F5242u

// Module execution modes
#define MODE_IN_OS       0u
#define MODE_BOOT_DIALOG 1u
#define MODE_SAFE_MENU   2u
#define MODE_INSPECT     3u

// Panic pages (page-based navigation, no line scrolling)
#define PAGE_MAIN  0u
#define PAGE_REGS  1u
#define PAGE_EXT   2u
#define PAGE_STACK 3u
#define PAGE_ASM   4u
#define PAGE_COUNT 5u

// Auto-continue timeout (seconds) on post-fault panic screen
#define PANIC_TIMEOUT_SECS  10u
#define PANIC_HOLD_SENTINEL 255u

// Report target flags
#define REPORT_SERIAL 0u
#define REPORT_FILE   1u

// P0 Leaf IDs (1-based, matches krx.toml `leaves` order)
#define LEAF_PREP       1u
#define LEAF_CAUSE      2u
#define LEAF_DISASM     3u
#define LEAF_PAGE_MAIN  4u
#define LEAF_PAGE_INFO  5u
#define LEAF_PAGE_REGS  6u
#define LEAF_PAGE_STACK 7u
#define LEAF_PAGE_ASM   8u
#define LEAF_SAFE_MENU  9u
#define LEAF_REPORT     10u

typedef struct __attribute__((packed)) {
    u32 crash_signature;
    u16 inst_pre;
    u8  fault_reg;
    u8  access_flags;
    u8  ipsr_num;
    u8  exec_mode;
    u8  unwind_count;
    u8  _pad;
} FaultForensics; // 12 B

typedef struct __attribute__((packed)) {
    u32 reset_reason;
    u32 fault_pc;
    u32 fault_lr;
    u32 fault_sp;
    u32 xpsr;
    u32 panic_file_hash;
    u32 stack_dump[16];
    u8  actor_tag[16];
    FaultForensics forensics;
    u8  ident[4];
    u16 panic_line;
    u8  crash_count;
    u8  trap_code;
    u32 crc32;
} BootSlot; // 128 B

typedef struct __attribute__((packed)) {
    BootSlot slot_a;
    BootSlot slot_b;
    u8 active_slot;
    u8 _pad[3];
} BootRecord; // 260 B

typedef struct __attribute__((packed)) {
    u32 callee[8]; // R4..R11
    u32 msp;
    u32 psp;
    u32 rcc_csr;
    u32 icsr;
    u32 valid;
} FaultExtraRegs; // 52 B

typedef struct __attribute__((packed)) {
    BootRecord     rec;        // 260 B
    FaultExtraRegs extra;      // 52 B
    u32            backup_tag; // 4 B
} ProcBootSnapshot; // 316 B

typedef struct {
    u16 opcodes[5];    // [PC-4, PC-2, PC, PC+2, PC+4]
    u8  op_valid[5];   // 1 if safely loaded from Flash/slot
    u8  fault_reg;
    u32 fault_addr;
    u32 crash_sig;
    char actor[16];    // sanitized printable ASCII actor tag
    char line1[28];    // human-readable cause headline
    char line2[28];    // effective address / detail
    char asm_txt[5][20]; // disassembled Thumb-16 window [PC-4..PC+4]
} FaultAnalysis;

typedef struct {
    ProcBootSnapshot snap;
    BootSlot         slot;
    FaultAnalysis    ana;
    u8  mode;
    u8  page;        // 0..PAGE_COUNT-1
    u8  has_slot;
    u8  extra_applies;
    u8  menu_sel;
    u8  boot_rem;    // 0..PANIC_TIMEOUT_SECS, or PANIC_HOLD_SENTINEL when paused
    u8  report_dst;  // REPORT_SERIAL or REPORT_FILE
    u8  dirty;
    u8  toast_ticks;
    char toast_msg[24];
} PanicState;

static inline u8 p_len(const char* s) {
    u8 n = 0;
    while (s[n]) n++;
    return n;
}

static inline const char* p_nth(const char* p, u8 idx) {
    while (idx > 0u) {
        while (*p) p++;
        p++;
        idx--;
    }
    return p;
}

static inline char p_hex_nib(u8 v) {
    v &= 0x0Fu;
    return (char)(v < 10u ? ('0' + v) : ('A' + (v - 10u)));
}

static inline u8 p_put_hex(char* out, u32 v, u8 digits) {
    for (i8 i = (i8)(digits - 1u); i >= 0; i--) {
        out[(u8)i] = p_hex_nib((u8)(v & 0xFu));
        v >>= 4;
    }
    return digits;
}

static inline u8 p_put_hex0x(char* out, u32 v, u8 digits) {
    out[0] = '0';
    out[1] = 'x';
    p_put_hex(out + 2, v, digits);
    return (u8)(digits + 2u);
}

static inline u8 p_put_uint(const KrxApiTable* api, char* out, u32 v) {
    char tmp[10];
    u8 n = 0;
    do {
        u32 r = 0;
        v = krx_udivmod(api, v, 10u, &r);
        tmp[n++] = (char)('0' + r);
    } while (v > 0u && n < 10u);
    for (u8 i = 0; i < n; i++) out[i] = tmp[n - 1u - i];
    return n;
}

static inline u8 p_app_s(char* o, u8 p, const char* s) {
    while (*s) o[p++] = *s++;
    return p;
}

static inline u8 p_put_reg(char* o, u8 r) {
    if (r < 10u) { o[0] = 'r'; o[1] = (char)('0' + r); return 2u; }
    if (r == 12u) { o[0] = 'i'; o[1] = 'p'; return 2u; }
    if (r == 13u) { o[0] = 's'; o[1] = 'p'; return 2u; }
    if (r == 14u) { o[0] = 'l'; o[1] = 'r'; return 2u; }
    if (r == 15u) { o[0] = 'p'; o[1] = 'c'; return 2u; }
    o[0] = 'r'; o[1] = '1'; o[2] = (char)('0' + (r - 10u));
    return 3u;
}

static inline u8 p_copy_cstr(char* dst, const char* src, u8 max_len) {
    u8 i = 0;
    while (i < max_len && src[i]) {
        dst[i] = src[i];
        i++;
    }
    dst[i] = 0;
    return i;
}

static inline u8 p_slot_valid(const KrxApiTable* api, const BootSlot* s) {
    if (s->ident[0] != BOOT_IDENT_0 || s->ident[1] != BOOT_IDENT_1 ||
        s->ident[2] != BOOT_IDENT_2 || s->ident[3] != BOOT_IDENT_3) {
        return 0u;
    }
    return (krx_crc32(api, (const u8*)s, 124u) == s->crc32) ? 1u : 0u;
}

static inline const BootSlot* p_latest_slot(const KrxApiTable* api, const BootRecord* rec) {
    u8 va = p_slot_valid(api, &rec->slot_a);
    u8 vb = p_slot_valid(api, &rec->slot_b);
    if (va && vb) {
        return (rec->active_slot == 0u) ? &rec->slot_a : &rec->slot_b;
    }
    if (va) return &rec->slot_a;
    if (vb) return &rec->slot_b;
    return (const BootSlot*)0;
}

static inline u8 p_is_safe_flash_hw(u32 addr) {
    return ((addr & 1u) == 0u && addr >= 0x08000000u && addr <= 0x0801FFF8u) ? 1u : 0u;
}

static inline const char* p_reason_name(u32 rr) {
    if (rr == REASON_HARD) return "HARDFAULT";
    if (rr == REASON_APPF) return "PANIC";
    if (rr == REASON_WDOG) return "WATCHDOG";
    if (rr == REASON_TAGS) return "MEM FAULT";
    if (rr == REASON_BROW) return "BROWNOUT";
    return "FAULT";
}

#endif // PANIC_SHARED_H
