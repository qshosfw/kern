/*
 * KERN Multiboot & Golden Backup Manager Shared State (`/apps/multiboot.mod`)
 */
#pragma once
#include <krx.h>
#include <stddef.h>

#define KRX_FONT_TOMTHUMB 0u
#define KRX_ALIGN_LEFT    0u
#define KRX_ALIGN_CENTER  1u
#define KRX_ALIGN_RIGHT   2u

#define MB_APP_FLASH_BASE 0x08002800u
#define MB_APP_MAX_PAGES  472u
#define MB_APP_MAX_BYTES  120832u
#define MB_FLASH_END      0x08020000u
#define MB_MAX_SLOTS      12u
#define MB_SCRATCH_OFFSET 3584u /* TRANSIENT_ARENA[3584..5120) = 1,536 B scratch */

/* P0 Overlay Leaf IDs */
#define MB_LEAF_MENU      1u
#define MB_LEAF_STATUS    2u
#define MB_LEAF_BACKUP    3u
#define MB_LEAF_SLOTS     4u
#define MB_LEAF_INSPECT   5u
#define MB_LEAF_FLASH_CHK 6u
#define MB_LEAF_FLASH_UI  7u
#define MB_LEAF_FLASH_GO  8u

/* Cooperative exit codes */
#define MB_EXIT_BACK      0u
#define MB_EXIT_DFU       2u

/* Flashable image formats */
#define FLASH_FMT_NONE    0u
#define FLASH_FMT_RAW     1u
#define FLASH_FMT_KFW_RAW 2u
#define FLASH_FMT_KFW_RLE 3u
#define FLASH_FMT_UF2     4u

typedef struct {
    char name[20];
    u32  size;
} MbSlot;

typedef struct {
    KrxFileExtents ext;         /* 20 B */
    u32            data_off;
    u32            stored_size;
    u32            raw_size;
    u8             fmt;
    u8             is_golden;
    u8             _pad[2];
    u8             page0[256];  /* pre-verified Page 0 (0x0800_2800), programmed LAST */
} MbFlashJob;

typedef struct {
    MbSlot     slots[MB_MAX_SLOTS]; /* 288 B */
    u32        sel_size;            /* 4 B */
    char       sel_path[32];        /* 32 B */
    u8         slot_count;          /* 1 B */
    u8         slot_sel;            /* 1 B */
    u8         _pad[2];             /* 2 B */
    MbFlashJob job;                 /* 292 B */
} MbScratch;                        /* 620 B <= 1,536 B */

_Static_assert(sizeof(MbScratch) <= (5120u - MB_SCRATCH_OFFSET),
               "MbScratch must fit before 5120 B P0 leaf window");

struct MbState;

typedef struct {
    void        (*softbar)(const KrxApiTable* api, const char* lk, const char* ll, const char* rk, const char* rl);
    void        (*kv)(const KrxApiTable* api, i16 y, const char* k, const char* v);
    void        (*draw_ctr)(const KrxApiTable* api, i16 y, const char* s);
    void        (*draw_box)(const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, u8 c);
    u8          (*wait_key)(const KrxApiTable* api);
    u32         (*u32_dec)(char* dst, u32 val);
    const char* (*nth_s)(const char* tbl, u8 idx);
    void        (*fmt_hex32)(char* dst, u32 val);
    u16         (*rd_at)(const KrxApiTable* api, const char* path, u8 plen, u32 off, u8* buf, u16 len);
    u16         (*pwr_get)(const KrxApiTable* api, u8 sub);
    u32         (*crc32_step)(u32 crc, const u8* data, u32 len);
    void        (*probe)(const KrxApiTable* api, struct MbState* st);
    /* SRAM-safe bare-metal helpers (live in resident root SRAM, zero internal-flash fetches) */
    void        (*py25_read)(u32 addr, u8* dst, u32 len);
    void        (*lcd_bar)(u32 done, u32 total);
} MbOps;

typedef struct MbState {
    MbOps      ui;
    MbScratch* sc;
    u32        app_bytes;
    u32        app_crc;
    u32        golden_bytes;
    u32        golden_crc;
    u8         boot_mode;     /* 1 = early boot / Recovery chain, 0 = Settings OS */
    u8         has_dfu;
    u8         has_recovery;
    u8         vec_ok;
    u8         golden_ok;
    u8         golden_synced;
    u8         menu_sel;
    u8         next_leaf;
    u8         exit_code;
    u8         flash_err;     /* 0=OK, 1=LowBatt, 2=ExtErr, 3=DecodeErr, 4=BadVec, 5=NoGolden */
} MbState;

static inline void mb_copy(u8* d, const u8* s, u32 n) {
    for (u32 i = 0u; i < n; i++) d[i] = s[i];
}

static inline void mb_zero(u8* d, u32 n) {
    for (u32 i = 0u; i < n; i++) d[i] = 0u;
}
