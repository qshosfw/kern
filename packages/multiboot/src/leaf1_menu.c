/*
 * Multiboot Leaf 1: Borderless 4-Row Comfortable Menu (Outline/Solid)
 */
#include "mb_shared.h"

static void render_menu(const KrxApiTable* api, const MbState* st, u8 sel, u8 solid) {
    static const char TITLES[] = "Boot Slots\0Backup Kern OS\0Restore Golden\0Slot Status\0";
    static const u8 ICONS[4] = { KRX_ICON_SECURITY, KRX_ICON_UPLOAD, KRX_ICON_APP_RADIO, KRX_ICON_APP_ABOUT };

    krx_clear(api);
    for (u8 i = 0u; i < 4u; i++) {
        const char* t = st->ui.nth_s(TITLES, i);
        u8 flags = KRX_MENU_FLAG_ARROW;
        if (i == sel) flags |= KRX_MENU_FLAG_SEL | (solid ? 0u : KRX_MENU_FLAG_OUTLINE);
        krx_ui_draw_menu_item_ex(
            api, (i16)(1 + i * 13), KRX_MENU_COMFORTABLE,
            (const u8*)t, (u8)krx_strlen(t),
            (const u8*)"", 0u, flags, ICONS[i]
        );
    }
    const char* exit_lbl = (st->boot_mode && !st->has_recovery && st->has_dfu) ? "DFU" : "Back";
    st->ui.softbar(api, "MENU", "Open", "EXIT", exit_lbl);
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    MbState* st = (MbState*)ctx->arg;

    u8 sel = (st->menu_sel < 4u) ? st->menu_sel : 0u;
    for (;;) {
        render_menu(api, st, sel, 0u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_UP)        sel = sel ? (u8)(sel - 1u) : 3u;
        else if (k == KEY_DOWN) sel = (sel < 3u) ? (u8)(sel + 1u) : 0u;
        else if (k == KEY_EXIT) {
            st->menu_sel  = sel;
            st->exit_code = (st->boot_mode && !st->has_recovery && st->has_dfu)
                                ? MB_EXIT_DFU : MB_EXIT_BACK;
            return 0u;
        } else if (k == KEY_MENU) {
            render_menu(api, st, sel, 1u);
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            st->menu_sel = sel;
            if (sel == 0u) {
                st->next_leaf = MB_LEAF_SLOTS;
            } else if (sel == 1u) {
                st->next_leaf = MB_LEAF_BACKUP;
            } else if (sel == 2u) {
                if (!st->golden_ok) {
                    st->flash_err = 5u;
                    st->sc->sel_path[0] = 0;
                    st->next_leaf = MB_LEAF_FLASH_UI;
                } else {
                    static const char GP[] = "/boot/kern_golden.bin";
                    mb_copy((u8*)st->sc->sel_path, (const u8*)GP, sizeof(GP));
                    st->sc->sel_size = st->golden_bytes;
                    MbFlashJob* j = &st->sc->job;
                    mb_zero((u8*)j, sizeof(MbFlashJob));
                    j->fmt = FLASH_FMT_RAW;
                    j->is_golden = 1u;
                    j->stored_size = st->golden_bytes;
                    j->raw_size = st->golden_bytes;
                    st->next_leaf = MB_LEAF_FLASH_CHK;
                }
            } else {
                st->next_leaf = MB_LEAF_STATUS;
            }
            return 0u;
        }
    }
}
