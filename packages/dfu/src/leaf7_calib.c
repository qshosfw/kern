/*
 * DFU Leaf 7: Compact Calibration Viewer & Action Launcher
 *
 * Delegates file I/O (Load / Save+Ascon-128 Sign / Backup) to Leaf 13 (`leaf13_calio.c`)
 * and live field editing to Leaf 14 (`leaf14_caledit.c`).
 */
#include "dfu_shared.h"

#define CAL_ITEMS      8u
#define CAL_EDIT_COUNT 6u

static const char* nth_s(const char* p, u8 i) {
    while (i--) { while (*p) p++; p++; }
    return p;
}

__attribute__((noinline))
static void render_cal(const KrxApiTable* api, const DfuSharedState* st,
                       u8 hdr_idx, u8 sel, u8 top, u8 loaded, u8 solid) {
    static const char LABELS[] =
        "XTAL Trim\0Vol Gain\0DAC Gain\0LNA Calib\0Mix Calib\0Batt 7.6V\0Save & Sign\0Backup\0";
    static const char MSGS[] =
        "Calibration\0No calib\0Signed OK\0Sign Err\0Backup OK\0Backup Err\0";
    static const u8 OFF_REL[CAL_EDIT_COUNT] = { 0x48u, 0x4Eu, 0x4Fu, 0x4Au, 0x4Cu, 0x06u };
    const u8* buf = st->stage_buf;

    krx_clear(api);
    dfu_draw_header(api, nth_s(MSGS, hdr_idx));

    for (u8 r = 0u; r < 4u; r++) {
        u8 idx = (u8)(top + r);
        if (idx >= CAL_ITEMS) break;
        const char* lbl = nth_s(LABELS, idx);
        char val[8];
        u32 vn = 0u;
        if (idx < CAL_EDIT_COUNT && loaded) {
            u32 o = 0x140u + OFF_REL[idx];
            i32 v = (o >= 0x18Eu) ? (i32)buf[o]
                                  : (i32)((u16)buf[o] | ((u16)buf[o + 1u] << 8));
            if (idx == 0u) v = (i32)(i16)v;
            if (v < 0) { val[vn++] = '-'; v = -v; }
            vn += st->ui.u32_dec(val + vn, (u32)v);
        }
        val[vn] = 0;
        u8 flags = (idx == sel) ? (KRX_MENU_FLAG_SEL | (solid ? 0u : KRX_MENU_FLAG_OUTLINE)) : 0u;
        krx_ui_draw_menu_item_ex(
            api, (i16)(10 + r * 11), KRX_MENU_COMPACT,
            (const u8*)lbl, (u8)krx_strlen(lbl), (const u8*)val, (u8)vn, flags, 0u
        );
    }
    krx_ui_draw_scrollbar(api, 124u, 10u, 43u, (u16)sel, (u16)CAL_ITEMS, 4u);
    st->ui.softbar(api, "MENU", (sel < CAL_EDIT_COUNT) ? "Edit" : "Run", "EXIT", "Back");
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf7_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    u8 act = st->tool_action;
    u8 loaded = (act != 1u);
    u8 hdr_idx = (act <= 5u) ? act : 0u;
    u8 sel = (act >= 10u) ? (u8)(act - 10u) : (act >= 4u ? 7u : (act >= 2u ? 6u : 0u));
    if (sel >= CAL_ITEMS) sel = 0u;
    u8 top = (sel >= 4u) ? (u8)(sel - 3u) : 0u;

    for (;;) {
        render_cal(api, st, hdr_idx, sel, top, loaded, 0u);
        u8 k = st->ui.wait_key(api);
        hdr_idx = 0u;
        if (k == KEY_EXIT) return 0u;
        if (k == KEY_UP)        sel = sel ? (u8)(sel - 1u) : (CAL_ITEMS - 1u);
        else if (k == KEY_DOWN) sel = (sel + 1u < CAL_ITEMS) ? (u8)(sel + 1u) : 0u;
        else if (k == KEY_MENU && loaded) {
            render_cal(api, st, hdr_idx, sel, top, loaded, 1u);
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            if (sel < CAL_EDIT_COUNT) {
                st->tool_sub = sel;
                st->next_leaf = DFU_LEAF_CALEDIT;
                return 0u;
            }
            st->tool_sub = (sel == 6u) ? 1u : 2u;
            st->next_leaf = DFU_LEAF_CALIO;
            return 0u;
        }
        if (sel < top) top = sel;
        if (sel >= top + 4u) top = (u8)(sel - 3u);
    }
}
