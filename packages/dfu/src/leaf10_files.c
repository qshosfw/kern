/*
 * DFU Leaf 10: Compact Flat Image & Applet List Browser
 */
#include "dfu_shared.h"

__attribute__((noinline))
static void render_list(const KrxApiTable* api, const DfuSharedState* st, u8 sel, u8 top, u8 solid) {
    const DfuToolsScratch* ts = (const DfuToolsScratch*)st->stage_buf;
    u8 applets = st->tool_sub;
    u8 count = ts->file_count;

    krx_clear(api);

    if (count == 0u) {
        const char* msg = applets ? "No .krx in /apps or /boot" : "No .kfw/.uf2 in /boot";
        krx_draw_str_align(api, 64, 28, (const u8*)msg, krx_strlen(msg), KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
        st->ui.softbar(api, "", "", "EXIT", "Back");
    } else {
        for (u8 row = 0u; row < 5u; row++) {
            u8 idx = (u8)(top + row);
            if (idx >= count) break;
            const DfuFlatFile* f = &ts->files[idx];
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
        if (count > 5u) krx_ui_draw_scrollbar(api, 124u, 1u, 50u, (u16)sel, (u16)count, 5u);
        st->ui.softbar(api, "MENU", applets ? "Run" : "Inspect", "EXIT", "Back");
    }
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf10_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    DfuToolsScratch* ts = (DfuToolsScratch*)st->stage_buf;
    u8 count = ts->file_count;
    u8 sel = (ts->file_sel < count) ? ts->file_sel : 0u;
    u8 top = (sel >= 5u) ? (u8)(sel - 4u) : 0u;

    for (;;) {
        render_list(api, st, sel, top, 0u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (count == 0u) continue;
        if (k == KEY_UP)        sel = (sel == 0u) ? (u8)(count - 1u) : (u8)(sel - 1u);
        else if (k == KEY_DOWN) sel = (sel + 1u >= count) ? 0u : (u8)(sel + 1u);
        else if (k == KEY_MENU) {
            render_list(api, st, sel, top, 1u);
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            const DfuFlatFile* f = &ts->files[sel];
            const char* pfx = (f->dir_idx == 0u) ? "/boot/" : (f->dir_idx == 1u) ? "/apps/" : "/";
            u32 pn = 0u, j = 0u;
            while (*pfx) ts->sel_path[pn++] = *pfx++;
            while (f->name[j] && pn < 31u) ts->sel_path[pn++] = f->name[j++];
            ts->sel_path[pn] = 0;
            ts->sel_size = f->size;
            ts->file_sel = sel;
            st->next_leaf = st->tool_sub ? DFU_LEAF_CONSOLE : DFU_LEAF_INSPECT;
            return 0u;
        }
        if (sel < top) top = sel;
        if (sel >= top + 5u) top = (u8)(sel - 4u);
    }
}
