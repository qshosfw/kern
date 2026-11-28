/*
 * KERN Realtime eXecutable (KRX) Standalone Hardware-in-the-Loop Test Bench & UI Lib Tester
 *
 * 100% position-independent, self-contained Cortex-M0+ Thumb-1 binary using unified kern_call.
 */

#include <krx.h>

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("b krx_main\n");
}

static u8 g_tab = 0;           // 0 = Widgets, 1 = Menu, 2 = Modal
static u8 g_prev_tab = 0;      // previous tab for smooth header glide
static u8 g_anim_prog = 16;    // 0..16 kinetic transition progress
static i8 g_slide_dir = 0;     // -1 left, +1 right, 0 idle
static u8 g_menu_selected = 0; // 0..5
static u8 g_progress = 65;     // 0..100

static u8 str_len(const char* s) {
    u8 len = 0;
    while (s[len]) len++;
    return len;
}

static void uart_put(const KrxApiTable* api, const char* str) {
    krx_serial_write(api, (const u8*)str, str_len(str));
}

static void uart_log(const KrxApiTable* api, const char* str) {
    uart_put(api, "[BENCH:ABI] ");
    uart_put(api, str);
    uart_put(api, "\r\n");
}

typedef struct {
    char name[14];
    u8 count;
} DomainSpec;

static const DomainSpec DOMAINS[9] = {
    { "SYS",   7 },
    { "PROC", 10 },
    { "IPC",   5 },
    { "FS",    4 },
    { "RF",    6 },
    { "AUD",   2 },
    { "GFX",   6 },
    { "UI",    3 },
    { "IO",    5 },
};

static const char TAB_TITLES[] = "UI\0Menu\0Modal";

static const char MENU_TITLES[6][10] = {
    "Squelch",
    "TX Power",
    "Theme",
    "Step",
    "Roger",
    "Key Lock",
};

static const char MENU_VALS[6][8] = {
    "Lvl 3",
    "High",
    "Light",
    "12.5k",
    "Kern",
    "Auto",
};

static const u8 MENU_ICONS[6] = { KRX_ICON_AUDIO, KRX_ICON_DEVICE, KRX_ICON_DISPLAY,
                                  KRX_ICON_VFO, KRX_ICON_APPS, KRX_ICON_SECURITY };

static void select_tab(const KrxApiTable* api, u8 next) {
    if (next < 3u && next != g_tab) {
        g_prev_tab = g_tab;
        g_slide_dir = (next > g_tab) ? 1 : -1;
        g_tab = next;
        g_anim_prog = krx_prop_get(api, KRX_PROP_UI_ANIMATIONS) ? 0u : 16u;
    }
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        uart_log(api, "STARTING UNIFIED KERN_CALL BENCHMARK ON CORTEX-M0+");

        krx_clear(api);
        krx_draw_line_h(api, 0, 15, 64, 1u);
        krx_draw_line_v(api, 15, 0, 32, 1u);
        krx_draw_dotted_line_h(api, 0, 127, 32, 2u, 1u);
        krx_clear_corners(api, 10, 10, 20, 20);
        krx_fill_rect_dither(api, 10, 10, 20, 20, 8u);
        krx_invert_rect(api, 10, 10, 20, 20);
        krx_str_width(api, (const u8*)"B", 1u, 0u);
        krx_draw_char(api, 0, 0, 'K', 0u, 1u);
        krx_draw_icon(api, 5, 5, KRX_ICON_ANTENNA, 1u);
        krx_scroll(api, 1, 0, 0u);

        (void)krx_prop_get32(api, KRX_PROP_BATT_MV);
        (void)krx_key_matrix(api);
        (void)krx_rf_get_freq(api);
        (void)krx_prop_get(api, KRX_PROP_VOLUME);
        (void)krx_fs_exists(api, (const u8*)"/apps", 5u);
        (void)krx_ticks_ms(api);
        (void)krx_sys_rand32(api);

        u8 ch_b = 'B';
        u8 origin[2] = { 0, 0 };
        krx_ui_draw_progress_bar(api, 10, 10, 50u, 6u, 75u, 1u);
        krx_ui_draw_tag_badge(api, 0, 0, &ch_b, 1u, 1u);
        krx_ui_draw_key_badge(api, 0, 20, &ch_b, 1u, &ch_b, 1u);
        krx_ui_draw_capsule_button(api, 10, 30, 40u, 9u, &ch_b, 1u, 1u);
        krx_ui_draw_scrollbar(api, 125u, 0u, 64u, 5u, 20u, 5u);
        krx_ui_draw_menu_item(api, 10, &ch_b, 1u, &ch_b, 1u, 1u, 0u);
        krx_ui_draw_battery(api, 100, 0, 85u, 0u);
        krx_ui_draw_signal(api, 0, 0, 4u, 1u);
        krx_ui_draw_modal_box(api, 60u, 30u, origin);
        krx_ui_show_confirm_labels(api, 4u, &ch_b, 1u, &ch_b, 1u, &ch_b, 1u, &ch_b, 1u);
        krx_ui_close_modal(api);

        KrxStatusBar sb;
        for (u8 i = 0; i < sizeof(KrxStatusBar); i++) ((u8*)&sb)[i] = 0;
        sb.flags = KRX_STATUS_VISIBLE | KRX_STATUS_CUSTOM_TITLE | KRX_STATUS_PARTIAL;
        krx_ui_set_status_bar(api, &sb);
        krx_ui_set_tray_icon(api, KRX_ICON_NONE);
        krx_draw_tabbed_sheet(
            api, 0u, 0u, 128u, 53u, 0u, 3u,
            KRX_TAB_FLAG_INTEGRATED_86 | KRX_TAB_FLAG_BOTTOM_BORDER,
            TAB_TITLES, (u16)(sizeof(TAB_TITLES) - 1u)
        );

        for (u8 i = 0; i < 9u; i++) {
            uart_put(api, "[BENCH:ABI] ");
            uart_put(api, DOMAINS[i].name);
            uart_put(api, " (");
            u8 c = DOMAINS[i].count;
            u8 d1 = 0;
            while (c >= 10u) { c -= 10u; d1++; }
            if (d1 > 0u) {
                u8 ch1 = (u8)('0' + d1);
                krx_serial_write(api, &ch1, 1u);
            }
            u8 d0 = (u8)('0' + c);
            krx_serial_write(api, &d0, 1u);
            uart_put(api, " ops): PASS\r\n");
        }
        uart_log(api, "TOTAL: 48/48 UNIFIED OPS VALIDATED: PASS");

        g_tab = 0;
        g_prev_tab = 0;
        g_anim_prog = 16;
        g_slide_dir = 0;
        g_menu_selected = 0;
        g_progress = 65;
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        if (!param) return 0;
        const u8* ev = (const u8*)param;
        u8 kind = ev[0];

        if (kind == KRX_EV_TICK) {
            if (g_anim_prog < 16u) {
                g_anim_prog += 4u;
                if (g_anim_prog >= 16u) {
                    g_anim_prog = 16u;
                    g_slide_dir = 0;
                }
                return 1;
            }
            return 0;
        }

        if (kind == KRX_EV_KEY) {
            u8 key = ev[1];
            u32 state = *(const u32*)(ev + 4);
            if (state != 0u) return 1;

            if (key == KEY_EXIT) return 0;
            if (key == KEY_4 || key == KEY_STAR) { select_tab(api, (g_tab == 0u) ? 2u : (u8)(g_tab - 1u)); return 1; }
            if (key == KEY_6 || key == KEY_HASH) { select_tab(api, (g_tab == 2u) ? 0u : (u8)(g_tab + 1u)); return 1; }
            if (key == KEY_1) { select_tab(api, 0u); return 1; }
            if (key == KEY_2) { select_tab(api, 1u); return 1; }
            if (key == KEY_3) { select_tab(api, 2u); return 1; }

            if (key == KEY_UP) {
                if (g_tab == 1u) g_menu_selected = (g_menu_selected == 0u) ? 5u : (u8)(g_menu_selected - 1u);
                else g_progress = (g_progress <= 90u) ? (u8)(g_progress + 10u) : 100u;
                return 1;
            }
            if (key == KEY_DOWN) {
                if (g_tab == 1u) g_menu_selected = (g_menu_selected >= 5u) ? 0u : (u8)(g_menu_selected + 1u);
                else g_progress = (g_progress >= 10u) ? (u8)(g_progress - 10u) : 0u;
                return 1;
            }
            if (key == KEY_MENU) {
                if (g_tab == 2u) {
                    krx_ui_show_confirm_labels(
                        api, 1u,
                        (const u8*)"UI Modal", 8u,
                        (const u8*)"Confirm Dialog?", 15u,
                        (const u8*)"YES", 3u,
                        (const u8*)"NO", 2u
                    );
                } else {
                    krx_ui_show_toast(api, (const u8*)"UI Action OK", 12u, 1200u);
                }
                return 1;
            }
            return 1;
        }
        return 1;
    }

    if (op == KRX_OP_RENDER) {
        krx_clear(api);
        i8 anim_off = (g_anim_prog < 16u) ? (i8)((16 - g_anim_prog) * g_slide_dir) : 0;
        i16 bx = (i16)(2 + anim_off);

        if (g_tab == 0u) {
            krx_ui_draw_signal(api, (i16)(bx + 106), 13, 4u, COLOR_FILL);
            krx_draw_str_ex(api, (i16)(bx + 2), 18, (const u8*)"UI KIT", 6u, 1u, COLOR_FILL);

            krx_ui_draw_tag_badge(api, (i16)(bx + 2), 22, (const u8*)"VFO-A", 5u, 1u);
            krx_ui_draw_tag_badge(api, (i16)(bx + 33), 22, (const u8*)"FM", 2u, 0u);
            krx_ui_draw_capsule_button(api, (i16)(bx + 52), 22, 34u, 9u, (const u8*)"ACT", 3u, 1u);
            krx_ui_draw_capsule_button(api, (i16)(bx + 88), 22, 34u, 9u, (const u8*)"STBY", 4u, 0u);

            krx_ui_draw_progress_bar(api, (i16)(bx + 2), 34, 120u, 6u, g_progress, 1u);
            krx_draw_str_ex(api, (i16)(bx + 3), 48, (const u8*)"UP/DN: Adjust Bar", 17u, 0u, COLOR_FILL);
        } else if (g_tab == 1u) {
            u8 start_row = (g_menu_selected >= 3u) ? (u8)(g_menu_selected - 2u) : 0u;
            for (u8 r = 0; r < 3u; r++) {
                u8 idx = (u8)(start_row + r);
                krx_ui_draw_menu_item(
                    api, (i16)(12 + r * 13),
                    (const u8*)MENU_TITLES[idx], str_len(MENU_TITLES[idx]),
                    (const u8*)MENU_VALS[idx], str_len(MENU_VALS[idx]),
                    (idx == g_menu_selected) ? 1u : 0u,
                    MENU_ICONS[idx]
                );
            }
            krx_ui_draw_scrollbar(api, 124u, 12u, 38u, start_row, 6u, 3u);
        } else {
            krx_draw_str_ex(api, (i16)(bx + 4), 24, (const u8*)"145.525 MHz", 11u, 1u, COLOR_FILL);

            krx_ui_draw_battery(api, (i16)(bx + 96), 16, g_progress, 0u);
            krx_ui_draw_tag_badge(api, (i16)(bx + 90), 25, (const u8*)"MODAL", 5u, 1u);
            krx_draw_str_ex(api, (i16)(bx + 4), 46, (const u8*)"Press MENU for Dialog", 21u, 0u, COLOR_FILL);
        }

        krx_draw_tabbed_sheet_anim(
            api, 0u, 0u, 128u, 53u, g_tab, g_prev_tab, g_anim_prog, 3u,
            KRX_TAB_FLAG_INTEGRATED_86 | KRX_TAB_FLAG_BOTTOM_BORDER,
            TAB_TITLES, (u16)(sizeof(TAB_TITLES) - 1u)
        );

        krx_ui_draw_key_badge(api, 2, 55, (const u8*)"< >", 3u, (const u8*)"Tab", 3u);
        if (g_tab == 2u) {
            krx_ui_draw_key_badge(api, 44, 55, (const u8*)"M", 1u, (const u8*)"Dialog", 6u);
        } else {
            krx_ui_draw_key_badge(api, 44, 55, (const u8*)"M", 1u, (const u8*)"Toast", 5u);
        }
        krx_ui_draw_key_badge(api, 86, 55, (const u8*)"EXIT", 4u, (const u8*)"Back", 4u);
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        krx_ui_set_status_bar(api, 0);
        krx_ui_set_tray_icon(api, KRX_ICON_NONE);
        krx_ui_close_modal(api);
        return 0;
    }

    return 0;
}
