/*
 * P0 Leaf 12: Crash Splash Top Bar, Animated Sad-Radio Art & Micro Telemetry (<= 1 KB).
 */
#include "panic_shared.h"

// 24x27 UV-K1 handheld silhouette with 15x11 white LCD cutout + speaker slits
static const u8 SAD_RADIO_BMP[81] = {
    0x02,0x00,0x00, 0x07,0x00,0x00, 0x07,0x00,0x00, 0x07,0x00,0x00,
    0x07,0x00,0x00, 0x1F,0xFF,0xF0, 0x3F,0xFF,0xF8, 0x3F,0xFF,0xF8,
    0x78,0x00,0x38, 0x78,0x00,0x38, 0x78,0x00,0x38, 0x78,0x00,0x38,
    0x78,0x00,0x38, 0x78,0x00,0x38, 0x38,0x00,0x38, 0x38,0x00,0x38,
    0x38,0x00,0x38, 0x38,0x00,0x38, 0x38,0x00,0x38, 0x3F,0xFF,0xF8,
    0x3F,0xFF,0xF8, 0x3C,0x00,0x78, 0x3F,0xFF,0xF8, 0x3C,0x00,0x78,
    0x3F,0xFF,0xF8, 0x3F,0xFF,0xF8, 0x1F,0xFF,0xF0,
};

// Packed 1px points [x, y]: frown (9), idle 2x2 eyes + spark (10), glitch X-eyes + spark (12)
static const u8 FROWN_PTS[9][2] = {
    {11,24}, {12,24}, {13,24}, {14,24}, {15,24}, {10,25}, {16,25}, {9,26}, {17,26},
};
static const u8 IDLE_PTS[10][2] = {
    {9,20}, {10,20}, {9,21}, {10,21}, {16,20}, {17,20}, {16,21}, {17,21}, {9,11}, {11,9},
};
static const u8 GLITCH_PTS[12][2] = {
    {9,20}, {11,20}, {10,21}, {9,22}, {11,22},
    {15,20}, {17,20}, {16,21}, {15,22}, {17,22},
    {10,10}, {12,12},
};

static __attribute__((noinline)) void draw_pts(const KrxApiTable* api, const u8 (*pts)[2], u8 n) {
    for (u8 i = 0; i < n; i++) {
        krx_draw_pixel(api, pts[i][0], pts[i][1], 1u);
    }
}

static __attribute__((noinline)) void txt(
    const KrxApiTable* api, i16 x, i16 y, const char* s, u8 len, u8 col_al
) {
    krx_draw_str_align(api, x, y, (const u8*)s, len, FONT_TOMTHUMB, col_al & 1u, col_al >> 1);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf12_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;
    krx_clear(api);

    krx_fill_rect(api, 0, 0, 128, 8, COLOR_FILL);
    char ln[24];
    u8 p = p_app_s(ln, 0u, ":( ");
    p = p_app_s(ln, p, p_reason_name(s->reset_reason));
    p = p_app_s(ln, p, " #");
    p += p_put_uint(api, ln + p, s->crash_count);
    txt(api, 2, 1, ln, p, COLOR_CLEAR);

    if (st->boot_rem == 255u) {
        txt(api, 126, 1, "HOLD", 4u, COLOR_CLEAR | (ALIGN_RIGHT << 1));
    } else {
        p = p_put_uint(api, ln, st->boot_rem);
        ln[p++] = 's';
        txt(api, 126, 1, ln, p, COLOR_CLEAR | (ALIGN_RIGHT << 1));
    }

    krx_draw_bitmap(api, 1, 10, 24u, 27u, SAD_RADIO_BMP, sizeof(SAD_RADIO_BMP));
    if (st->boot_anim & 1u) {
        draw_pts(api, GLITCH_PTS, 12u);
    } else {
        draw_pts(api, IDLE_PTS, 10u);
    }
    draw_pts(api, FROWN_PTS, 9u);

    p = p_app_s(ln, 0u, "IQ:");
    p += p_put_uint(api, ln + p, s->forensics.ipsr_num);
    txt(api, 1, 40, ln, p, COLOR_FILL);
    p = p_app_s(ln, 0u, "S:");
    p += p_put_hex(ln + p, st->ana.crash_sig >> 16, 4u);
    txt(api, 1, 47, ln, p, COLOR_FILL);

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0;
}
