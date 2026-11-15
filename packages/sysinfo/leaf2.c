/*
 * P0 Leaf 2: Firmware Meta, UTC Civil Date, Active Model & BootROM Scanner.
 * Chained from Leaf 1 inside the shared 1024B P0 overlay window.
 */

#include "sysinfo_shared.h"

static const char L2_MODEL_K1[] = "UV-K1";
static const char L2_MODEL_K5[] = "UV-K5 V3";
static const char L2_CFG_PATH[] = "/pref/system.cfg";

static void format_epoch_date(const KrxApiTable* api, u32 epoch, char* out) {
    u32 rem = 0;
    i32 z = (i32)si_udivmod(api, epoch, 86400u, &rem) + 719468;
    u32 era = si_udivmod(api, (u32)(z >= 0 ? z : z - 146096), 146097u, &rem);
    u32 doe = (u32)z - era * 146097u;
    u32 q1 = si_udivmod(api, doe, 1460u, 0);
    u32 q2 = si_udivmod(api, doe, 36524u, 0);
    u32 q3 = si_udivmod(api, doe, 146096u, 0);
    u32 yoe = si_udivmod(api, doe - q1 + q2 - q3, 365u, 0);
    i32 y = (i32)yoe + (i32)(era * 400u);
    u32 d1 = si_udivmod(api, yoe, 4u, 0);
    u32 d2 = si_udivmod(api, yoe, 100u, 0);
    u32 doy = doe - (365u * yoe + d1 - d2);
    u32 mp = si_udivmod(api, 5u * doy + 2u, 153u, 0);
    u32 d = doy - si_udivmod(api, 153u * mp + 2u, 5u, 0) + 1u;
    u32 m = mp + (mp < 10u ? 3u : (u32)-9);
    if (m <= 2u) y += 1;

    u32 hi = si_udivmod(api, (u32)y, 100u, &rem);
    si_put_2d(api, out + 0, hi);
    si_put_2d(api, out + 2, rem);
    out[4] = '-';
    si_put_2d(api, out + 5, m);
    out[7] = '-';
    si_put_2d(api, out + 8, d);
    out[10] = 0;
}

__attribute__((section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;

    SysInfoState* st = (SysInfoState*)ctx->arg;
    const CapServiceVTable* s = api->service;
    const CapStorageVTable* storage = api->storage;

    // 1. Firmware metadata (version + git hash + civil date)
    KrxFirmwareMeta meta;
    si_zero((u8*)&meta, sizeof(meta));
    if (s && s->sys_query && s->sys_query(KRX_SYS_QUERY_FW_META, 0, (u8*)&meta, sizeof(meta)) == sizeof(meta)) {
        u8 pos = 0;
        st->fw_badge[pos++] = 'v';
        pos += si_put_uint(api, st->fw_badge + pos, meta.version_major);
        st->fw_badge[pos++] = '.';
        pos += si_put_uint(api, st->fw_badge + pos, meta.version_minor);
        st->fw_badge[pos++] = '.';
        pos += si_put_uint(api, st->fw_badge + pos, meta.version_patch);
        st->fw_badge[pos++] = ' ';
        for (u8 i = 0; i < 7 && meta.git_hash[i] && pos < 17; i++) {
            st->fw_badge[pos++] = (char)meta.git_hash[i];
        }
        format_epoch_date(api, meta.build_epoch, st->build);
    }

    // 2. Active hardware model string from /pref/system.cfg
    u8 cfg[44];
    si_zero(cfg, sizeof(cfg));
    si_copy((u8*)st->model, (const u8*)L2_MODEL_K1, 6);
    if (storage && storage->storage_read_file &&
        storage->storage_read_file((const u8*)L2_CFG_PATH, 16, cfg, sizeof(cfg)) >= 40) {
        if (cfg[1] == 1) {
            si_copy((u8*)st->model, (const u8*)L2_MODEL_K5, 9);
        } else if (cfg[1] == 2) {
            u8 n = 0;
            while (n < 12 && cfg[28 + n] >= 32 && cfg[28 + n] <= 126 && cfg[28 + n] != ' ') {
                st->model[n] = (char)cfg[28 + n];
                n++;
            }
            if (n > 0) st->model[n] = 0;
        }
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
