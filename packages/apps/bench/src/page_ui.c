/*
 * P1 v2 Block 0: UI Kit (Tab 0) & Interactive Menu/Modal (Tab 1).
 */

#include "bench_shared.h"

static const char MENU_TITLES[6][10] = {
    "Squelch", "TX Power", "Theme", "Step", "Roger", "Key Lock"
};
static const char MENU_VALS[6][8] = {
    "Lvl 3", "High", "Light", "12.5k", "Kern", "Auto"
};
static const u8 MENU_ICONS[6] = {
    KRX_ICON_AUDIO, KRX_ICON_DEVICE, KRX_ICON_DISPLAY,
    KRX_ICON_VFO, KRX_ICON_APPS, KRX_ICON_SECURITY
};

int block_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl block_main\n pop {pc}\n");
}

int block_main(u32 op, const KrxApiTable* api, const void* param) {
    (void)op;
    if (!api || !param) return 0;
    const u32* ctx = (const u32*)param;
    const BenchState* st = (const BenchState*)ctx[1];
    if (!st) return 0;

    i8 anim_off = (st->anim_prog < 16u) ? (i8)((16 - st->anim_prog) * st->slide_dir) : 0;
    i16 bx = (i16)(2 + anim_off);

    if (st->tab == 0u) {
        krx_draw_str_ex(api, (i16)(bx + 2), 20, (const u8*)"UI KIT", 6u, KRX_FONT_MUMATRIX, COLOR_FILL);
        krx_ui_draw_signal(api, (i16)(bx + 90), 14, 4u, COLOR_FILL);
        krx_ui_draw_battery(api, (i16)(bx + 106), 14, st->progress, 0u);

        krx_ui_draw_tag_badge(api, (i16)(bx + 2), 24, (const u8*)"VFO-A", 5u, 1u);
        krx_ui_draw_tag_badge(api, (i16)(bx + 32), 24, (const u8*)"FM", 2u, 0u);
        krx_ui_draw_capsule_button(api, (i16)(bx + 50), 24, 34u, 9u, (const u8*)"ACT", 3u, 1u);
        krx_ui_draw_capsule_button(api, (i16)(bx + 87), 24, 34u, 9u, (const u8*)"STBY", 4u, 0u);

        krx_ui_draw_progress_bar(api, (i16)(bx + 2), 37, 118u, 6u, st->progress, 1u);
        krx_draw_str_ex(api, (i16)(bx + 2), 50, (const u8*)"UP/DN:BAR  M:TOAST", 18u, KRX_FONT_TOMTHUMB, COLOR_FILL);
    } else {
        u8 start_row = (st->menu_sel >= 3u) ? (u8)(st->menu_sel - 2u) : 0u;
        for (u8 r = 0; r < 3u; r++) {
            u8 idx = (u8)(start_row + r);
            krx_ui_draw_menu_item(
                api, (i16)(13 + r * 12),
                (const u8*)MENU_TITLES[idx], bench_slen(MENU_TITLES[idx]),
                (const u8*)MENU_VALS[idx], bench_slen(MENU_VALS[idx]),
                (idx == st->menu_sel) ? 1u : 0u,
                MENU_ICONS[idx]
            );
        }
        krx_ui_draw_scrollbar(api, 123u, 13u, 37u, start_row, 6u, 3u);
    }
    return 0;
}
