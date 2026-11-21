/*
 * Multiboot Leaf 2: Borderless Live Slot & Golden Backup Status
 */
#include "mb_shared.h"

static void fmt_kb(const MbState* st, char* dst, u32 bytes) {
    u32 n = st->ui.u32_dec(dst, bytes >> 10);
    dst[n++] = ' '; dst[n++] = 'K'; dst[n++] = 'B'; dst[n] = 0;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    MbState* st = (MbState*)ctx->arg;

    char v_act[12], v_acrc[10], v_gold[14], v_gcrc[10], v_bat[12];
    fmt_kb(st, v_act, st->app_bytes);
    st->ui.fmt_hex32(v_acrc, st->app_crc);

    if (st->golden_synced) {
        mb_copy((u8*)v_gold, (const u8*)"Synced", 7u);
    } else if (st->golden_ok) {
        fmt_kb(st, v_gold, st->golden_bytes);
    } else {
        mb_copy((u8*)v_gold, (const u8*)"Missing", 8u);
    }

    if (st->golden_ok) st->ui.fmt_hex32(v_gcrc, st->golden_crc);
    else { v_gcrc[0] = '-'; v_gcrc[1] = 0; }

    u16 mv = st->ui.pwr_get(api, SUB_IO_POWER_BATT_MV);
    u32 bi = 0u, rem = mv, v = 0u, c = 0u;
    while (rem >= 1000u) { rem -= 1000u; v++; }
    while (rem >= 10u)   { rem -= 10u;   c++; }
    bi += st->ui.u32_dec(v_bat + bi, v);
    v_bat[bi++] = '.';
    if (c < 10u) v_bat[bi++] = '0';
    bi += st->ui.u32_dec(v_bat + bi, c);
    v_bat[bi++] = 'V'; v_bat[bi] = 0;

    static const char KEYS[] =
        "Active OS\0Active CRC\0Golden OS\0Golden CRC\0Vectors\0Battery\0";
    const char* vals[6] = {
        v_act, v_acrc, v_gold, v_gcrc, st->vec_ok ? "Valid" : "Invalid", v_bat
    };

    krx_clear(api);
    for (u8 i = 0u; i < 6u; i++) {
        st->ui.kv(api, (i16)(7 + i * 8), st->ui.nth_s(KEYS, i), vals[i]);
    }
    st->ui.softbar(api, "", "", "EXIT", "Back");
    krx_commit_frame(api);

    while (st->ui.wait_key(api) != KEY_EXIT) {}
    return 0u;
}
