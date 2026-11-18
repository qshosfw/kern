/*
 * P0 Leaf 1: Signature CRC32, Safe Flash Opcode Peek [PC-4..PC+4], Non-HardFault Info (<= 1 KB).
 */
#include "panic_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;
    FaultAnalysis* a = &st->ana;

    for (u16 i = 0; i < (u16)sizeof(*a); i++) ((u8*)a)[i] = 0;
    a->fault_reg = 0xFFu;

    if (s->forensics.crash_signature != 0u) {
        a->crash_sig = s->forensics.crash_signature;
    } else if (api->math && api->math->crc32) {
        u32 sig_in[5];
        sig_in[0] = s->fault_pc;
        sig_in[1] = s->fault_lr;
        sig_in[2] = s->panic_file_hash;
        sig_in[3] = (u32)s->panic_line;
        sig_in[4] = s->reset_reason;
        a->crash_sig = api->math->crc32((const u8*)sig_in, 20u);
    }

    u32 pc_even = s->fault_pc & ~1u;
    if (st->cur_idx == 0u && p_is_safe_flash_hw(pc_even)) {
        for (u8 i = 0; i < 5u; i++) {
            u32 addr = (u32)((i32)pc_even + ((i32)i - 2) * 2);
            if (p_is_safe_flash_hw(addr)) {
                a->opcodes[i] = *(const volatile u16*)addr;
                a->op_valid[i] = 1u;
            }
        }
    }
    if (s->reset_reason == REASON_HARD) {
        if (!a->op_valid[1] && s->forensics.inst_pre != 0u) {
            a->opcodes[1] = s->forensics.inst_pre;
            a->op_valid[1] = 1u;
        }
        if (!a->op_valid[2] && s->panic_line != 0u) {
            a->opcodes[2] = s->panic_line;
            a->op_valid[2] = 1u;
        }
    } else {
        const char* l2k = "CSR:";
        u32 l2v = s->panic_file_hash;
        if (s->reset_reason == REASON_APPF) {
            u8 p = p_copy_cstr(a->line1, "Panic Line ", 16);
            p += p_put_uint(api, a->line1 + p, s->panic_line);
            a->line1[p] = 0;
            l2k = "Hash:";
        } else if (s->reset_reason == REASON_WDOG) {
            p_copy_cstr(a->line1, "IWDG/WWDG Watchdog", 27);
        } else if (s->reset_reason == REASON_BROW) {
            p_copy_cstr(a->line1, "Power Brownout Reset", 27);
        } else {
            p_copy_cstr(a->line1, "Strike / Tag Fault", 27);
            l2k = "BKP:";
            l2v = st->snap.backup_tag;
        }
        u8 q = p_copy_cstr(a->line2, l2k, 8);
        q += p_put_hex0x(a->line2 + q, l2v, 8);
        a->line2[q] = 0;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
