/*
 * Recovery Leaf 1: Dynamic Borderless Full-Screen Comfortable Menu
 *
 * Shows Flash Image, Applets, Peer Flash, Information, plus Multiboot and/or
 * Calibration automatically when /apps/multiboot.mod or /apps/calib.mod exists.
 * Outline highlight at rest, solid inverted block while holding MENU.
 */
#include "rec_shared.h"

#define ID_FLASH     0u
#define ID_APPLETS   1u
#define ID_PEER      2u
#define ID_MULTIBOOT 3u
#define ID_CALIB     4u
#define ID_INFO      5u

static void render_menu(const KrxApiTable* api, const RecState* st,
                        const u8* items, u8 count, u8 sel, u8 top, u8 solid) {
    static const char TITLES[] =
        "Flash Image\0Applets\0Peer Flash\0Multiboot\0Calibration\0Information\0";
    /* Built-in 8x8 Kern icons: Upload(49), Apps(46), Radio(25), Layers(47), Wrench(48), Info(30) */
    static const u8 ICONS[6] = { 49u, 46u, 25u, 47u, 48u, 30u };

    krx_clear(api);
    for (u8 row = 0u; row < 4u; row++) {
        u8 idx = (u8)(top + row);
        if (idx >= count) break;
        u8 id = items[idx];
        const char* t = st->ui.nth_s(TITLES, id);
        u8 flags = KRX_MENU_FLAG_ARROW;
        if (idx == sel) {
            flags |= KRX_MENU_FLAG_SEL | (solid ? 0u : KRX_MENU_FLAG_OUTLINE);
        }
        krx_ui_draw_menu_item_ex(
            api, (i16)(1 + row * 13), KRX_MENU_COMFORTABLE,
            (const u8*)t, (u8)krx_strlen(t),
            (const u8*)"", 0u, flags, ICONS[id]
        );
    }
    if (count > 4u) krx_ui_draw_scrollbar(api, 124u, 1u, 51u, (u16)sel, (u16)count, 4u);
    st->ui.softbar(api, "MENU", "Open", "EXIT", st->has_dfu ? "DFU" : "Exit");
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    RecState* st = (RecState*)ctx->arg;

    u8 items[6];
    u8 count = 0u;
    items[count++] = ID_FLASH;
    items[count++] = ID_APPLETS;
    items[count++] = ID_PEER;
    if (st->has_multiboot) items[count++] = ID_MULTIBOOT;
    if (st->has_calib)     items[count++] = ID_CALIB;
    items[count++] = ID_INFO;

    u8 sel = (st->menu_sel < count) ? st->menu_sel : 0u;
    u8 top = (sel >= 4u) ? (u8)(sel - 3u) : 0u;

    for (;;) {
        render_menu(api, st, items, count, sel, top, 0u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_UP)        sel = sel ? (u8)(sel - 1u) : (u8)(count - 1u);
        else if (k == KEY_DOWN) sel = (sel + 1u < count) ? (u8)(sel + 1u) : 0u;
        else if (k == KEY_EXIT) {
            st->menu_sel  = sel;
            st->exit_code = st->has_dfu ? REC_EXIT_DFU : REC_EXIT_OS;
            return 0u;
        } else if (k == KEY_MENU) {
            render_menu(api, st, items, count, sel, top, 1u);
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            st->menu_sel = sel;
            u8 id = items[sel];
            if (id == ID_FLASH || id == ID_APPLETS) {
                st->tool_sub  = (id == ID_APPLETS) ? 1u : 0u;
                st->next_leaf = REC_LEAF_SCAN;
            } else if (id == ID_PEER) {
                st->next_leaf = REC_LEAF_PEER;
            } else if (id == ID_INFO) {
                st->next_leaf = REC_LEAF_INFO;
            } else if (id == ID_MULTIBOOT) {
                st->exit_code = REC_EXIT_MULTIBOOT;
            } else if (id == ID_CALIB) {
                st->exit_code = REC_EXIT_CALIB;
            }
            return 0u;
        }
        if (sel < top) top = sel;
        if (sel >= top + 4u) top = (u8)(sel - 3u);
    }
}
