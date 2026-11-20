/*
 * Multiboot Leaf 7: Error Notice, Golden Warning & Hold-MENU 1.0 s Confirm
 */
#include "mb_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf7_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    MbState* st = (MbState*)ctx->arg;

    const char* base = st->sc->sel_path;
    for (const char* q = base; *q; q++) if (*q == '/') base = q + 1;
    if (!base[0]) base = "Golden OS";

    if (st->flash_err != 0u) {
        static const char ERRS[] =
            "Low battery - plug USB\0File extent error\0Decode failed\0Invalid ARM vectors\0Save Golden OS first\0";
        krx_clear(api);
        st->ui.draw_ctr(api, 22, base);
        st->ui.draw_ctr(api, 33, st->ui.nth_s(ERRS, (u8)(st->flash_err - 1u)));
        st->ui.softbar(api, "", "", "EXIT", "Back");
        krx_commit_frame(api);
        while (st->ui.wait_key(api) != KEY_EXIT) {}
        st->next_leaf = (st->flash_err == 5u) ? 0u : MB_LEAF_INSPECT;
        return 0u;
    }

    u8 warn_no_golden = (!st->golden_ok && !st->sc->job.is_golden) ? 1u : 0u;
    while (krx_poll_key(api) != 0u) krx_delay_ms(api, 10u);
    u8 hold = 0u;
    for (;;) {
        krx_clear(api);
        st->ui.draw_ctr(api, warn_no_golden ? 10 : 15, base);
        if (warn_no_golden) st->ui.draw_ctr(api, 19, "No Golden OS backup!");
        st->ui.draw_ctr(api, warn_no_golden ? 28 : 26, "Hold MENU to switch");
        st->ui.draw_box(api, 14, 35, 100, 6, 1u);
        st->ui.draw_box(api, 15, 36, 98, 4, 0u);
        if (hold) st->ui.draw_box(api, 16, 37, (u8)(hold * 3u), 2, 1u);
        st->ui.softbar(api, "MENU", "Hold", "EXIT", "Cancel");
        krx_commit_frame(api);

        u8 k = krx_poll_key(api);
        if (k == KEY_EXIT) {
            st->next_leaf = st->sc->job.is_golden ? 0u : MB_LEAF_INSPECT;
            return 0u;
        }
        if (k == KEY_MENU) { if (++hold >= 32u) break; }
        else hold = 0u;
        krx_delay_ms(api, 30u);
    }

    krx_clear(api);
    st->ui.draw_ctr(api, 18, base);
    st->ui.draw_ctr(api, 27, "Switching... Do not turn off");
    krx_commit_frame(api);
    st->next_leaf = MB_LEAF_FLASH_GO;
    return 0u;
}
