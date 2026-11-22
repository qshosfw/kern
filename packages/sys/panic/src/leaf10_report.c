/*
 * P0 Leaf 10: Unified Serial (USB/UART) & File (/log/panic.txt) Report Writer (<= 1 KB).
 */
#include "panic_shared.h"

static void rpt_raw(const KrxApiTable* api, PanicState* st, const char* s, u8 len) {
    if (st->report_dst == REPORT_SERIAL) {
        krx_serial_write(api, (const u8*)s, len);
    } else {
        krx_fs_append(api, (const u8*)"/log/panic.txt", 14u, (const u8*)s, len);
    }
}

static void rpt_line(const KrxApiTable* api, PanicState* st, const char* s) {
    rpt_raw(api, st, s, p_len(s));
    rpt_raw(api, st, "\r\n", 2u);
}

static void rpt_kv(const KrxApiTable* api, PanicState* st, const char* k, u32 v, u8 nl) {
    char b[16];
    b[0] = k[0]; b[1] = k[1]; b[2] = '=';
    u8 p = (u8)(3u + p_put_hex(b + 3, v, 8u));
    if (nl) { b[p++] = '\r'; b[p++] = '\n'; } else { b[p++] = ' '; }
    rpt_raw(api, st, b, p);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf10_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->slot;

    static const char HDR[] = "=== KERN PANIC ===\r\n";
    if (st->report_dst == REPORT_FILE) {
        krx_fs_mkdir(api, (const u8*)"/log", 4u);
        krx_fs_write(api, (const u8*)"/log/panic.txt", 14u, (const u8*)HDR, sizeof(HDR) - 1u);
    } else {
        rpt_raw(api, st, HDR, sizeof(HDR) - 1u);
    }

    if (st->ana.actor[0] != 0) rpt_line(api, st, st->ana.actor);
    rpt_line(api, st, st->ana.line1);
    rpt_line(api, st, st->ana.line2);
    if (st->ana.asm_txt[2][0] != 0) rpt_line(api, st, st->ana.asm_txt[2]);

    static const char KH[] = "PC\0LR\0SP\0xP\0SG";
    const u32 hv[5] = { s->fault_pc, s->fault_lr, s->fault_sp, s->xpsr, st->ana.crash_sig };
    for (u8 i = 0u; i < 5u; i++) rpt_kv(api, st, KH + i * 3u, hv[i], (i == 2u || i == 4u));

    static const char KR[] = "R0\0R1\0R2\0R3\0IP";
    for (u8 i = 0u; i < 5u; i++) rpt_kv(api, st, KR + i * 3u, s->stack_dump[i], (i == 3u || i == 4u));

    if (st->extra_applies) {
        static const char KEX[] = "R4\0R5\0R6\0R7\0R8\0R9\0RA\0RB\0MS\0PS";
        for (u8 i = 0u; i < 8u; i++) rpt_kv(api, st, KEX + i * 3u, st->snap.extra.callee[i], (i & 3u) == 3u);
        rpt_kv(api, st, "MS", st->snap.extra.msp, 0u);
        rpt_kv(api, st, "PS", st->snap.extra.psp, 1u);
    }

    for (u8 i = 0u; i < s->forensics.unwind_count && i < 4u; i++) {
        rpt_kv(api, st, "BT", s->stack_dump[8u + i], (i & 1u) == 1u);
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    return 0u;
}
