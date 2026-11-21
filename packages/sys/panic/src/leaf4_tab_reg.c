/*
 * P0 Leaf 4: CPU Registers Tab Renderer (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf4_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    if (!st->has_slot) {
        krx_draw_str_ex(api, 8, 28, (const u8*)"No crash records", 16u, FONT_TOMTHUMB, COLOR_FILL);
        goto done;
    }

    static const char RNAMES[] =
        "R0\0R1\0R2\0R3\0IP\0LR\0PC\0XP\0"
        "R4\0R5\0R6\0R7\0R8\0R9\0RA\0RB\0"
        "MS\0PS\0CR\0IC";

    u8 max_rows = st->extra_applies ? 10u : 4u;
    u8 sc = st->scroll;
    if (sc + 5u > max_rows) sc = (max_rows > 5u) ? (u8)(max_rows - 5u) : 0u;

    char ln[32];
    for (u8 v = 0; v < 5u && (u8)(sc + v) < max_rows; v++) {
        u8 row = (u8)(sc + v);
        u8 idx = (u8)(row * 2u);
        u32 vals[2] = {0, 0};
        if (row < 4u) {
            vals[0] = s->stack_dump[idx];
            vals[1] = s->stack_dump[idx + 1u];
        } else if (row < 8u) {
            vals[0] = st->snap.extra.callee[idx - 8u];
            vals[1] = st->snap.extra.callee[idx - 7u];
        } else if (row == 8u) {
            vals[0] = st->snap.extra.msp;
            vals[1] = st->snap.extra.psp;
        } else {
            vals[0] = st->snap.extra.rcc_csr;
            vals[1] = st->snap.extra.icsr;
        }
        u8 p = 0;
        for (u8 c = 0; c < 2u; c++) {
            if (c == 1u) ln[p++] = ' ';
            const char* rn = p_nth(RNAMES, (u8)(idx + c));
            ln[p++] = rn[0]; ln[p++] = rn[1]; ln[p++] = ':';
            p += p_put_hex(ln + p, vals[c], 8u);
        }
        ln[p] = 0;
        krx_draw_str_ex(api, 4, (i16)(14 + v * 9), (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);
    }

    if (max_rows > 5u) {
        krx_ui_draw_scrollbar(api, 123u, 12u, 48u, sc, max_rows, 5u);
    }

done:
    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
