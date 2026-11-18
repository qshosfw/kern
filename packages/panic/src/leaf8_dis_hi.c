/*
 * P0 Leaf 8: Thumb-16 Disassembler Pass C (op >= 0x6000: Imm L/S, PUSH/POP, Branches, UDF) (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf8_entry(u32 op_code, const KrxApiTable* api, void* param) {
    (void)op_code;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;

    static const char BCOND[] = "eq\0ne\0cs\0cc\0mi\0pl\0vs\0vc\0hi\0ls\0ge\0lt\0gt\0le\0al";

    for (u8 i = 0; i < 5u; i++) {
        char* o = st->ana.asm_txt[i];
        if (!st->ana.op_valid[i] || st->cur_slot.reset_reason != REASON_HARD || o[0] != 0) continue;

        u16 op = st->ana.opcodes[i];
        u16 hi = (u16)(op >> 12);
        u8 p = 0;
        if (hi >= 6u && hi <= 9u) {
            u8 is_l = (u8)((op >> 11) & 1u);
            p = p_app_s(o, p, is_l ? "ldr" : "str");
            if (hi == 7u) o[p++] = 'b';
            else if (hi == 8u) o[p++] = 'h';
            o[p++] = ' ';
            if (hi == 9u) {
                p += p_put_reg(o + p, (u8)((op >> 8) & 7u));
                p = p_app_s(o, p, ",[sp,#");
                p += p_put_uint(api, o + p, (op & 0xFFu) << 2);
            } else {
                p += p_put_reg(o + p, (u8)(op & 7u));
                p = p_app_s(o, p, ",[");
                p += p_put_reg(o + p, (u8)((op >> 3) & 7u));
                p = p_app_s(o, p, ",#");
                u8 sh = (hi == 6u) ? 2u : ((hi == 8u) ? 1u : 0u);
                p += p_put_uint(api, o + p, ((op >> 6) & 0x1Fu) << sh);
            }
            o[p++] = ']';
        } else if ((op & 0xF600u) == 0xB400u) {
            u8 is_p = (u8)((op >> 11) & 1u);
            p = p_app_s(o, p, is_p ? "pop " : "push ");
            p = p_app_s(o, p, (op & 0x100u) ? (is_p ? "{..pc}" : "{..lr}") : "{list}");
        } else if ((op & 0xFF00u) == 0xDE00u) {
            p = p_app_s(o, p, "udf #0x");
            p += p_put_hex(o + p, op & 0xFFu, 2u);
        } else if (hi == 0xDu) {
            u8 c = (u8)((op >> 8) & 0xFu);
            if (c == 0xFu) { p = p_app_s(o, p, "svc #"); p += p_put_uint(api, o + p, op & 0xFFu); }
            else { o[p++] = 'b'; p = p_app_s(o, p, p_nth(BCOND, c < 14u ? c : 14u)); }
        } else if ((op & 0xF800u) == 0xE000u) {
            p = p_app_s(o, p, "b #"); p += p_put_hex(o + p, op & 0x7FFu, 3u);
        } else if (hi == 0xFu) {
            p = p_app_s(o, p, (op & 0x0800u) ? "bl2 " : "bl1 ");
            p += p_put_hex(o + p, op & 0x7FFu, 3u);
        } else {
            p = p_app_s(o, p, "dw 0x");
            p += p_put_hex(o + p, op, 4u);
        }
        if (p > 19u) p = 19u;
        o[p] = 0;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
