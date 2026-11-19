/*
 * KERN DFU ABI & Open Bootloader Handoff Protocol (KBHP v1.0)
 *
 * Vendor-neutral binary contract shared between calling firmware / `/apps/dfu.mod`,
 * the bare-metal SRAM SysTick hook (`hook/hook_defs.inc`), Stage-2 `kboot`, and
 * any third-party firmware or bootloader adopting KBHP v1.0 (see `docs/bootloader-protocol.md`).
 *
 * SRAM Map during DFU handoff (PY32F071xB, 16 KB: 0x2000_0000..0x2000_3FFF):
 *   0x2000_0000..0x2000_2FFF  Stock bootloader .data/.bss/stack (untouched)
 *   0x2000_3000..0x2000_327F  DFU_SRAM_BANNERS (8 x 80 B = 640 B, Page 5 subtitles)
 *   0x2000_3280..0x2000_33BF  DFU_SRAM_TITLES  (5 x 64 B = 320 B, Page 4 idle titles)
 *   0x2000_3540..0x2000_3565  DFU_SRAM_ARROW   (38 B, 2 x 19 B)
 *   0x2000_3580..0x2000_35A7  DFU_SRAM_VIDPID  (40 B)
 *   0x2000_35C0..0x2000_35FF  KBHP_SRAM_ADDR   (64 B, KBHP v1.0 Handoff Envelope)
 *   0x2000_3600..0x2000_36BF  DFU_SRAM_VTOR    (192 B, 48 relocated vectors)
 *   0x2000_36C0..0x2000_36FF  DFU_SRAM_STATE   (64 B, DfuHookState)
 *   0x2000_3700..0x2000_3FFF  DFU_SRAM_CODE    (2,304 B max, dfu_hook.bin)
 */
#pragma once
#include <krx.h>
#include <stddef.h>

/* ── Flash Partition Boundaries (PY32F071xB) ────────────────────────────────── */
#define DFU_BL_FLASH_BASE    0x08000000u
#define DFU_BL_CODE_LIMIT    0x08002700u /* 9,984 B bootloader code ceiling */
#define DFU_FACTORY_PAGE     0x08002700u /* 256 B factory provisioning page (NEVER WRITE) */
#define DFU_APP_FLASH_BASE   0x08002800u /* 118 KB application flash base */
#define DFU_APP_MAX_PAGES    472u        /* 472 x 256 B = 120,832 B (118 KB) */
#define DFU_FLASH_END        0x08020000u

/* ── Bootloader USB VID:PID & Target Identifiers ────────────────────────────── */
#define DFU_STOCK_VID_PID    0x36B70001u /* Quansheng factory K1/K5v3 bootloader */
#define DFU_MOTO_VID_PID     0x36B7FFFFu /* muzkr/moto UF2 MSC bootloader */
#define DFU_TARGET_UV_K1     0x51530101u /* Canonical KfwTargetDesc UV-K1/K5v3 ID */

/* ── Upper-SRAM Runtime Arena (invisible to stock BL & Moto) ────────────────── */
#define DFU_SRAM_BANNERS     0x20003000u /* 8 x 80 B = 640 B (Page 5) */
#define DFU_SRAM_TITLES      0x20003280u /* 5 x 64 B = 320 B (Page 4 idle titles) */
#define DFU_SRAM_ARROW       0x20003540u
#define DFU_SRAM_VIDPID      0x20003580u
#define KBHP_SRAM_ADDR       0x200035C0u
#define DFU_SRAM_VTOR        0x20003600u
#define DFU_SRAM_STATE       0x200036C0u
#define DFU_SRAM_CODE        0x20003700u
#define DFU_SRAM_CODE_MAX    0x00000900u /* 2,304 B */

/* Staging window inside TRANSIENT_ARENA[4160..5120): 960 B = 640 B banners + 320 B titles */
#define DFU_STAGE_OFFSET     4160u
#define DFU_BANNERS_BYTES    (8u * 80u)
#define DFU_TITLES_BYTES     (5u * 64u)

/* ── Backup-Domain RTC_ALR Bridge (zero-FW-cost MENU Tools wakeup) ──────────── */
#define KBHP_RTC_IDENT          0xA5u       /* Matches kernel::boot::BackupCrashTag::IDENT_BYTE */
#define KBHP_RTC_FLAG_CLEAN     (1u << 0)   /* FLAG_CLEAN_REBOOT */
#define KBHP_RTC_FLAG_BL_HOLD   (1u << 3)   /* FLAG_BOOTLOADER_HOLD */
#define KBHP_RTC_FLAG_TOOLS     (1u << 6)   /* DFU Tools menu request (preserved by OS boot) */
#define KBHP_RTC_TOOLS_WORD     0xA5004900u /* IDENT=0xA5 | CLEAN | BL_HOLD | TOOLS */

/* ── KBHP v1.0 Open Protocol Constants ──────────────────────────────────────── */
#define KBHP_IDENT           0x5048424Bu /* "KBHP" FourCC (little-endian) */
#define KBHP_ORIGIN_KERN     0x4E52454Bu /* "KERN" FourCC (calling firmware ID) */
#define KBHP_VERSION         0x0100u     /* v1.0 */
#define KBHP_HOOK_VERSION    1u
#define KBHP_HEADER_HASH_LEN 44u         /* Bytes [0x00..0x2C) sealed by header_checksum */

/* Handoff commands (firmware -> bootloader / hook) */
#define KBHP_CMD_ENTER_DFU      1u /* Standard USB/UART DFU mode */
#define KBHP_CMD_RESTORE_GOLDEN 2u /* Stage-2 kboot: restore /boot/kern_golden.bin */
#define KBHP_CMD_UART_RESCUE    3u /* Stage-2 kboot: enter UART rescue console */
#define KBHP_CMD_VERIFY_ONLY    4u /* Verify app flash integrity & reboot */
#define KBHP_CMD_MSC_DFU        5u /* Staged USB MSC virtual drive (.uf2/.kfw) */
#define KBHP_CMD_RAM_EXEC       6u /* Zero-wear SRAM live-boot handoff */

/* Detected primary bootloader family at 0x0800_0000 */
#define KBHP_BL_UNKNOWN         0u /* Unrecognized bootloader: validated raw handoff */
#define KBHP_BL_STOCK_K1        1u /* Verified Quansheng PY32F071 7.00..7.03: full hook */
#define KBHP_BL_STOCK_SAFE      2u /* Stock RVCT BL, unknown rev: safe guard-only hook */
#define KBHP_BL_MOTO            3u /* muzkr/moto UF2 MSC BL: raw handoff (PB10=0, PB14/15=1) */
#define KBHP_BL_KERN_STAGE2     4u /* KBHP-aware Stage-2 / custom bootloader */

/* Immutable policy & pre-flight flags */
#define KBHP_FLAG_HOOK_ACTIVE       (1u << 0)
#define KBHP_FLAG_ANTIBRICK_GUARD   (1u << 1)
#define KBHP_FLAG_FLASHLIGHT_LOCKED (1u << 2)
#define KBHP_FLAG_LOW_BATTERY       (1u << 3)
#define KBHP_FLAG_FACTORY_PAGE_OK   (1u << 4)
#define KBHP_FLAG_STAGE_ASSETS_OK   (1u << 5)

/* Live status codes mirrored by hook / KBHP-aware bootloader */
#define KBHP_STATUS_STANDBY         0u
#define KBHP_STATUS_USB_LINK        1u
#define KBHP_STATUS_READY           2u
#define KBHP_STATUS_LINK_ACTIVE     3u
#define KBHP_STATUS_FLASHING        4u
#define KBHP_STATUS_COMPLETE        5u
#define KBHP_STATUS_UART_LINK       6u
#define KBHP_STATUS_ABORTED         7u

/* Live / return error codes */
#define KBHP_ERR_NONE               0u
#define KBHP_ERR_STALLED            1u
#define KBHP_ERR_BAD_TARGET         2u
#define KBHP_ERR_LOW_BATTERY        3u
#define KBHP_ERR_BAD_INTEGRITY      4u
#define KBHP_ERR_ABORTED            5u

/* Live telemetry flags (KbhpEnvelope.flags_live) */
#define KBHP_LIVE_USB               (1u << 0)
#define KBHP_LIVE_UART              (1u << 1)
#define KBHP_LIVE_FLASH_DIRTY       (1u << 2)
#define KBHP_LIVE_VERIFY_OK         (1u << 3)

/* ── DfuHookState Flag Bits (must match hook/hook_defs.inc) ─────────────────── */
#define HOOK_FLAG_STOCK_K1      (1u << 0)
#define HOOK_FLAG_KEY_SEEN_HIGH (1u << 1)
#define HOOK_FLAG_USB_ACTIVE    (1u << 2)
#define HOOK_FLAG_WAS_FLASHING  (1u << 3)
#define HOOK_FLAG_ARROW_VIS     (1u << 4)
#define HOOK_FLAG_BL_OFF        (1u << 5)
#define HOOK_FLAG_NO_TAP_TOGGLE (1u << 6)
#define HOOK_FLAG_KBHP_VALID    (1u << 7)

/*
 * 64-byte (2^6) KBHP v1.0 Handoff Envelope at 0x2000_35C0.
 *
 * Partitioned into two strictly separated regions so live 30 Hz telemetry updates
 * NEVER invalidate the signed request/identity header:
 *   [0x00..0x2F] (48 B): Immutable Request & Identity Header sealed by header_checksum
 *   [0x30..0x3F] (16 B): Mutable Live Telemetry & Return Result Block
 */
typedef struct {
    /* ── Immutable Signed Header (48 B: 0x00..0x2F) ── */
    u32  ident;           /* +0x00: KBHP_IDENT ("KBHP") */
    u16  version;         /* +0x04: KBHP_VERSION (0x0100) */
    u8   cmd;             /* +0x06: KBHP_CMD_* */
    u8   bl_kind;         /* +0x07: KBHP_BL_* */
    u32  flags;           /* +0x08: KBHP_FLAG_* */
    u32  target_id;       /* +0x0C: Canonical KfwTargetDesc target_id */
    u32  origin_tag;      /* +0x10: FourCC of caller ("KERN", "KBOT", "MOTO", "F4HW", ...) */
    u32  bl_vid_pid;      /* +0x14: Detected bootloader USB VID:PID */
    u32  fw_version;      /* +0x18: Calling firmware packed SemVer */
    u16  battery_mv;      /* +0x1C: Pre-handoff battery voltage in mV (0 = unknown) */
    u16  app_max_pages;   /* +0x1E: Max 256 B app pages above base (472 = 118 KB) */
    char bl_ver[8];       /* +0x20: NUL-padded bootloader version/name */
    u32  app_flash_base;  /* +0x28: Protected app flash base (0x0800_2800) */
    u32  header_checksum; /* +0x2C: FNV-1a 32-bit digest over bytes [0x00..0x2C) */

    /* ── Live Telemetry & Return Block (16 B: 0x30..0x3F) ── */
    u8   status;          /* +0x30: Live KBHP_STATUS_* */
    u8   error_code;      /* +0x31: Live/return KBHP_ERR_* */
    u8   hook_ver;        /* +0x32: Attached hook / KBHP responder version */
    u8   flags_live;      /* +0x33: Live KBHP_LIVE_* bits */
    u16  flashed_pages;   /* +0x34: Current/final flashed page count */
    u16  total_pages;     /* +0x36: Total image pages reported by host/BL */
    u16  elapsed_sec;     /* +0x38: Completed flash duration in seconds (0xFFFF = unset) */
    u16  boot_strikes;    /* +0x3A: Mirrored RTC_ALR strike count */
    u32  hook_state_addr; /* +0x3C: Pointer to DfuHookState (0x2000_36C0) or 0 */
} KbhpEnvelope;

_Static_assert(sizeof(KbhpEnvelope) == 64u, "KbhpEnvelope must be exactly 64 bytes (2^6)");
_Static_assert(offsetof(KbhpEnvelope, header_checksum) == 0x2Cu, "KBHP header_checksum offset mismatch");
_Static_assert(offsetof(KbhpEnvelope, status) == 0x30u, "KBHP_OFF_STATUS mismatch");
_Static_assert(offsetof(KbhpEnvelope, error_code) == 0x31u, "KBHP_OFF_ERR mismatch");
_Static_assert(offsetof(KbhpEnvelope, flags_live) == 0x33u, "KBHP_OFF_LIVE mismatch");
_Static_assert(offsetof(KbhpEnvelope, flashed_pages) == 0x34u, "KBHP_OFF_PAGES mismatch");
_Static_assert(offsetof(KbhpEnvelope, total_pages) == 0x36u, "KBHP_OFF_TOTAL mismatch");
_Static_assert(offsetof(KbhpEnvelope, elapsed_sec) == 0x38u, "KBHP_OFF_ELAPSED mismatch");
_Static_assert(offsetof(KbhpEnvelope, hook_state_addr) == 0x3Cu, "KBHP hook_state_addr offset mismatch");

/*
 * 64-byte (2^6) Live SysTick Hook Runtime State at 0x2000_36C0.
 * Offsets are compile-time verified against `src/hook/hook_defs.inc`.
 */
typedef struct {
    u32 flags;            /* +0x00 (0)  */
    u32 key_hold_ticks;   /* +0x04 (4)  */
    u32 tick;             /* +0x08 (8)  */
    u32 last_capsule;     /* +0x0C (12) */
    u16 last_usb_frame;   /* +0x10 (16) */
    u8  usb_miss_cnt;     /* +0x12 (18) */
    u8  uart_timer;       /* +0x13 (19) */
    u32 kbhp_addr;        /* +0x14 (20) */
    u32 flash_start_tick; /* +0x18 (24) */
    u16 last_banner;      /* +0x1C (28) */
    u8  last_led;         /* +0x1E (30) */
    u8  serial_pulse;     /* +0x1F (31) */
    u32 last_serial_hash; /* +0x20 (32) */
    u16 flash_elapsed_s;  /* +0x24 (36) */
    u16 last_cutout;      /* +0x26 (38) */
    u32 last_top_key;     /* +0x28 (40) */
    u16 last_flash_cur;   /* +0x2C (44) */
    u16 stall_ticks;      /* +0x2E (46) */
    u16 prev_cndtr;       /* +0x30 (48) */
    u16 _pad0;            /* +0x32 (50) */
    u32 _reserved[3];     /* +0x34..0x3F (52..63) */
} DfuHookState;

_Static_assert(sizeof(DfuHookState) == 64u, "DfuHookState must be exactly 64 bytes (2^6)");
_Static_assert(offsetof(DfuHookState, flags) == 0u, "ST_FLAGS mismatch");
_Static_assert(offsetof(DfuHookState, key_hold_ticks) == 4u, "ST_KEY_HOLD mismatch");
_Static_assert(offsetof(DfuHookState, tick) == 8u, "ST_TICK mismatch");
_Static_assert(offsetof(DfuHookState, last_capsule) == 12u, "ST_LAST_CAPSULE mismatch");
_Static_assert(offsetof(DfuHookState, last_usb_frame) == 16u, "ST_USB_FRAME mismatch");
_Static_assert(offsetof(DfuHookState, usb_miss_cnt) == 18u, "ST_USB_MISS mismatch");
_Static_assert(offsetof(DfuHookState, uart_timer) == 19u, "ST_UART_TIMER mismatch");
_Static_assert(offsetof(DfuHookState, kbhp_addr) == 20u, "ST_KBHP_ADDR mismatch");
_Static_assert(offsetof(DfuHookState, flash_start_tick) == 24u, "ST_FLASH_START mismatch");
_Static_assert(offsetof(DfuHookState, last_banner) == 28u, "ST_LAST_BANNER mismatch");
_Static_assert(offsetof(DfuHookState, last_led) == 30u, "ST_LAST_LED mismatch");
_Static_assert(offsetof(DfuHookState, serial_pulse) == 31u, "ST_SERIAL_PULSE mismatch");
_Static_assert(offsetof(DfuHookState, last_serial_hash) == 32u, "ST_SERIAL_HASH mismatch");
_Static_assert(offsetof(DfuHookState, flash_elapsed_s) == 36u, "ST_FLASH_ELAPSED mismatch");
_Static_assert(offsetof(DfuHookState, last_cutout) == 38u, "ST_LAST_CUTOUT mismatch");
_Static_assert(offsetof(DfuHookState, last_top_key) == 40u, "ST_LAST_TOP mismatch");
_Static_assert(offsetof(DfuHookState, last_flash_cur) == 44u, "ST_FLASH_CUR mismatch");
_Static_assert(offsetof(DfuHookState, stall_ticks) == 46u, "ST_STALL_TICKS mismatch");
_Static_assert(offsetof(DfuHookState, prev_cndtr) == 48u, "ST_PREV_CNDTR mismatch");
