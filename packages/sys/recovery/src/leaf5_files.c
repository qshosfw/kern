/*
 * Recovery Leaf 5: Borderless 5-Row Compact Flat File Browser
 */
#include "rec_shared.h"

__attribute__((noinline))
static void render_list(const KrxApiTable* api, const RecState* st, u8 sel, u8 top, u8 solid) {
    const RecScratch* sc = st->sc;
    u8 applets = st->tool_sub;
    u8 count = sc->file_count;

    krx_clear(api);
    if (count == 0u) {
        st->ui.draw_ctr(api, 28, applets ? "No .krx in /apps or /boot" : "No .kfw/.uf2/.bin in /boot or /user");
        st->ui.softbar(api, "", "", "EXIT", "Back");
    } else {
        for (u8 row = 0u; row < 5u; row++) {
            u8 idx = (u8)(top + row);
            if (idx >= count) break;
            const RecFlatFile* f = &sc->files[idx];
            char sz[8];
            u32 v = f->size;
            u32 sn = st->ui.u32_dec(sz, (v >= 1024u) ? (v >> 10) : v);
            sz[sn++] = (v >= 1024u) ? 'K' : 'B';
            sz[sn] = 0;
            u8 flags = (idx == sel) ? (KRX_MENU_FLAG_SEL | (solid ? 0u : KRX_MENU_FLAG_OUTLINE)) : 0u;
            krx_ui_draw_menu_item_ex(
                api, (i16)(1 + row * 10), KRX_MENU_COMPACT,
                (const u8*)f->name, (u8)krx_strlen(f->name), (const u8*)sz, (u8)sn, flags, 0u
            );
        }
        if (count > 5u) krx_ui_draw_scrollbar(api, 124u, 1u, 50u, (u16)top, (u16)count, 5u);
        st->ui.softbar(api, "MENU", applets ? "Run" : "Inspect", "EXIT", "Back");
    }
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf5_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    RecState* st = (RecState*)ctx->arg;

    RecScratch* sc = st->sc;
    u8 count = sc->file_count;
    u8 sel = (sc->file_sel < count) ? sc->file_sel : 0u;
    u8 top = (sel >= 5u) ? (u8)(sel - 4u) : 0u;

    for (;;) {
        render_list(api, st, sel, top, 0u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (!count) continue;
        if (k == KEY_UP)        sel = sel ? (u8)(sel - 1u) : (u8)(count - 1u);
        else if (k == KEY_DOWN) sel = (sel + 1u < count) ? (u8)(sel + 1u) : 0u;
        else if (k == KEY_MENU) {
            render_list(api, st, sel, top, 1u);
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            const RecFlatFile* f = &sc->files[sel];
            const char* pfx = (f->dir_idx == 0u) ? "/boot/" :
                              (f->dir_idx == 1u) ? "/apps/" :
                              (f->dir_idx == 2u) ? "/user/" : "/";
            u32 pn = 0u, j = 0u;
            while (*pfx) sc->sel_path[pn++] = *pfx++;
            while (f->name[j] && pn < 31u) sc->sel_path[pn++] = f->name[j++];
            sc->sel_path[pn] = 0;
            sc->sel_size = f->size;
            sc->file_sel = sel;
            st->next_leaf = st->tool_sub ? REC_LEAF_CONSOLE : REC_LEAF_INSPECT;
            return 0u;
        }
        if (sel < top) top = sel;
        if (sel >= top + 5u) top = (u8)(sel - 4u);
    }
}
