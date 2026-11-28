/*
 * Recovery Leaf 2: Borderless Scrollable Device & Bootloader Diagnostics (12 Live Rows)
 */
#include "rec_shared.h"

#define INFO_ROWS 12u
#define INFO_VIS  7u

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    RecState* st = (RecState*)ctx->arg;

    KrxSysInfo si;
    rec_zero((u8*)&si, sizeof(si));
    krx_sys_info(api, &si);

    char bl_str[18];
    u32 app_kb = 0u;
    u8 vec_ok = 0u;
    u32 vidpid = st->ui.probe_bl(bl_str, &app_kb, &vec_ok);

    u16 mv  = st->ui.prop_get(api, KRX_PROP_BATT_MV);
    u16 pct = st->ui.prop_get(api, KRX_PROP_BATT_PCT);
    u16 chg = st->ui.prop_get(api, KRX_PROP_CHARGING);

    static const char KEYS[] =
        "Bootloader\0Firmware\0App Flash\0Vectors\0Battery\0Calib\0"
        "USB ID\0MCU\0Memory\0QS-1 Tag\0UID Hi\0UID Lo\0";

    u8 top = 0u;
    for (;;) {
        krx_clear(api);
        for (u8 r = 0u; r < INFO_VIS; r++) {
            u8 idx = (u8)(top + r);
            if (idx >= INFO_ROWS) break;
            char buf[20];
            const char* v = buf;
            u32 n = 0u;
            if (idx == 0u) {
                v = bl_str;
            } else if (idx == 1u) {
                buf[n++] = 'v';
                n += st->ui.u32_dec(buf + n, si.fw_major); buf[n++] = '.';
                n += st->ui.u32_dec(buf + n, si.fw_minor); buf[n++] = '.';
                n += st->ui.u32_dec(buf + n, si.fw_patch); buf[n] = 0;
            } else if (idx == 2u) {
                n = st->ui.u32_dec(buf, app_kb);
                buf[n++] = '/'; buf[n++] = '1'; buf[n++] = '1'; buf[n++] = '8'; buf[n++] = 'K'; buf[n] = 0;
            } else if (idx == 3u) {
                v = vec_ok ? "Valid" : "Corrupt";
            } else if (idx == 4u) {
                u32 q = 0u, rem = mv;
                while (rem >= 1000u) { rem -= 1000u; q++; }
                u32 d = 0u;
                while (rem >= 10u) { rem -= 10u; d++; }
                n = st->ui.u32_dec(buf, q); buf[n++] = '.';
                if (d < 10u) buf[n++] = '0';
                n += st->ui.u32_dec(buf + n, d); buf[n++] = 'V'; buf[n++] = ' ';
                n += st->ui.u32_dec(buf + n, pct); buf[n++] = chg ? '+' : '%'; buf[n] = 0;
            } else if (idx == 5u) {
                v = (si.flags & KRX_SYS_INFO_FLAG_CALIB_VALID) ? "Valid" : "Missing";
            } else if (idx == 6u) {
                u8 vp[4] = { (u8)(vidpid >> 24), (u8)(vidpid >> 16), (u8)(vidpid >> 8), (u8)vidpid };
                st->ui.fmt_hex(buf, vp, 2u); buf[4] = ':'; st->ui.fmt_hex(buf + 5, vp + 2, 2u);
            } else if (idx == 7u) {
                v = si.mcu;
            } else if (idx == 8u) {
                n = st->ui.u32_dec(buf, si.flash_kb); buf[n++] = 'K'; buf[n++] = '/';
                n += st->ui.u32_dec(buf + n, si.sram_kb); buf[n++] = 'K'; buf[n] = 0;
            } else if (idx == 9u) {
                v = si.qs1_len ? si.qs1 : "Missing";
            } else {
                st->ui.fmt_hex(buf, si.mcu_uid + ((idx == 10u) ? 0u : 6u), 6u);
            }
            st->ui.kv(api, (i16)(7 + r * 7), st->ui.nth_s(KEYS, idx), v);
        }
        krx_ui_draw_scrollbar(api, 125u, 1u, 50u, (u16)top, (u16)INFO_ROWS, INFO_VIS);
        st->ui.softbar(api, "", "", "EXIT", "Back");
        krx_commit_frame(api);

        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (k == KEY_UP && top) top--;
        if (k == KEY_DOWN && top + INFO_VIS < INFO_ROWS) top++;
    }
}
