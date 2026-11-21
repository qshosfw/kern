/*
 * P0 Leaf 11: Crash Report Registers, Callee-Saved, Backtrace & Opcodes Writer (<= 1 KB).
 */
#include "panic_shared.h"

static void rpt_kv(const KrxApiTable* api, PanicState* st, const char* k, u32 v, u8 nl) {
    char b[16];
    b[0] = k[0]; b[1] = k[1]; b[2] = '=';
    u8 p = (u8)(3u + p_put_hex(b + 3, v, 8u));
    if (nl) { b[p++] = '\r'; b[p++] = '\n'; } else { b[p++] = ' '; }
    if (st->leaf_arg == REPORT_UART) {
        krx_serial_write(api, (const u8*)b, p);
    } else {
        krx_fs_append(api, (const u8*)"/log/crash.txt", 14u, (const u8*)b, p);
    }
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf11_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    static const char KR[] = "R0\0R1\0R2\0R3\0IP";
    for (u8 i = 0; i < 5u; i++) {
        rpt_kv(api, st, KR + i * 3u, s->stack_dump[i], (i == 3u || i == 4u));
    }

    if (st->extra_applies) {
        static const char KEX[] = "R4\0R5\0R6\0R7\0R8\0R9\0RA\0RB\0MS\0PS\0CR\0IC";
        u32 ex[12];
        for (u8 i = 0; i < 8u; i++) ex[i] = st->snap.extra.callee[i];
        ex[8] = st->snap.extra.msp;
        ex[9] = st->snap.extra.psp;
        ex[10] = st->snap.extra.rcc_csr;
        ex[11] = st->snap.extra.icsr;
        for (u8 i = 0; i < 12u; i++) {
            rpt_kv(api, st, KEX + i * 3u, ex[i], (i & 3u) == 3u);
        }
    }

    for (u8 i = 0; i < s->forensics.unwind_count && i < 4u; i++) {
        rpt_kv(api, st, "BT", s->stack_dump[8u + i], (i & 1u) == 1u);
    }
    for (u8 i = 0; i < 5u; i++) {
        if (st->ana.op_valid[i]) {
            rpt_kv(api, st, (i == 2u) ? "*O" : "OP", st->ana.opcodes[i], 1u);
        }
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
