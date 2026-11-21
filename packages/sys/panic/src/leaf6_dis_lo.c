/*
 * P0 Leaf 6: Thumb-16 Disassembler Pass A (op < 0x4000: Shifts, Add/Sub, Mov/Cmp Imm) (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf6_entry(u32 op_code, const KrxApiTable* api, void* param) {
    (void)op_code;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;

    for (u8 i = 0; i < 5u; i++) {
        char* o = st->ana.asm_txt[i];
        o[0] = 0;
        if (!st->ana.op_valid[i] || st->cur_slot.reset_reason != REASON_HARD) continue;

        u16 op = st->ana.opcodes[i];
        u8 p = 0;
        if ((op & 0xF800u) == 0x1800u) {
            p = p_app_s(o, p, (op & 0x0200u) ? "subs " : "adds ");
            p += p_put_reg(o + p, (u8)(op & 7u));
            o[p++] = ',';
            p += p_put_reg(o + p, (u8)((op >> 3) & 7u));
            o[p++] = ',';
            if (op & 0x0400u) { o[p++] = '#'; p += p_put_uint(api, o + p, (op >> 6) & 7u); }
            else p += p_put_reg(o + p, (u8)((op >> 6) & 7u));
        } else if (op < 0x2000u) {
            u8 mop = (u8)((op >> 11) & 3u);
            p = p_app_s(o, p, (mop == 0u) ? "lsls " : (mop == 1u ? "lsrs " : "asrs "));
            p += p_put_reg(o + p, (u8)(op & 7u));
            o[p++] = ',';
            p += p_put_reg(o + p, (u8)((op >> 3) & 7u));
            p = p_app_s(o, p, ",#");
            p += p_put_uint(api, o + p, (op >> 6) & 0x1Fu);
        } else if (op < 0x4000u) {
            u8 mop = (u8)((op >> 11) & 3u);
            p = p_app_s(o, p, (mop == 0u) ? "movs " : (mop == 1u ? "cmp " : (mop == 2u ? "adds " : "subs ")));
            p += p_put_reg(o + p, (u8)((op >> 8) & 7u));
            p = p_app_s(o, p, ",#");
            p += p_put_uint(api, o + p, op & 0xFFu);
        }
        if (p > 19u) p = 19u;
        o[p] = 0;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
