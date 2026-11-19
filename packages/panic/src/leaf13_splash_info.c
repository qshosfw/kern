/*
 * P0 Leaf 13: Crash Splash Right-Side Dense Forensics Zone (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf13_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    char ln[32];
    krx_draw_str_max(api, 29, 10, (const u8*)st->ana.line1, p_len(st->ana.line1), FONT_TOMTHUMB, COLOR_FILL, 98);
    u8 p = p_copy_cstr(ln, (const char*)s->actor_tag, 12u);
    ln[p++] = ' ';
    p += p_copy_cstr(ln + p, st->ana.line2, 14u);
    krx_draw_str_max(api, 29, 17, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL, 98);

    static const char LBLS[] = "PC\0LR\0SP\0PS\0R0\0OP\0BT\0R4";
    u32 vals[8];
    vals[0] = s->fault_pc;
    vals[1] = s->fault_lr;
    vals[2] = s->fault_sp;
    vals[3] = s->xpsr;
    vals[4] = s->stack_dump[0];
    vals[5] = st->ana.op_valid[2] ? st->ana.opcodes[2] : 0u;
    vals[6] = s->forensics.unwind_count ? s->stack_dump[8] : s->fault_lr;
    vals[7] = st->extra_applies ? st->snap.extra.callee[0] : s->stack_dump[12];

    for (u8 r = 0; r < 4u; r++) {
        i16 y = (i16)(24 + r * 7);
        for (u8 c = 0; c < 2u; c++) {
            u8 idx = (u8)(r * 2u + c);
            const char* lb = p_nth(LBLS, idx);
            ln[0] = lb[0]; ln[1] = lb[1]; ln[2] = ':';
            u8 q = 3u;
            if (idx == 5u) {
                if (st->ana.op_valid[2]) q += p_put_hex(ln + 3, vals[idx], 4u);
                else q = p_app_s(ln, 3u, "----");
            } else {
                q += p_put_hex(ln + 3, vals[idx], 8u);
            }
            krx_draw_str_ex(api, (i16)(29 + c * 50), y, (const u8*)ln, q, FONT_TOMTHUMB, COLOR_FILL);
        }
    }

    krx_draw_line_h(api, 0, 55, 128, COLOR_FILL);
    static const u8 BX[4] = { 1, 36, 64, 90 };
    static const char BT[] = "OK\0Boot\0v\0Dbg\0*\0QR\0S1\0Log";
    for (u8 i = 0; i < 4u; i++) {
        const char* k = p_nth(BT, (u8)(i * 2u));
        const char* v = p_nth(BT, (u8)(i * 2u + 1u));
        krx_ui_draw_key_badge(api, BX[i], 57, (const u8*)k, p_len(k), (const u8*)v, p_len(v));
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
