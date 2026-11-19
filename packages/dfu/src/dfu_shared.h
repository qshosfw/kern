/*
 * KERN DFU Shared Module State, Resident UI Dispatch Table & Inline Primitives
 */
#pragma once
#include "dfu_abi.h"

#define KRX_FONT_TOMTHUMB 0u
#define KRX_ALIGN_LEFT    0u
#define KRX_ALIGN_CENTER  1u
#define KRX_ALIGN_RIGHT   2u

#define DFU_FLASH_PREFIX_X   24u
#define TAG_CONFIRM_DFU      1u
#define DFU_MAX_FILES        12u

/* P0 Overlay Leaf IDs (StructuredLeafManager<16> supports 1..=16) */
#define DFU_LEAF_SPLASH   1u
#define DFU_LEAF_MODAL    2u
#define DFU_LEAF_STAGE    3u
#define DFU_LEAF_PROBE    4u
#define DFU_LEAF_TOOLS    5u
#define DFU_LEAF_INFO     6u
#define DFU_LEAF_RESCUE   7u
#define DFU_LEAF_SCAN     8u
#define DFU_LEAF_FILES    9u
#define DFU_LEAF_INSPECT  10u
#define DFU_LEAF_CONSOLE  11u

/* Recovery menu dispatch actions */
#define DFU_TOOL_ACT_DFU       0u /* Return to live DFU bootloader handoff */
#define DFU_TOOL_ACT_OPEN      1u /* Open selected sub-leaf */

/* Recovery menu items */
#define DFU_TOOL_ITEM_FLASH    0u /* Flat firmware image browser (.kfw/.uf2/.bin) */
#define DFU_TOOL_ITEM_APPLETS  1u /* Flat applet browser (.krx/.ram/.mod) */
#define DFU_TOOL_ITEM_PEER     2u /* Peer Flash (2-pin radio-to-radio transfer) */
#define DFU_TOOL_ITEM_INFO     3u /* Bootloader & hardware information */
#define DFU_TOOL_ITEM_COUNT    4u

typedef struct {
    char name[20];
    u32  size;
    u8   dir_idx; /* 0="/boot/", 1="/apps/", 2="/" */
    u8   _pad[3];
} DfuFlatFile;

/*
 * Transient scratch hosted inside TRANSIENT_ARENA[4160..5120) (`st->stage_buf`, 960 B):
 *   - During Recovery menu: holds DfuToolsScratch (380 B) + applet console log ring
 *   - During DFU staging:   holds 640 B Page-5 banners + 320 B Page-4 titles (960 B)
 * Zero bytes added to root.bin!
 */
typedef struct {
    DfuFlatFile files[DFU_MAX_FILES]; /* 336 B */
    u32         sel_size;             /* 4 B */
    char        sel_path[32];         /* 32 B */
    u8          file_count;           /* 1 B */
    u8          file_sel;             /* 1 B */
    u8          _pad[2];              /* 2 B */
} DfuToolsScratch;

_Static_assert(sizeof(DfuToolsScratch) <= (DFU_BANNERS_BYTES + DFU_TITLES_BYTES),
               "DfuToolsScratch must fit inside 960 B stage_buf");

typedef struct {
    void (*softbar)(const KrxApiTable* api, const char* lk, const char* ll, const char* rk, const char* rl);
    void (*kv)(const KrxApiTable* api, i16 y, const char* k, const char* v);
    u8   (*wait_key)(const KrxApiTable* api);
    u32  (*u32_dec)(char* dst, u32 val);
} DfuUiOps;

typedef struct {
    KbhpEnvelope kbhp;             /* 64 B: Prepared KBHP v1.0 handoff envelope */
    u32          tramp[3];         /* 12 B: [0]=MSP, [1]=SystemInit, [2]=__main */
    u32          op;               /* 4 B:  KRX_OP_* forwarded to modal leaf */
    const void*  param;            /* 4 B:  KRX event/param forwarded to modal leaf */
    u8           keys_held;        /* 1 B:  Non-zero if boot chord keys still pressed */
    u8           has_stage;        /* 1 B:  Non-zero if SRAM UI strips were staged */
    u8           use_hook;         /* 1 B:  Non-zero if SysTick hook should attach */
    u8           tool_sel;         /* 1 B:  Active selection in Recovery menu */
    u8           tool_sub;         /* 1 B:  Sub-mode (0=Flash Image, 1=Applets) */
    u8           tool_action;      /* 1 B:  DFU_TOOL_ACT_* dispatch result */
    u8           tools_active;     /* 1 B:  Non-zero once RTC tools flag was consumed */
    u8           next_leaf;        /* 1 B:  Next P0 leaf continuation (0 = back to Recovery menu) */
    DfuUiOps     ui;               /* 16 B: Resident shared UI helpers */
    u8*          stage_buf;        /* 4 B:  TRANSIENT_ARENA[4160..5120) = 960 B scratch/staging */
    u8           sram_arrow[38];   /* 38 B: 2 x 19 B connection arrow */
    u8           sram_vidpid[40];  /* 40 B: Page 1 USB VID:PID badge */
} DfuSharedState;

typedef struct {
    u8  name[24];
    u32 size;
    u8  is_dir;
    u8  reserved[3];
} DfuDirEntry;

_Static_assert(sizeof(DfuDirEntry) == 32u, "DfuDirEntry must match kernel KrxDirEntry (32 B)");

static inline void dfu_copy_bytes(u8* dst, const u8* src, u32 len) {
    for (u32 i = 0; i < len; i++) {
        dst[i] = src[i];
    }
}

static inline void dfu_zero_bytes(u8* dst, u32 len) {
    for (u32 i = 0; i < len; i++) {
        dst[i] = 0u;
    }
}

static inline u32 kbhp_checksum(const KbhpEnvelope* env) {
    const u8* p = (const u8*)env;
    u32 h = 0x811C9DC5u; /* FNV-1a 32-bit offset basis */
    for (u32 i = 0; i < KBHP_HEADER_HASH_LEN; i++) {
        h ^= p[i];
        h *= 0x01000193u;
    }
    return h;
}
