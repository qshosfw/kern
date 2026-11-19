/*
 * KERN DFU Shared Module State & Inline Primitives (Pure 4-Leaf DFU Module)
 */
#pragma once
#include "dfu_abi.h"

#define KRX_FONT_TOMTHUMB 0u
#define KRX_ALIGN_LEFT    0u
#define KRX_ALIGN_CENTER  1u
#define KRX_ALIGN_RIGHT   2u

#define DFU_FLASH_PREFIX_X   24u
#define TAG_CONFIRM_DFU      1u

/* P0 Overlay Leaf IDs (4 dedicated DFU leaves) */
#define DFU_LEAF_SPLASH   1u
#define DFU_LEAF_MODAL    2u
#define DFU_LEAF_STAGE    3u
#define DFU_LEAF_PROBE    4u

typedef struct {
    void (*softbar)(const KrxApiTable* api, const char* lk, const char* ll, const char* rk, const char* rl);
} DfuUiOps;

typedef struct {
    KbhpEnvelope kbhp;             /* 64 B: Prepared KBHP v1.0 handoff envelope */
    u32          tramp[3];         /* 12 B: [0]=MSP, [1]=SystemInit, [2]=__main */
    u32          op;               /* 4 B:  KRX_OP_* forwarded to modal leaf */
    const void*  param;            /* 4 B:  KRX event/param forwarded to modal leaf */
    u8           keys_held;        /* 1 B:  Non-zero if boot chord keys still pressed */
    u8           has_stage;        /* 1 B:  Non-zero if SRAM UI strips were staged */
    u8           use_hook;         /* 1 B:  Non-zero if SysTick hook should attach */
    u8           has_recovery;     /* 1 B:  Non-zero if /apps/recovery.mod is installed */
    DfuUiOps     ui;               /* 4 B:  Resident softbar helper */
    u8*          stage_buf;        /* 4 B:  TRANSIENT_ARENA[4160..5120) = 960 B strip staging */
    u8           sram_arrow[38];   /* 38 B: 2 x 19 B connection arrow */
    u8           sram_vidpid[40];  /* 40 B: Page 1 USB VID:PID badge */
} DfuSharedState;

static inline void dfu_copy_bytes(u8* dst, const u8* src, u32 len) {
    for (u32 i = 0; i < len; i++) dst[i] = src[i];
}

static inline void dfu_zero_bytes(u8* dst, u32 len) {
    for (u32 i = 0; i < len; i++) dst[i] = 0u;
}

static inline u32 kbhp_checksum(const KbhpEnvelope* env) {
    const u8* p = (const u8*)env;
    u32 h = 0x811C9DC5u;
    for (u32 i = 0; i < KBHP_HEADER_HASH_LEN; i++) {
        h ^= p[i];
        h *= 0x01000193u;
    }
    return h;
}
