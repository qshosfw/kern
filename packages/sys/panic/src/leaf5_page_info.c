/*
 * P0 Leaf 5: Page 1 Dense Forensics (Cause, Disassembly, 6 Key Regs & Micro Tag) (<= 1 KB).
 *
 * - y=16: Cause + addr (e.g. "Null read @0x00000000")
 * - y=24: Disassembled faulting PC ("> ldr r0,[r1,#00]")
 * - y=33: PC & LR
 * - y=41: SP & R0
 * - y=49: XP & OP
 * - y=47 (left, under radio): IRQ N or S:XXXX
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf5_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->slot;

    char ln[32];
    u8 p = p_copy_cstr(ln, st->ana.line1, 14u);
    if (st->ana.line2[0] != 0) {
        ln[p++] = ' ';
        p += p_copy_cstr(ln + p, st->ana.line2, 16u);
    }
    krx_draw_str_max(api, 29, 16, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL, 96);

    if (st->ana.asm_txt[2][0] != 0) {
        p = p_app_s(ln, 0u, "> ");
        p += p_copy_cstr(ln + p, st->ana.asm_txt[2], 19u);
    } else {
        p = p_app_s(ln, 0u, "Sig 0x");
        p += p_put_hex(ln + p, st->ana.crash_sig, 8u);
    }
    krx_draw_str_max(api, 29, 24, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL, 96);

    static const char LBLS[] = "PC\0LR\0SP\0R0\0XP\0OP";
    u32 vals[6] = {
        s->fault_pc, s->fault_lr, s->fault_sp, s->stack_dump[0],
        s->xpsr, st->ana.op_valid[2] ? st->ana.opcodes[2] : 0u
    };
    for (u8 r = 0u; r < 3u; r++) {
        i16 y = (i16)(33 + r * 8);
        for (u8 c = 0u; c < 2u; c++) {
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
            krx_draw_str_ex(api, (i16)(29 + c * 49), y, (const u8*)ln, q, FONT_TOMTHUMB, COLOR_FILL);
        }
    }

    if (s->forensics.ipsr_num > 0u) {
        p = p_app_s(ln, 0u, "IQ:");
        p += p_put_uint(api, ln + p, s->forensics.ipsr_num);
    } else {
        p = p_app_s(ln, 0u, "THR");
    }
    ln[p++] = ' ';
    ln[p++] = (s->forensics.exec_mode & 1u) ? 'P' : 'M';
    krx_draw_str_ex(api, 2, 44, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);

    p = p_app_s(ln, 0u, "S:");
    p += p_put_hex(ln + p, st->ana.crash_sig >> 16, 4u);
    krx_draw_str_ex(api, 2, 51, (const u8*)ln, p, FONT_TOMTHUMB, COLOR_FILL);

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
