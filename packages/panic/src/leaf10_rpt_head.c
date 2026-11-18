/*
 * P0 Leaf 10: Crash Report Header & Core Fault Info Writer (<= 1 KB).
 */
#include "panic_shared.h"

static void rpt_raw(const KrxApiTable* api, PanicState* st, const char* s, u8 len) {
    if (st->leaf_arg == REPORT_UART) {
        if (api->serial && api->serial->serial_write) api->serial->serial_write((const u8*)s, len);
    } else if (api->storage && api->storage->storage_append_file) {
        api->storage->storage_append_file((const u8*)"/log/crash.txt", 14, (const u8*)s, len);
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
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    static const char HDR[] = "=== KERN CRASH REPORT ===\r\n";
    if (st->leaf_arg == REPORT_FILE && api->storage) {
        if (api->storage->storage_create_dir) api->storage->storage_create_dir((const u8*)"/log", 4);
        if (api->storage->storage_write_file) {
            api->storage->storage_write_file((const u8*)"/log/crash.txt", 14, (const u8*)HDR, sizeof(HDR) - 1u);
        }
    } else {
        rpt_raw(api, st, HDR, sizeof(HDR) - 1u);
    }

    rpt_line(api, st, p_reason_name(s->reset_reason));
    rpt_line(api, st, (const char*)s->actor_tag);
    rpt_line(api, st, st->ana.line1);
    rpt_line(api, st, st->ana.line2);

    static const char KH[] = "PC\0LR\0SP\0xP\0SG";
    const u32 vals[5] = { s->fault_pc, s->fault_lr, s->fault_sp, s->xpsr, st->ana.crash_sig };
    for (u8 i = 0; i < 5u; i++) {
        rpt_kv(api, st, KH + i * 3u, vals[i], (i == 2u || i == 4u));
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
