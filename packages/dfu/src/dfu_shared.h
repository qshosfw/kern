#pragma once
#include "krx.h"

#define KRX_FONT_TOMTHUMB 0u
#define KRX_ALIGN_LEFT    0u
#define KRX_ALIGN_CENTER  1u
#define KRX_ALIGN_RIGHT   2u

#define DFU_SRAM_BANNERS 0x20003000u
#define DFU_SRAM_ARROW   0x20003540u
#define DFU_SRAM_VIDPID  0x20003580u

#define DFU_FLASH_PREFIX_X   22u
#define DFU_FLASH_DIGITS_COL 76u

#define TAG_CONFIRM_DFU   1
#define TAG_CONFIRM_BLANK 2

typedef struct {
    u32 vid_pid;
    u32 op;
    const void* param;
    u8  sel;
    u8  action; // 0 = none, 1 = enter_dfu, 2 = blank_and_dfu
    u8  keys_held;
    u8  has_stage;
    u8  sram_banners[8 * 168];
    u8  sram_arrow[38];
    u8  sram_vidpid[40];
} DfuSharedState;

static inline void dfu_copy_bytes(u8* dst, const u8* src, u32 len) {
    for (u32 i = 0; i < len; i++) {
        dst[i] = src[i];
    }
}

static inline void dfu_read_bl_version(char out[8]) {
    const u8* rom = (const u8*)0x08001F00u;
    for (u32 i = 0; i + 8u <= 0x0800u; i++) {
        const u8* w = rom + i;
        if (w[1] == '.' && w[4] == '.' && w[7] == 0 &&
            w[0] >= '0' && w[0] <= '9' &&
            w[2] >= '0' && w[2] <= '9' &&
            w[3] >= '0' && w[3] <= '9' &&
            w[5] >= '0' && w[5] <= '9' &&
            w[6] >= '0' && w[6] <= '9') {
            dfu_copy_bytes((u8*)out, w, 8);
            return;
        }
    }
    dfu_copy_bytes((u8*)out, (const u8*)"7.02.02", 8);
}
