/*
 * DFU Leaf 5: RTC_ALR Tools Gate & Full-Screen Comfortable Recovery Menu
 *
 * No top title bar -> 4 visible Comfortable (13 px) rows fill y=1..52 directly
 * above the y=54..60 square soft bar. Uses Settings-style outline selection at
 * rest and full solid inverted block while MENU is held.
 */
#include "dfu_shared.h"

static u8 rtc_consume_tools_flag(void) {
    *(volatile u32*)0x4002103Cu |= (1u << 28) | (1u << 10);
    *(volatile u32*)0x40007000u |= (1u << 8);
    volatile u32* rtc = (volatile u32*)0x40002800u;
    u32 raw = ((rtc[0x20 / 4] & 0xFFFFu) << 16) | (rtc[0x24 / 4] & 0xFFFFu);
    if (((raw >> 24) & 0xFFu) != KBHP_RTC_IDENT) return 0u;
    if (((raw >> 8) & KBHP_RTC_FLAG_TOOLS) == 0u) return 0u;

    u32 cleared = raw & ~((u32)KBHP_RTC_FLAG_TOOLS << 8);
    for (u32 t = 50000u; ((rtc[0x04 / 4] & (1u << 5)) == 0u) && t; t--) {}
    rtc[0x04 / 4] |= (1u << 4);
    rtc[0x20 / 4] = cleared >> 16;
    rtc[0x24 / 4] = cleared & 0xFFFFu;
    rtc[0x04 / 4] &= ~(1u << 4);
    for (u32 t = 50000u; ((rtc[0x04 / 4] & (1u << 5)) == 0u) && t; t--) {}
    return 1u;
}

static const char* nth_str(const char* tbl, u8 idx) {
    while (idx > 0u) {
        while (*tbl) tbl++;
        tbl++;
        idx--;
    }
    return tbl;
}

static void render_menu(const KrxApiTable* api, const DfuSharedState* st, u8 sel, u8 top, u8 solid) {
    static const char TITLES[] =
        "Flash Image\0"
        "Applets\0"
        "Peer Flash\0"
        "Information\0";
    /* Built-in 8x8 Kern icons: Upload(49), Apps(46), Radio(25), Info(30) */
    static const u8 ICONS[DFU_TOOL_ITEM_COUNT] = { 49u, 46u, 25u, 30u };

    krx_clear(api);
    for (u8 row = 0u; row < 4u; row++) {
        u8 idx = (u8)(top + row);
        if (idx >= DFU_TOOL_ITEM_COUNT) break;
        const char* t = nth_str(TITLES, idx);
        u8 flags = KRX_MENU_FLAG_ARROW;
        if (idx == sel) {
            flags |= KRX_MENU_FLAG_SEL;
            if (!solid) flags |= KRX_MENU_FLAG_OUTLINE;
        }
        krx_ui_draw_menu_item_ex(
            api, (i16)(1 + row * 13), KRX_MENU_COMFORTABLE,
            (const u8*)t, (u8)krx_strlen(t),
            (const u8*)"", 0u, flags, ICONS[idx]
        );
    }
    st->ui.softbar(api, "MENU", "Open", "EXIT", "DFU");
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf5_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    if (!st->tools_active) {
        if (!rtc_consume_tools_flag()) {
            st->tool_action = DFU_TOOL_ACT_DFU;
            return 0u;
        }
        st->tools_active = 1u;
    }

    u8 sel = st->tool_sel;
    if (sel >= DFU_TOOL_ITEM_COUNT) sel = 0u;
    u8 top = (sel >= 4u) ? (u8)(sel - 3u) : 0u;

    for (;;) {
        render_menu(api, st, sel, top, 0u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_UP) {
            sel = (sel == 0u) ? (u8)(DFU_TOOL_ITEM_COUNT - 1u) : (u8)(sel - 1u);
        } else if (k == KEY_DOWN) {
            sel = (sel + 1u >= DFU_TOOL_ITEM_COUNT) ? 0u : (u8)(sel + 1u);
        } else if (k == KEY_MENU) {
            render_menu(api, st, sel, top, 1u); /* Solid inverted block while holding MENU */
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            st->tool_sel = sel;
            st->tool_action = DFU_TOOL_ACT_OPEN;
            return 0u;
        } else if (k == KEY_EXIT) {
            st->tool_sel = sel;
            st->tool_action = DFU_TOOL_ACT_DFU;
            return 0u;
        }
        if (sel < top) top = sel;
        if (sel >= top + 4u) top = (u8)(sel - 3u);
    }
}
