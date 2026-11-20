/*
 * Multiboot Leaf 4: Borderless 5-Row Boot Slot Browser (/boot/*.kfw, *.uf2, *.bin)
 */
#include "mb_shared.h"

typedef struct {
    u8  name[24];
    u32 size;
    u8  is_dir;
    u8  reserved[3];
} MbDirEntry;

static u8 is_img(const char* name, u32 len) {
    if (len < 4u || name[len - 4u] != '.') return 0u;
    const char* s = name + len - 3u;
    u32 w = ((u32)(s[0] | 32u) << 16) | ((u32)(s[1] | 32u) << 8) | (u32)(s[2] | 32u);
    return (w == 0x6B6677u || w == 0x756632u || w == 0x62696Eu); /* kfw, uf2, bin */
}

static void render_slots(const KrxApiTable* api, const MbState* st, u8 sel, u8 top, u8 solid) {
    MbScratch* sc = st->sc;
    krx_clear(api);
    if (!sc->slot_count) {
        st->ui.draw_ctr(api, 23, "No images in /boot");
        st->ui.draw_ctr(api, 34, "Copy .kfw/.uf2/.bin");
    } else {
        for (u8 row = 0u; row < 5u; row++) {
            u8 idx = (u8)(top + row);
            if (idx >= sc->slot_count) break;
            const MbSlot* f = &sc->slots[idx];
            u8 flags = KRX_MENU_FLAG_ARROW;
            if (idx == sel) flags |= KRX_MENU_FLAG_SEL | (solid ? 0u : KRX_MENU_FLAG_OUTLINE);
            krx_ui_draw_menu_item_ex(
                api, (i16)(1 + row * 10), KRX_MENU_COMPACT,
                (const u8*)f->name, (u8)krx_strlen(f->name),
                (const u8*)"", 0u, flags, 0xFFu
            );
        }
        if (sc->slot_count > 5u) {
            krx_ui_draw_scrollbar(api, 124u, 1u, 50u, (u16)top, (u16)sc->slot_count, 5u);
        }
    }
    st->ui.softbar(api, sc->slot_count ? "MENU" : "", sc->slot_count ? "Inspect" : "", "EXIT", "Back");
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf4_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    MbState* st = (MbState*)ctx->arg;
    MbScratch* sc = st->sc;

    static const char BDIR[] = "/boot";
    MbDirEntry ents[12];
    KrxFsReq req;
    req.path = (const u8*)BDIR; req.buf = (u8*)ents;
    req.offset = 0u; req.len = 12u; req.path_len = sizeof(BDIR) - 1u; req.extra = 0u;
    u32 n = krx_call(api, OP_FS_READ, SUB_FS_READ_LIST_DIR, (u32)&req, 0u);
    if (n > 12u) n = 12u;
    sc->slot_count = 0u;
    for (u32 i = 0u; i < n && sc->slot_count < MB_MAX_SLOTS; i++) {
        if (ents[i].is_dir) continue;
        const char* nm = (const char*)ents[i].name;
        u32 nl = krx_strlen(nm);
        if (is_img(nm, nl)) {
            MbSlot* dst = &sc->slots[sc->slot_count++];
            u32 cl = (nl < 19u) ? nl : 19u;
            mb_copy((u8*)dst->name, (const u8*)nm, cl); dst->name[cl] = 0;
            dst->size = ents[i].size;
        }
    }

    u8 sel = (sc->slot_sel < sc->slot_count) ? sc->slot_sel : 0u;
    u8 top = (sel >= 5u) ? (u8)(sel - 4u) : 0u;

    for (;;) {
        render_slots(api, st, sel, top, 0u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (!sc->slot_count) continue;
        if (k == KEY_UP)        sel = sel ? (u8)(sel - 1u) : (u8)(sc->slot_count - 1u);
        else if (k == KEY_DOWN) sel = (sel + 1u < sc->slot_count) ? (u8)(sel + 1u) : 0u;
        else if (k == KEY_MENU) {
            render_slots(api, st, sel, top, 1u);
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            sc->slot_sel = sel;
            const MbSlot* f = &sc->slots[sel];
            mb_copy((u8*)sc->sel_path, (const u8*)"/boot/", 6u);
            u32 nl = krx_strlen(f->name);
            mb_copy((u8*)(sc->sel_path + 6), (const u8*)f->name, nl + 1u);
            sc->sel_size = f->size;
            st->next_leaf = MB_LEAF_INSPECT;
            return 0u;
        }
        if (sel < top) top = sel;
        if (sel >= top + 5u) top = (u8)(sel - 4u);
    }
}
