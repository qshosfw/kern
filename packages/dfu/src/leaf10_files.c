/*
 * DFU Leaf 10: Flat Firmware Image & Applet List Browser
 */
#include "dfu_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf10_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    u8 applets = st->tool_sub;
    u8 count = st->buf.tools.file_count;
    u8 sel = st->buf.tools.file_sel;
    if (sel >= count) sel = 0u;
    u8 top = (sel >= 4u) ? (u8)(sel - 3u) : 0u;
    const char* hdr = applets ? "Applets" : "Flash Image";

    for (;;) {
        krx_clear(api);
        st->ui.header(api, hdr);

        if (count == 0u) {
            const char* empty = applets ? "No applets found" : "No firmware files";
            const char* hint  = applets ? "Copy .krx to /apps" : "Copy .kfw/.uf2 to /boot";
            krx_draw_str_align(api, 64, 28, (const u8*)empty, krx_strlen(empty),
                               KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
            krx_draw_str_align(api, 64, 38, (const u8*)hint, krx_strlen(hint),
                               KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
            st->ui.softbar(api, "", "", "EXIT", "Back");
        } else {
            for (u8 row = 0u; row < 4u; row++) {
                u8 idx = (u8)(top + row);
                if (idx >= count) break;
                const DfuFlatFile* f = &st->buf.tools.files[idx];
                char sz[8];
                u32 v = f->size;
                u32 sn = st->ui.u32_dec(sz, (v >= 1024u) ? (v >> 10) : v);
                sz[sn++] = (v >= 1024u) ? 'K' : 'B';
                sz[sn] = 0;
                krx_ui_draw_menu_item(
                    api, (i16)(10 + row * 11),
                    (const u8*)f->name, (u8)krx_strlen(f->name),
                    (const u8*)sz, (u8)sn,
                    (idx == sel) ? 1u : 0u, 0u
                );
            }
            if (count > 4u) {
                krx_ui_draw_scrollbar(api, 124u, 10u, 43u, (u16)sel, (u16)count, 4u);
            }
            st->ui.softbar(api, "MENU", "Inspect", "EXIT", "Back");
        }
        krx_commit_frame(api);

        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (count == 0u) continue;
        if (k == KEY_UP) {
            sel = (sel == 0u) ? (u8)(count - 1u) : (u8)(sel - 1u);
        } else if (k == KEY_DOWN) {
            sel = (sel + 1u >= count) ? 0u : (u8)(sel + 1u);
        } else if (k == KEY_MENU) {
            const DfuFlatFile* f = &st->buf.tools.files[sel];
            const char* pfx = (f->dir_idx == 0u) ? "/boot/" : (f->dir_idx == 1u) ? "/apps/" : "/";
            u32 pn = 0u, j = 0u;
            while (*pfx) st->buf.tools.sel_path[pn++] = *pfx++;
            while (f->name[j] && pn < 31u) {
                st->buf.tools.sel_name[j] = f->name[j];
                st->buf.tools.sel_path[pn++] = f->name[j++];
            }
            st->buf.tools.sel_name[j] = 0;
            st->buf.tools.sel_path[pn] = 0;
            st->buf.tools.sel_size = f->size;
            st->buf.tools.file_sel = sel;
            st->tool_action = DFU_TOOL_ACT_INSPECT;
            return 0u;
        }
        if (sel < top) top = sel;
        if (sel >= top + 4u) top = (u8)(sel - 3u);
    }
}
