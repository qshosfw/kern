/*
 * DFU Leaf 6: Scrollable Bootloader & Device Information Screen (9 Live Entries)
 */
#include "dfu_shared.h"

#define INFO_ROWS 9u
#define INFO_VIS  6u

static const char* nth_s(const char* p, u8 i) {
    while (i--) { while (*p) p++; p++; }
    return p;
}

static void fmt_hex(char* dst, const u8* src, u32 n) {
    for (u32 i = 0u; i < n; i++) {
        u8 b = src[i], hi = b >> 4, lo = b & 0xFu;
        dst[i * 2u]     = (char)(hi + (hi < 10u ? '0' : ('A' - 10)));
        dst[i * 2u + 1] = (char)(lo + (lo < 10u ? '0' : ('A' - 10)));
    }
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf6_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;
    const KbhpEnvelope* env = &st->kbhp;

    KrxSysInfo si;
    dfu_zero_bytes((u8*)&si, sizeof(si));
    krx_sys_info(api, &si);

    static const char KEYS[] =
        "Bootloader\0USB ID\0Factory ID\0QS-1 Tag\0MCU\0MCU UID\0Flash/RAM\0Firmware\0Protocol\0";

    u8 top = 0u;
    for (;;) {
        krx_clear(api);
        dfu_draw_header(api, "Information");
        for (u8 r = 0u; r < INFO_VIS; r++) {
            u8 idx = (u8)(top + r);
            if (idx >= INFO_ROWS) break;
            char buf[20];
            const char* v = buf;
            u32 n = 0u;
            if (idx == 0u) {
                const char* fam = (env->bl_kind == KBHP_BL_STOCK_K1)   ? "Stock " :
                                  (env->bl_kind == KBHP_BL_STOCK_SAFE) ? "Safe "  :
                                  (env->bl_kind == KBHP_BL_MOTO)       ? "Moto "  : "Custom ";
                while (*fam) buf[n++] = *fam++;
                for (u32 i = 0u; i < 8u && env->bl_ver[i]; i++) buf[n++] = env->bl_ver[i];
                buf[n] = 0;
            } else if (idx == 1u) {
                u8 vp[4] = {
                    (u8)(env->bl_vid_pid >> 24), (u8)(env->bl_vid_pid >> 16),
                    (u8)(env->bl_vid_pid >> 8),  (u8)env->bl_vid_pid
                };
                fmt_hex(buf, vp, 2u); buf[4] = ':'; fmt_hex(buf + 5, vp + 2, 2u); buf[9] = 0;
            } else if (idx == 2u) {
                v = (env->flags & KBHP_FLAG_FACTORY_PAGE_OK) ? "QS-1 Valid" : "Missing";
            } else if (idx == 3u) {
                v = si.qs1_len ? si.qs1 : "--";
            } else if (idx == 4u) {
                v = si.mcu;
            } else if (idx == 5u) {
                fmt_hex(buf, si.mcu_uid, 6u); buf[12] = 0;
            } else if (idx == 6u) {
                n = st->ui.u32_dec(buf, si.flash_kb); buf[n++] = 'K'; buf[n++] = '/';
                n += st->ui.u32_dec(buf + n, si.sram_kb); buf[n++] = 'K'; buf[n] = 0;
            } else if (idx == 7u) {
                buf[n++] = 'v';
                n += st->ui.u32_dec(buf + n, si.fw_major); buf[n++] = '.';
                n += st->ui.u32_dec(buf + n, si.fw_minor); buf[n++] = '.';
                n += st->ui.u32_dec(buf + n, si.fw_patch); buf[n] = 0;
            } else {
                v = (si.flags & KRX_SYS_INFO_FLAG_CALIB_VALID) ? "KBHP 1.0 (Cal)" : "KBHP 1.0";
            }
            st->ui.kv(api, (i16)(16 + r * 7), nth_s(KEYS, idx), v);
        }
        krx_ui_draw_scrollbar(api, 125u, 11u, 41u, (u16)top, (u16)INFO_ROWS, INFO_VIS);
        st->ui.softbar(api, "", "", "EXIT", "Back");
        krx_commit_frame(api);

        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (k == KEY_UP && top > 0u) top--;
        if (k == KEY_DOWN && top + INFO_VIS < INFO_ROWS) top++;
    }
}
