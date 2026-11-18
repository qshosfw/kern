/*
 * P0 Leaf 12: Crash Splash Top Bar, Animated Sad-Radio Art & Bottom Action Bar (<= 1 KB).
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

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf12_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg || !api->display) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const CapDisplayVTable* d = api->display;
    const BootSlot* s = &st->cur_slot;
    d->clear();

    // Full-bleed edge-to-edge inverted header bar (y=0..8)
    d->fill_rect(0, 0, 128, 8, COLOR_FILL);
    char ln[24];
    u8 p = p_app_s(ln, 0u, ":( ");
    p = p_app_s(ln, p, p_reason_name(s->reset_reason));
    p = p_app_s(ln, p, " #");
    p += p_put_uint(api, ln + p, s->crash_count);
    d->draw_str(2, 1, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_CLEAR);

    if (st->boot_rem == 255u) {
        d->draw_str_align(126, 1, (const u8*)"HOLD", 4u, FONT_TOMTHUMB, COLOR_CLEAR, ALIGN_RIGHT);
    } else {
        p = p_put_uint(api, ln, st->boot_rem);
        ln[p++] = 's';
        d->draw_str_align(126, 1, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_CLEAR, ALIGN_RIGHT);
    }

    // Left: DFU-style radio icon @ (1, 10) with animated :( / X( inside LCD + antenna spark
    d->draw_bitmap(1, 10, 24, 27, SAD_RADIO_BMP, sizeof(SAD_RADIO_BMP));
    if (st->boot_anim & 1u) {
        // X( glitch eyes + spark
        for (i8 k = 0; k < 3; k++) {
            for (u8 e = 0; e < 2u; e++) {
                i16 ex = (i16)((e ? 15 : 9) + k);
                d->draw_pixel(ex, (i16)(20 + k), 1);
                d->draw_pixel((i16)((e ? 17 : 11) - k), (i16)(20 + k), 1);
            }
        }
        d->draw_pixel(10, 10, 1); d->draw_pixel(12, 12, 1);
    } else {
        d->fill_rect(9, 20, 2, 2, 1);
        d->fill_rect(16, 20, 2, 2, 1);
        d->draw_pixel(9, 11, 1); d->draw_pixel(11, 9, 1);
    }
    // Sad frown mouth inside radio LCD
    d->fill_rect(11, 24, 5, 1, 1);
    d->draw_pixel(10, 25, 1); d->draw_pixel(16, 25, 1);
    d->draw_pixel(9, 26, 1); d->draw_pixel(17, 26, 1);

    // Micro telemetry under radio icon (x=1..27)
    p = p_app_s(ln, 0u, "IQ:");
    p += p_put_uint(api, ln + p, s->forensics.ipsr_num);
    d->draw_str(1, 40, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);
    p = p_app_s(ln, 0u, "S:");
    p += p_put_hex(ln + p, st->ana.crash_sig >> 16, 4u);
    d->draw_str(1, 47, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
