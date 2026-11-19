/*
 * DFU Leaf 8: Peer Flash (Borderless 2-Pin K1 Radio-to-Radio Transfer Hub)
 *
 * Minimalist TX -> RX radio graphic + styled 2-row action menu without
 * top inverted header bar.
 */
#include "dfu_shared.h"

__attribute__((noinline))
static void draw_boxes(const KrxApiTable* api, i16 x_off, const u8* tbl, u32 count) {
    for (u32 i = 0u; i < count; i++, tbl += 5) {
        krx_fill_rect(api, (i16)(x_off + tbl[0]), (i16)tbl[1], tbl[2], tbl[3], tbl[4]);
    }
}

__attribute__((noinline))
static void draw_ctr(const KrxApiTable* api, i16 x, i16 y, const char* s) {
    krx_draw_str_align(api, x, y, (const u8*)s, (u16)krx_strlen(s),
                       KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
}

__attribute__((noinline))
static void render_peer(const KrxApiTable* api, const DfuSharedState* st,
                        const char* sz, u8 sel, u8 mode, u8 solid) {
    static const u8 RADIO_BOXES[5 * 5] = {
        2,  3,  2,  5, 1,
        12, 6,  3,  2, 1,
        0,  8, 18, 17, 1,
        3, 10, 12,  7, 0,
        4, 21, 10,  1, 0,
    };
    static const u8 CABLE_BOXES[5 * 5] = {
        30, 15,  4, 3, 1,
        94, 15,  4, 3, 1,
        34, 16, 60, 1, 1,
        62, 14,  2, 5, 1,
        64, 15,  2, 3, 1,
    };

    krx_clear(api);
    draw_boxes(api, 12, RADIO_BOXES, 5u);
    draw_boxes(api, 98, RADIO_BOXES, 5u);
    draw_boxes(api,  0, CABLE_BOXES, 5u);
    draw_ctr(api,  21, 15, "TX");
    draw_ctr(api, 107, 15, "RX");
    draw_ctr(api,  64, 11, "2-PIN 38.4K");

    if (mode == 0u) {
        for (u8 r = 0u; r < 2u; r++) {
            const char* lbl = r ? "Cable Guide" : "Send Image";
            const char* val = r ? "2-Pin K1" : sz;
            u8 flags = (r == sel) ? (KRX_MENU_FLAG_SEL | (solid ? 0u : KRX_MENU_FLAG_OUTLINE)) : 0u;
            krx_ui_draw_menu_item_ex(api, (i16)(29 + r * 11), KRX_MENU_COMPACT,
                (const u8*)lbl, (u8)krx_strlen(lbl), (const u8*)val, (u8)krx_strlen(val), flags, 0u);
        }
        st->ui.softbar(api, "MENU", "Open", "EXIT", "Back");
    } else {
        draw_ctr(api, 64, 37, (mode == 1u) ? "Hold PTT + power on RX" : "USB is Device-only");
        draw_ctr(api, 64, 46, (mode == 1u) ? "Ready: 2-pin K1 @ 38400" : "Use K1 2-pin TX/RX cable");
        st->ui.softbar(api, "", "", "EXIT", "Back");
    }
    krx_commit_frame(api);
}

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

    char sz[10];
    u32 n = st->ui.u32_dec(sz, end_kb);
    sz[n++] = ' '; sz[n++] = 'K'; sz[n++] = 'B'; sz[n] = 0;

    u8 sel = 0u, mode = 0u;
    for (;;) {
        render_peer(api, st, sz, sel, mode, 0u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) {
            if (mode != 0u) { mode = 0u; continue; }
            return 0u;
        }
        if (mode != 0u) continue;
        if (k == KEY_UP || k == KEY_DOWN) sel ^= 1u;
        else if (k == KEY_MENU) {
            render_peer(api, st, sz, sel, mode, 1u);
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            mode = (u8)(sel + 1u);
        }
    }
}
