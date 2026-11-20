/*
 * P0 Leaf 17: High-Brightness 2x2 QR Drawer & Metadata Legend (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf17_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    krx_wake_backlight(api);
    (void)krx_prop_set(api, KRX_PROP_INVERT_PIXELS, 0u);

    if (!st->has_slot) {
        krx_draw_str_ex(api, 8, 28, (const u8*)"No crash records", 16u, FONT_TOMTHUMB, COLOR_FILL);
        goto done;
    }

    i16 qx = st->qr_full ? 39 : 3;
    i16 qy = st->qr_full ? 7  : 12;
    for (u8 r = 0; r < 25u; r++) {
        u32 row = st->qr_rows[r];
        for (u8 c = 0; c < 25u; c++) {
            if ((row >> c) & 1u) {
                krx_fill_rect(api, (i16)(qx + c * 2), (i16)(qy + r * 2), 2, 2, COLOR_FILL);
            }
        }
    }

    if (st->qr_full) {
        krx_draw_str_ex(api, 2, 2, (const u8*)"QR", 2u, FONT_TOMTHUMB, COLOR_FILL);
        krx_draw_str_align(api, 126, 57, (const u8*)"[M]Back", 7u, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);
    } else {
        char ln[16];
        u8 q = p_app_s(ln, 0u, p_reason_name(s->reset_reason));
        q = p_app_s(ln, q, " #");
        q += p_put_uint(api, ln + q, s->crash_count);
        krx_draw_str_ex(api, 57, 14, (const u8*)ln, q, FONT_TOMTHUMB, COLOR_FILL);

        static const char KL[] = "PC\0LR\0SP\0SG";
        u32 kv[4] = { s->fault_pc, s->fault_lr, s->fault_sp, st->ana.crash_sig };
        for (u8 i = 0; i < 4u; i++) {
            const char* lb = p_nth(KL, i);
            ln[0] = lb[0]; ln[1] = lb[1]; ln[2] = ':';
            p_put_hex(ln + 3, kv[i], 8u);
            krx_draw_str_ex(api, 57, (i16)(22 + i * 8), (const u8*)ln, 11u, FONT_TOMTHUMB, COLOR_FILL);
        }
        krx_draw_str_ex(api, 57, 56, (const u8*)"[M]Zoom QR", 10u, FONT_TOMTHUMB, COLOR_FILL);
    }

done:
    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
