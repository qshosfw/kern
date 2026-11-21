/*
 * P0 Leaf 5: Stack Unwind & Backtrace Tab Renderer (<= 1 KB).
 */
#include "panic_shared.h"

static const char* addr_tag(u32 v) {
    if (v == 0u) return "NULL";
    if (v >= 0x08000000u && v < 0x08020000u) return "FLSH";
    if (v >= 0x20000000u && v < 0x20004000u) return "SRAM";
    if (v >= 0x40000000u && v < 0x60000000u) return "MMIO";
    if (v >= 0xE0000000u) return "SCB";
    return "OOB!";
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf5_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    if (!st->has_slot) {
        krx_draw_str_ex(api, 8, 28, (const u8*)"No crash records", 16u, FONT_TOMTHUMB, COLOR_FILL);
        goto done;
    }

    u8 uw = s->forensics.unwind_count <= 4u ? s->forensics.unwind_count : 4u;
    u8 total = (u8)(6u + uw);
    u8 sc = st->scroll;
    if (sc + 5u > total) sc = (total > 5u) ? (u8)(total - 5u) : 0u;

    char ln[28];
    for (u8 v = 0; v < 5u && (u8)(sc + v) < total; v++) {
        u8 idx = (u8)(sc + v);
        u32 val;
        u8 p = 0;
        if (idx == 0u) { ln[p++] = 'P'; ln[p++] = 'C'; ln[p++] = ' '; val = s->fault_pc; }
        else if (idx == 1u) { ln[p++] = 'L'; ln[p++] = 'R'; ln[p++] = ' '; val = s->fault_lr; }
        else if (idx < (u8)(2u + uw)) {
            ln[p++] = 'F'; ln[p++] = (char)('0' + (idx - 2u)); ln[p++] = ' ';
            val = s->stack_dump[8u + (idx - 2u)];
        } else {
            u8 si = (u8)(idx - 2u - uw);
            ln[p++] = 'S'; ln[p++] = (char)('0' + si); ln[p++] = ' ';
            val = s->stack_dump[12u + (si & 3u)];
        }
        p += p_put_hex0x(ln + p, val, 8u);
        ln[p++] = ' ';
        p = p_app_s(ln, p, addr_tag(val));
        ln[p] = 0;
        krx_draw_str_ex(api, 4, (i16)(14 + v * 9), (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);
    }

    if (total > 5u) {
        krx_ui_draw_scrollbar(api, 123u, 12u, 48u, sc, total, 5u);
    }

done:
    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
