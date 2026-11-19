/*
 * P0 Leaf 9: Asm Tab Renderer ([PC-4..PC+4] with inverted fault row) (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf9_entry(u32 op_code, const KrxApiTable* api, void* param) {
    (void)op_code;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;

    if (st->cur_slot.reset_reason != REASON_HARD) {
        krx_draw_str_ex(api, 6, 28, (const u8*)"N/A (Software Panic)", 20u, FONT_TOMTHUMB, COLOR_FILL);
    } else {
        u32 pc_even = st->cur_slot.fault_pc & ~1u;
        char line[32];
        for (u8 i = 0; i < 5u; i++) {
            i16 y = (i16)(14 + i * 9);
            u8 is_pc = (i == 2u) ? 1u : 0u;
            if (is_pc) krx_fill_rect(api, 3, y - 1, 118, 8, COLOR_FILL);
            u32 addr = (u32)((i32)pc_even + ((i32)i - 2) * 2);
            u8 lp = 0;
            line[lp++] = is_pc ? '>' : ' ';
            lp += p_put_hex(line + lp, addr & 0xFFFFu, 4u);
            line[lp++] = ':';
            lp = p_app_s(line, lp, st->ana.op_valid[i] ? st->ana.asm_txt[i] : "????");
            line[lp] = 0;
            krx_draw_str_ex(api, 4, y, (const u8*)line, lp, FONT_TOMTHUMB, is_pc ? COLOR_CLEAR : COLOR_FILL);
        }
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
