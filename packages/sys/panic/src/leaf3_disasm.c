/*
 * P0 Leaf 3: Compact Thumb-16 Disassembler for [PC-4..PC+4] Window (<= 1 KB).
 */
#include "panic_shared.h"

static __attribute__((noinline)) u8 d_reg(char* o, u8 p, u8 r) {
    return (u8)(p + p_put_reg(o + p, r));
}

static __attribute__((noinline)) u8 d_str(char* o, u8 p, const char* s) {
    return p_app_s(o, p, s);
}

static __attribute__((noinline)) u8 d_hex(char* o, u8 p, u32 v, u8 dig) {
    return (u8)(p + p_put_hex(o + p, v, dig));
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf3_entry(u32 op_code, const KrxApiTable* api, void* param) {
    (void)op_code; (void)api;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;

    for (u8 i = 0u; i < 5u; i++) {
        char* o = st->ana.asm_txt[i];
        o[0] = 0;
        if (!st->ana.op_valid[i]) continue;
        u16 op = st->ana.opcodes[i];
        u16 hi = (u16)(op >> 12);
        u8 p = 0u;

        if (hi >= 6u && hi <= 9u) {
            p = d_str(o, p, ((op >> 11) & 1u) ? "ldr" : "str");
            if (hi == 7u) o[p++] = 'b'; else if (hi == 8u) o[p++] = 'h';
            o[p++] = ' ';
            if (hi == 9u) {
                p = d_reg(o, p, (u8)((op >> 8) & 7u));
                p = d_str(o, p, ",[sp,#");
                p = d_hex(o, p, (op & 0xFFu) << 2, 3u);
            } else {
                p = d_reg(o, p, (u8)(op & 7u));
                p = d_str(o, p, ",[");
                p = d_reg(o, p, (u8)((op >> 3) & 7u));
                p = d_str(o, p, ",#");
                u8 sh = (hi == 6u) ? 2u : ((hi == 8u) ? 1u : 0u);
                p = d_hex(o, p, ((op >> 6) & 0x1Fu) << sh, 2u);
            }
            o[p++] = ']';
        } else if (hi == 5u) {
            static const char LS[] = "str \0strh \0strb \0ldrsb \0ldr \0ldrh \0ldrb \0ldrsh ";
            p = d_str(o, p, p_nth(LS, (u8)((op >> 9) & 7u)));
            p = d_reg(o, p, (u8)(op & 7u)); p = d_str(o, p, ",[");
            p = d_reg(o, p, (u8)((op >> 3) & 7u)); o[p++] = ',';
            p = d_reg(o, p, (u8)((op >> 6) & 7u)); o[p++] = ']';
        } else if ((op & 0xF800u) == 0x4800u) {
            p = d_str(o, p, "ldr ");
            p = d_reg(o, p, (u8)((op >> 8) & 7u));
            p = d_str(o, p, ",[pc,#"); p = d_hex(o, p, (op & 0xFFu) << 2, 3u); o[p++] = ']';
        } else if ((op & 0xFF00u) == 0x4700u) {
            p = d_str(o, p, (op & 0x80u) ? "blx " : "bx ");
            p = d_reg(o, p, (u8)((op >> 3) & 0xFu));
        } else if ((op & 0xF600u) == 0xB400u) {
            u8 is_p = (u8)((op >> 11) & 1u);
            p = d_str(o, p, is_p ? "pop " : "push ");
            p = d_str(o, p, (op & 0x100u) ? (is_p ? "{..pc}" : "{..lr}") : "{list}");
        } else if ((op & 0xFF00u) == 0xDE00u) {
            p = d_str(o, p, "udf #"); p = d_hex(o, p, op & 0xFFu, 2u);
        } else {
            p = d_str(o, p, "op 0x"); p = d_hex(o, p, op, 4u);
        }
        o[p] = 0;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
