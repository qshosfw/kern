/*
 * P0 Leaf 8: Page 5 — Disassembly [PC-4..PC+4] with Rounded Fault Highlight (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf8_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->slot;

    krx_clear(api);
    i16 tab_r = p_draw_sq_tab(api, "ASM");

    char pc_hdr[14];
    u8 pp = p_app_s(pc_hdr, 0u, "@0x");
    pp += p_put_hex(pc_hdr + pp, s->fault_pc, 8u);
    krx_draw_str_ex(api, (i16)(tab_r + 5), 7, (const u8*)pc_hdr, pp, FONT_TOMTHUMB, COLOR_FILL);

    char pg[4] = { (char)('1' + PAGE_ASM), '/', (char)('0' + PAGE_COUNT), 0 };
    krx_draw_str_align(api, 125, 7, (const u8*)pg, 3u, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    if (s->reset_reason != REASON_HARD) {
        krx_draw_str_ex(api, 3, 19, (const u8*)st->ana.line1, p_len(st->ana.line1), FONT_TOMTHUMB, COLOR_FILL);
        krx_draw_str_ex(api, 3, 28, (const u8*)st->ana.line2, p_len(st->ana.line2), FONT_TOMTHUMB, COLOR_FILL);
        krx_draw_str_ex(api, 3, 40, (const u8*)"[S2] Save /log/panic.txt", 24u, FONT_TOMTHUMB, COLOR_FILL);
        krx_draw_str_ex(api, 3, 48, (const u8*)"[0]  Reboot", 11u, FONT_TOMTHUMB, COLOR_FILL);
    } else {
        u32 pc_even = s->fault_pc & ~1u;
        char ln[32];
        for (u8 i = 0u; i < 5u; i++) {
            i16 y = (i16)(17 + i * 8);
            u8 is_pc = (i == 2u) ? 1u : 0u;
            if (is_pc) krx_fill_rect(api, 2, (i16)(y - 6), 124, 8, COLOR_FILL);
            u32 addr = (u32)((i32)pc_even + ((i32)i - 2) * 2);
            u8 lp = 0u;
            ln[lp++] = is_pc ? '>' : ' ';
            lp += p_put_hex(ln + lp, addr & 0xFFFFu, 4u);
            ln[lp++] = ':';
            ln[lp++] = ' ';
            lp = p_app_s(ln, lp, st->ana.op_valid[i] ? st->ana.asm_txt[i] : "????");
            krx_draw_str_max(api, 4, y, (const u8*)ln, lp, FONT_TOMTHUMB, is_pc ? COLOR_CLEAR : COLOR_FILL, 120);
        }
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
