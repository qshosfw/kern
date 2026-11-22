/*
 * P0 Leaf 7: Page 4 — Stack & Backtrace with Memory Region Tags (<= 1 KB).
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
u32 leaf7_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->slot;

    krx_clear(api);
    i16 tab_r = p_draw_tab(api, "STACK");

    char hdr[16];
    u8 hp = 0u;
    if (s->fault_sp >= 0x20000000u && s->fault_sp <= 0x20004000u) {
        u32 used = 0x20004000u - s->fault_sp;
        hp = p_app_s(hdr, 0u, "used ");
        hp += p_put_uint(api, hdr + hp, used);
        hdr[hp++] = 'B';
    } else {
        hp = p_app_s(hdr, 0u, "SP:OOB!");
    }
    krx_draw_str_ex(api, (i16)(tab_r + 5), 7, (const u8*)hdr, hp, FONT_TOMTHUMB, COLOR_FILL);

    char pg[4] = { (char)('1' + PAGE_STACK), '/', (char)('0' + PAGE_COUNT), 0 };
    krx_draw_str_align(api, 125, 7, (const u8*)pg, 3u, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    u8 uw = s->forensics.unwind_count <= 3u ? s->forensics.unwind_count : 3u;
    char ln[28];
    for (u8 i = 0u; i < 5u; i++) {
        u32 val;
        u8 p = 0u;
        if (i == 0u) { ln[p++] = 'P'; ln[p++] = 'C'; ln[p++] = ' '; val = s->fault_pc; }
        else if (i == 1u) { ln[p++] = 'L'; ln[p++] = 'R'; ln[p++] = ' '; val = s->fault_lr; }
        else if (i < (u8)(2u + uw)) {
            ln[p++] = 'F'; ln[p++] = (char)('0' + (i - 2u)); ln[p++] = ' ';
            val = s->stack_dump[8u + (i - 2u)];
        } else {
            u8 si = (u8)(i - 2u - uw);
            ln[p++] = 'S'; ln[p++] = (char)('0' + si); ln[p++] = ' ';
            val = s->stack_dump[12u + (si & 3u)];
        }
        p += p_put_hex0x(ln + p, val, 8u);
        ln[p++] = ' ';
        p = p_app_s(ln, p, addr_tag(val));
        krx_draw_str_ex(api, 3, (i16)(17 + i * 8), (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
