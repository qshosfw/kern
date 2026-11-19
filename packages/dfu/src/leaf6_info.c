/*
 * DFU Leaf 6: Bootloader & Hardware Information Screen
 */
#include "dfu_shared.h"

static void fmt_hex16(char* dst, u16 v) {
    for (u32 i = 0u; i < 4u; i++) {
        u8 nib = (u8)(v >> 12);
        v <<= 4;
        dst[i] = (char)(nib + (nib < 10u ? '0' : ('A' - 10)));
    }
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf6_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;
    const KbhpEnvelope* env = &st->kbhp;

    krx_clear(api);
    dfu_draw_header(api, "Information");

    char buf[20];
    u32 n = 0u;
    const char* fam = (env->bl_kind == KBHP_BL_STOCK_K1)   ? "Stock " :
                      (env->bl_kind == KBHP_BL_STOCK_SAFE) ? "Safe "  :
                      (env->bl_kind == KBHP_BL_MOTO)       ? "Moto "  : "Custom ";
    while (*fam && n < 10u) buf[n++] = *fam++;
    for (u32 i = 0u; i < 8u && env->bl_ver[i] && n < 19u; i++) buf[n++] = env->bl_ver[i];
    buf[n] = 0;
    st->ui.kv(api, 17, "Bootloader", buf);

    if (env->bl_vid_pid != 0u) {
        fmt_hex16(buf, (u16)(env->bl_vid_pid >> 16));
        buf[4] = ':';
        fmt_hex16(buf + 5, (u16)(env->bl_vid_pid & 0xFFFFu));
        buf[9] = 0;
    } else {
        dfu_copy_bytes((u8*)buf, (const u8*)"None", 5u);
    }
    st->ui.kv(api, 25, "USB ID", buf);

    st->ui.kv(api, 33, "Factory ID",
              (env->flags & KBHP_FLAG_FACTORY_PAGE_OK) ? "QS-1 Valid" : "Missing");

    u16 mv = krx_pwr_u16(api, SUB_IO_POWER_BATT_MV);
    if (mv > 0u) {
        u16 pct = krx_pwr_u16(api, SUB_IO_POWER_BATT_PCT);
        n = st->ui.u32_dec(buf, (u32)mv);
        buf[n++] = 'm'; buf[n++] = 'V'; buf[n++] = ' '; buf[n++] = '(';
        n += st->ui.u32_dec(buf + n, (u32)pct);
        buf[n++] = '%'; buf[n++] = ')'; buf[n] = 0;
        st->ui.kv(api, 41, "Battery", buf);
    } else {
        st->ui.kv(api, 41, "App Region", "118 KB @ 0x2800");
    }

    st->ui.kv(api, 49, "Target", "PY32F071 / KBHP 1.0");
    st->ui.softbar(api, "", "", "EXIT", "Back");
    krx_commit_frame(api);

    for (;;) {
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT || k == KEY_MENU) return 0u;
    }
}
