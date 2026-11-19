/*
 * DFU Leaf 8: Peer Flash (2-Pin Serial Radio-to-Radio Transfer)
 */
#include "dfu_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf8_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    u32 end_kb = 0u;
    for (u32 kb = 118u; kb > 0u; kb--) {
        const volatile u32* p = (const volatile u32*)(DFU_APP_FLASH_BASE + (kb - 1u) * 1024u);
        for (u32 w = 0u; w < 256u; w++) {
            if (p[w] != 0xFFFFFFFFu) { end_kb = kb; break; }
        }
        if (end_kb != 0u) break;
    }

    char sz[12];
    u32 n = st->ui.u32_dec(sz, end_kb);
    sz[n++] = ' '; sz[n++] = 'K'; sz[n++] = 'B'; sz[n] = 0;

    krx_clear(api);
    dfu_draw_header(api, "Peer Flash");
    st->ui.kv(api, 17, "Local Image", sz);
    st->ui.kv(api, 25, "Serial Link", "2-Pin K1 (38400)");
    st->ui.kv(api, 33, "Protocol",
              (st->kbhp.bl_kind == KBHP_BL_STOCK_K1) ? "Stock XOR v7" : "Standard UART");
    krx_draw_str_ex(api, 3, 43, (const u8*)"Connect 2-pin cable to", 22u, KRX_FONT_TOMTHUMB, 1u);
    krx_draw_str_ex(api, 3, 50, (const u8*)"target radio in DFU mode.", 25u, KRX_FONT_TOMTHUMB, 1u);
    st->ui.softbar(api, "", "", "EXIT", "Back");
    krx_commit_frame(api);

    for (;;) {
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT || k == KEY_MENU) return 0u;
    }
}
