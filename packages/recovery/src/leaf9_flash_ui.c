/*
 * Recovery Leaf 9: Error Notice, Hold-MENU 1.0 s Confirmation & Flashing Frame
 */
#include "rec_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf9_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    RecState* st = (RecState*)ctx->arg;

    const char* base = st->sc->sel_path;
    for (const char* q = base; *q; q++) if (*q == '/') base = q + 1;

    if (st->flash_err != 0u) {
        static const char ERRS[] =
            "Low battery - plug USB\0File extent error\0Decode failed\0Invalid ARM vectors\0";
        krx_clear(api);
        st->ui.draw_ctr(api, 22, base);
        st->ui.draw_ctr(api, 33, st->ui.nth_s(ERRS, (u8)(st->flash_err - 1u)));
        st->ui.softbar(api, "", "", "EXIT", "Back");
        krx_commit_frame(api);
        while (st->ui.wait_key(api) != KEY_EXIT) {}
        st->next_leaf = REC_LEAF_INSPECT;
        return 0u;
    }

    while (krx_poll_key(api) != 0u) krx_delay_ms(api, 10u);
    u8 hold = 0u;
    for (;;) {
        krx_clear(api);
        st->ui.draw_ctr(api, 15, base);
        st->ui.draw_ctr(api, 26, "Hold MENU to flash");
        st->ui.draw_box(api, 14, 34, 100, 6, 1u);
        st->ui.draw_box(api, 15, 35, 98, 4, 0u);
        if (hold) st->ui.draw_box(api, 16, 36, (u8)(hold * 3u), 2, 1u);
        st->ui.softbar(api, "MENU", "Hold", "EXIT", "Cancel");
        krx_commit_frame(api);

        u8 k = krx_poll_key(api);
        if (k == KEY_EXIT) { st->next_leaf = REC_LEAF_INSPECT; return 0u; }
        if (k == KEY_MENU) { if (++hold >= 32u) break; }
        else hold = 0u;
        krx_delay_ms(api, 30u);
    }

    krx_clear(api);
    st->ui.draw_ctr(api, 18, base);
    st->ui.draw_ctr(api, 27, "Flashing... Do not turn off");
    krx_commit_frame(api);
    st->next_leaf = REC_LEAF_FLASH_GO;
    return 0u;
}
