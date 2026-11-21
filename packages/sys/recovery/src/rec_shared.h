/*
 * KERN Recovery Module Shared State & Resident Helper Table (`/apps/recovery.mod`)
 */
#pragma once
#include <krx.h>
#include <stddef.h>

#define KRX_FONT_TOMTHUMB 0u
#define KRX_ALIGN_LEFT    0u
#define KRX_ALIGN_CENTER  1u
#define KRX_ALIGN_RIGHT   2u

#define REC_APP_FLASH_BASE 0x08002800u
#define REC_APP_MAX_PAGES  472u
#define REC_FLASH_END      0x08020000u
#define REC_MAX_FILES      12u
#define REC_SCRATCH_OFFSET 3584u /* TRANSIENT_ARENA[3584..5120) = 1,536 B scratch */

/* P0 Overlay Leaf IDs */
#define REC_LEAF_MENU      1u
#define REC_LEAF_INFO      2u
#define REC_LEAF_PEER      3u
#define REC_LEAF_SCAN      4u
#define REC_LEAF_FILES     5u
#define REC_LEAF_INSPECT   6u
#define REC_LEAF_CONSOLE   7u
#define REC_LEAF_FLASH_CHK 8u
#define REC_LEAF_FLASH_UI  9u
#define REC_LEAF_FLASH_GO  10u

/* Cooperative exit codes returned to main.rs boot-module chain */
#define REC_EXIT_OS        0u
#define REC_EXIT_DFU       2u
#define REC_EXIT_MULTIBOOT 3u
#define REC_EXIT_CALIB     4u

/* Image formats supported by the on-device SRAM Page-0-Last Flasher */
#define FLASH_FMT_NONE     0u
#define FLASH_FMT_RAW      1u /* Raw ARM .bin */
#define FLASH_FMT_KFW_RAW  2u /* .kfw v1.0 PART_FIRMWARE COMP_NONE */
#define FLASH_FMT_KFW_RLE  3u /* .kfw v1.0 PART_FIRMWARE COMP_PACKBITS_RLE (--mcu-ota) */
#define FLASH_FMT_UF2      4u /* UF2 blocks (512 B, 256 B payload) */

typedef struct {
    char name[20];
    u32  size;
    u8   dir_idx; /* 0="/boot/", 1="/apps/", 2="/user/", 3="/" */
    u8   _pad[3];
} RecFlatFile;

typedef struct {
    KrxFileExtents ext;         /* 20 B: KernFS depth, file size, l1[8] SPI2 pages */
    u32            data_off;    /* byte offset of firmware payload in file */
    u32            stored_size; /* stored payload bytes in file */
    u32            raw_size;    /* uncompressed firmware bytes (<= 120,832 B) */
    u8             fmt;         /* FLASH_FMT_* */
    u8             _pad[3];
    u8             page0[256];  /* pre-verified Page 0 (0x0800_2800), programmed LAST */
} RecFlashJob;

typedef struct {
    RecFlatFile files[REC_MAX_FILES]; /* 336 B */
    u32         sel_size;             /* 4 B */
    char        sel_path[32];         /* 32 B */
    u8          file_count;           /* 1 B */
    u8          file_sel;             /* 1 B */
    u8          _pad[2];              /* 2 B */
    RecFlashJob job;                  /* 292 B */
    char        con_ring[18 * 32];    /* 576 B: live-streaming applet console */
} RecScratch;                         /* 1,244 B <= 1,536 B scratch */

_Static_assert(sizeof(RecScratch) <= (5120u - REC_SCRATCH_OFFSET),
               "RecScratch must fit before 5120 B P0 leaf window");

typedef struct {
    void        (*softbar)(const KrxApiTable* api, const char* lk, const char* ll, const char* rk, const char* rl);
    void        (*kv)(const KrxApiTable* api, i16 y, const char* k, const char* v);
    void        (*draw_ctr)(const KrxApiTable* api, i16 y, const char* s);
    void        (*draw_box)(const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, u8 c);
    u8          (*wait_key)(const KrxApiTable* api);
    u32         (*u32_dec)(char* dst, u32 val);
    const char* (*nth_s)(const char* tbl, u8 idx);
    void        (*fmt_hex)(char* dst, const u8* src, u32 n);
    u16         (*rd_at)(const KrxApiTable* api, const char* path, u8 plen, u32 off, u8* buf, u16 len);
    u16         (*pwr_get)(const KrxApiTable* api, u8 sub);
    /* SRAM-safe bare-metal helpers (live in resident root SRAM, zero internal-flash fetches) */
    void        (*py25_read)(u32 addr, u8* dst, u32 len);
    void        (*lcd_bar)(u32 done, u32 total);
    u32         (*probe_bl)(char* bl_out, u32* app_kb_out, u8* vec_ok_out);
} RecOps;

typedef struct {
    RecOps      ui;
    RecScratch* sc;
    u8          menu_sel;
    u8          tool_sub;       /* 0 = Flash Image, 1 = Applets */
    u8          next_leaf;
    u8          exit_code;      /* REC_EXIT_* */
    u8          has_dfu;
    u8          has_multiboot;
    u8          has_calib;
    u8          flash_err;      /* 0=OK, 1=LowBatt, 2=ExtErr, 3=DecodeErr, 4=BadVec */
    u8          boot_mode;      /* 1 = early-boot DFU chain, 0 = launched from Settings OS */
} RecState;

static inline void rec_copy(u8* d, const u8* s, u32 n) {
    for (u32 i = 0u; i < n; i++) d[i] = s[i];
}

static inline void rec_zero(u8* d, u32 n) {
    for (u32 i = 0u; i < n; i++) d[i] = 0u;
}
