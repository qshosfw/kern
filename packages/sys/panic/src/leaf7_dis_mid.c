/*
 * P0 Leaf 7: Thumb-16 Disassembler Pass B (0x4000..0x5FFF: ALU16, HiReg/BX, PC LDR, Reg L/S) (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf7_entry(u32 op_code, const KrxApiTable* api, void* param) {
    (void)op_code;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;

    static const char ALU16[] =
        "ands\0eors\0lsls\0lsrs\0asrs\0adcs\0sbcs\0rors\0"
        "tsts\0negs\0cmps\0cmns\0orrs\0muls\0bics\0mvns";
    static const char LSREG[] =
        "str\0strh\0strb\0ldrsb\0ldr\0ldrh\0ldrb\0ldrsh";

    for (u8 i = 0; i < 5u; i++) {
        char* o = st->ana.asm_txt[i];
        if (!st->ana.op_valid[i] || st->cur_slot.reset_reason != REASON_HARD || o[0] != 0) continue;

        u16 op = st->ana.opcodes[i];
        u8 p = 0;
        if ((op & 0xFC00u) == 0x4000u) {
            p = p_app_s(o, p, p_nth(ALU16, (u8)((op >> 6) & 0xFu)));
            o[p++] = ' ';
            p += p_put_reg(o + p, (u8)(op & 7u));
            o[p++] = ',';
            p += p_put_reg(o + p, (u8)((op >> 3) & 7u));
        } else if ((op & 0xFC00u) == 0x4400u) {
            u8 sop = (u8)((op >> 8) & 3u);
            if (sop == 3u) {
                p = p_app_s(o, p, (op & 0x80u) ? "blx " : "bx ");
                p += p_put_reg(o + p, (u8)((op >> 3) & 0xFu));
            } else {
                p = p_app_s(o, p, (sop == 0u) ? "add " : (sop == 1u ? "cmp " : "mov "));
                p += p_put_reg(o + p, (u8)((op & 7u) | ((op >> 4) & 8u)));
                o[p++] = ',';
                p += p_put_reg(o + p, (u8)((op >> 3) & 0xFu));
            }
        } else if ((op & 0xF800u) == 0x4800u) {
            p = p_app_s(o, p, "ldr ");
            p += p_put_reg(o + p, (u8)((op >> 8) & 7u));
            p = p_app_s(o, p, ",[pc,#");
            p += p_put_uint(api, o + p, (op & 0xFFu) << 2);
            o[p++] = ']';
        } else if ((op & 0xF000u) == 0x5000u) {
            p = p_app_s(o, p, p_nth(LSREG, (u8)((op >> 9) & 7u)));
            o[p++] = ' ';
            p += p_put_reg(o + p, (u8)(op & 7u));
            p = p_app_s(o, p, ",[");
            p += p_put_reg(o + p, (u8)((op >> 3) & 7u));
            o[p++] = ',';
            p += p_put_reg(o + p, (u8)((op >> 6) & 7u));
            o[p++] = ']';
        }
        if (p > 19u) p = 19u;
        o[p] = 0;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
