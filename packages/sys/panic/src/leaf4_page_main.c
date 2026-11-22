/*
 * P0 Leaf 4: Page 1 Square Inverted Fault Tab, Deduplicated Actor, Countdown & Sad Radio (<= 1 KB).
 */
#include "panic_shared.h"

static const u8 SAD_RADIO_BMP[81] = {
    0x02,0x00,0x00, 0x07,0x00,0x00, 0x07,0x00,0x00, 0x07,0x00,0x00,
    0x07,0x00,0x00, 0x1F,0xFF,0xF0, 0x3F,0xFF,0xF8, 0x3F,0xFF,0xF8,
    0x78,0x00,0x38, 0x78,0x00,0x38, 0x78,0x00,0x38, 0x78,0x00,0x38,
    0x78,0x00,0x38, 0x78,0x00,0x38, 0x38,0x00,0x38, 0x38,0x00,0x38,
    0x38,0x00,0x38, 0x38,0x00,0x38, 0x38,0x00,0x38, 0x3F,0xFF,0xF8,
    0x3F,0xFF,0xF8, 0x3C,0x00,0x78, 0x3F,0xFF,0xF8, 0x3C,0x00,0x78,
    0x3F,0xFF,0xF8, 0x3F,0xFF,0xF8, 0x1F,0xFF,0xF0,
};

static const u8 FACE_PTS[17][2] = {
    {9,22}, {10,22}, {9,23}, {10,23},
    {16,22}, {17,22}, {16,23}, {17,23},
    {11,26}, {12,26}, {13,26}, {14,26}, {15,26},
    {10,27}, {16,27}, {9,28}, {17,28},
};

static u8 eq_ci(const char* a, const char* b) {
    while (*a && *b) {
        u8 ca = (u8)*a++, cb = (u8)*b++;
        if (ca >= 'a' && ca <= 'z') ca = (u8)(ca - 32u);
        if (cb >= 'a' && cb <= 'z') cb = (u8)(cb - 32u);
        if (ca != cb) return 0u;
    }
    return (*a == 0 && *b == 0) ? 1u : 0u;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf4_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->slot;

    krx_clear(api);

    const char* title = p_reason_name(s->reset_reason);
    i16 tab_r = p_draw_sq_tab(api, title);

    if (st->ana.actor[0] != 0 && !eq_ci(st->ana.actor, title)) {
        i16 ax = (i16)(tab_r + 4);
        if (ax < 100) {
            krx_draw_str_max(api, ax, 7, (const u8*)st->ana.actor, p_len(st->ana.actor), FONT_TOMTHUMB, COLOR_FILL, (i16)(104 - ax));
        }
    }

    char rt[8];
    u8 p = 0u;
    if (st->mode == MODE_BOOT_DIALOG && st->boot_rem != PANIC_HOLD_SENTINEL) {
        p = p_put_uint(api, rt, st->boot_rem);
        rt[p++] = 's';
    } else {
        rt[p++] = '1'; rt[p++] = '/'; rt[p++] = (char)('0' + PAGE_COUNT);
    }
    krx_draw_str_align(api, 125, 7, (const u8*)rt, p, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    krx_draw_bitmap(api, 2, 12, 24u, 27u, SAD_RADIO_BMP, sizeof(SAD_RADIO_BMP));
    for (u8 i = 0u; i < 17u; i++) {
        krx_draw_pixel(api, FACE_PTS[i][0], FACE_PTS[i][1], COLOR_FILL);
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
