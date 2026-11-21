/*
 * P0 Leaf 2: HardFault Cause Decoder & Effective Address Elaborator (<= 1 KB).
 */
#include "panic_shared.h"

static u32 reg_val(const PanicState* st, u8 reg) {
    if (reg < 4u) return st->cur_slot.stack_dump[reg];
    if (reg < 12u) return st->extra_applies ? st->snap.extra.callee[reg - 4u] : 0u;
    if (reg == 12u) return st->cur_slot.stack_dump[4];
    if (reg == 13u) return st->cur_slot.fault_sp;
    if (reg == 14u) return st->cur_slot.fault_lr;
    return st->cur_slot.fault_pc;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op; (void)api;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;
    FaultAnalysis* a = &st->ana;

    u16 inst = a->opcodes[2];
    const char* l1 = "CPU HardFault";
    const char* l2k = "PC:";
    u32 l2v = s->fault_pc;

    if (s->fault_sp < 0x20000000u || s->fault_sp > 0x20003FE0u) {
        l1 = "Stack Ptr Corrupt";
        l2k = "SP:";
        l2v = s->fault_sp;
    } else if (a->op_valid[2] && (inst & 0xFF00u) == 0xDE00u) {
        u8 q = p_copy_cstr(a->line1, "Kernel Trap #", 16);
        q += p_put_hex(a->line1 + q, inst & 0xFFu, 2);
        a->line1[q] = 0;
        p_copy_cstr(a->line2, (const char*)s->actor_tag, 16);
        goto done;
    } else if (a->op_valid[2] && (inst & 0xFF00u) == 0x4700u) {
        u8 rm = (u8)((inst >> 3) & 0xFu);
        l2v = reg_val(st, rm);
        a->fault_reg = rm;
        a->fault_addr = l2v;
        l1 = (inst & 0x0080u) ? "Bad BLX Rm" : "Bad BX Rm";
        l2k = "Tgt:";
    } else if (a->op_valid[2] && ((inst & 0xE000u) == 0x6000u || (inst & 0xF000u) == 0x8000u)) {
        u8 rn = (u8)((inst >> 3) & 7u);
        u8 imm5 = (u8)((inst >> 6) & 0x1Fu);
        u8 is_ld = (u8)((inst >> 11) & 1u);
        u8 sz = ((inst & 0xF000u) == 0x7000u) ? 0u : (((inst & 0xF000u) == 0x8000u) ? 1u : 2u);
        u32 addr = reg_val(st, rn) + ((u32)imm5 << sz);
        a->fault_reg = rn;
        a->fault_addr = addr;
        u8 unalign = (sz == 2u && (addr & 3u) != 0u) || (sz == 1u && (addr & 1u) != 0u);
        l1 = (addr < 0x1000u) ? (is_ld ? "Null Ptr Read" : "Null Ptr Write")
           : (unalign ? "Unaligned Access"
           : (is_ld ? "Bus Read Fault" : "Bus Write Fault"));
        l2k = "EA:";
        l2v = addr;
    } else if (s->fault_pc == 0u || (s->fault_pc & 1u) == 0u) {
        l1 = "Zero/ARM PC Fault";
        l2k = "LR:";
        l2v = s->fault_lr;
    }

    p_copy_cstr(a->line1, l1, 27);
    u8 q = p_copy_cstr(a->line2, l2k, 6);
    q += p_put_hex0x(a->line2 + q, l2v, 8);
    a->line2[q] = 0;

done:
    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
