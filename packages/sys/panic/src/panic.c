/*
 * KERN Panic Screen & Safe Mode Recovery Module (/apps/panic.mod)
 *
 * Hardened P0 Structured Paged Module focused on instant post-fault recovery:
 * - Page-based navigation (5 full pages: Overview, Core Regs, Extended Regs,
 *   Stack & Trace, Disassembly [PC-4..PC+4]) — UP/DOWN, * / #, or 1..5 flip pages.
 * - 10-second auto-continue on single fault (MENU/EXIT continues immediately,
 *   any page navigation pauses countdown, SIDE1 dumps serial, SIDE2 saves /log/panic.txt,
 *   0 reboots radio).
 * - Crash-loop Safe Mode recovery menu (param == 2).
 */
#include "panic_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((visibility("default"), naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl krx_main\n pop {pc}\n");
}

static PanicState g_st;

static void show_toast(const char* msg) {
    p_copy_cstr(g_st.toast_msg, msg, 23u);
    g_st.toast_ticks = 35u;
    g_st.dirty = 1u;
}

static void init_panic_data(const KrxApiTable* api, u8 mode) {
    for (u16 i = 0; i < (u16)sizeof(g_st); i++) ((u8*)&g_st)[i] = 0u;
    g_st.mode = mode;
    g_st.dirty = 1u;
    g_st.boot_rem = (mode == MODE_BOOT_DIALOG) ? PANIC_TIMEOUT_SECS : PANIC_HOLD_SENTINEL;

    u16 n = krx_fs_read(api, (const u8*)"/proc/boot", 10u, (u8*)&g_st.snap, (u16)sizeof(g_st.snap));
    if (n < 260u) return;

    const BootSlot* live = p_latest_slot(api, &g_st.snap.rec);
    if (live) {
        for (u16 i = 0; i < 128u; i++) ((u8*)&g_st.slot)[i] = ((const u8*)live)[i];
        g_st.has_slot = 1u;
        if (g_st.snap.extra.valid == FAULT_EXTRA_IDENT) {
            g_st.extra_applies = 1u;
        }
        krx_leaf_call(api, LEAF_PREP, (u32)&g_st);
        if (g_st.slot.reset_reason == REASON_HARD) {
            krx_leaf_call(api, LEAF_CAUSE, (u32)&g_st);
            krx_leaf_call(api, LEAF_DISASM, (u32)&g_st);
        }
    }
}

static void do_report(const KrxApiTable* api, u8 dst) {
    if (!g_st.has_slot) return;
    g_st.report_dst = dst;
    krx_leaf_call(api, LEAF_REPORT, (u32)&g_st);
    show_toast(dst == REPORT_SERIAL ? "Serial Dump Sent" : "Saved /log/panic.txt");
}

static void do_reset_prefs(const KrxApiTable* api) {
    static const char PREFS[] =
        "/pref/system.cfg\0/pref/display.cfg\0"
        "/pref/audio.cfg\0/pref/keys.cfg\0/pref/battery.cfg";
    for (u8 i = 0; i < 5u; i++) {
        const char* pf = p_nth(PREFS, i);
        krx_fs_remove(api, (const u8*)pf, p_len(pf));
    }
    krx_fs_remove(api, (const u8*)"/proc/boot", 10u);
}

static void draw_toast_overlay(const KrxApiTable* api) {
    if (g_st.toast_ticks == 0u) return;
    krx_draw_rounded_box(api, 10u, 24u, 108u, 15u);
    krx_draw_str_align(
        api, 64, 34, (const u8*)g_st.toast_msg, p_len(g_st.toast_msg),
        FONT_TOMTHUMB, COLOR_FILL, ALIGN_CENTER
    );
}

static void draw_footer(const KrxApiTable* api, u8 has_pages) {
    krx_ui_draw_key_badge(api, 3, 55, (const u8*)"M", 1u, (const u8*)"Boot", 4u);
    if (has_pages) {
        krx_ui_draw_key_badge(api, 46, 55, (const u8*)"v", 1u, (const u8*)"Next", 4u);
        krx_ui_draw_key_badge(api, 88, 55, (const u8*)"S1", 2u, (const u8*)"Dump", 4u);
    }
}

static void render_current(const KrxApiTable* api) {
    if (g_st.mode == MODE_SAFE_MENU) {
        krx_leaf_call(api, LEAF_SAFE_MENU, (u32)&g_st);
    } else if (!g_st.has_slot) {
        krx_clear(api);
        p_draw_sq_tab(api, "NO FAULT");
        krx_draw_str_align(api, 64, 33, (const u8*)"System clean", 12u, FONT_TOMTHUMB, COLOR_FILL, ALIGN_CENTER);
        draw_footer(api, 0u);
    } else {
        if (g_st.page == PAGE_MAIN) {
            krx_leaf_call(api, LEAF_PAGE_MAIN, (u32)&g_st);
            krx_leaf_call(api, LEAF_PAGE_INFO, (u32)&g_st);
        } else if (g_st.page == PAGE_REGS || g_st.page == PAGE_EXT) {
            krx_clear(api);
            p_draw_sq_tab(api, g_st.page == PAGE_EXT ? "EXT REGS" : "REGS");
            char pg[4] = { (char)('1' + g_st.page), '/', (char)('0' + PAGE_COUNT), 0 };
            krx_draw_str_align(api, 125, 7, (const u8*)pg, 3u, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);
            krx_leaf_call(api, LEAF_PAGE_REGS, (u32)&g_st);
        } else if (g_st.page == PAGE_STACK) {
            krx_leaf_call(api, LEAF_PAGE_STACK, (u32)&g_st);
        } else {
            krx_leaf_call(api, LEAF_PAGE_ASM, (u32)&g_st);
        }
        draw_footer(api, 1u);
    }
    draw_toast_overlay(api);
}

// Returns 0 = ignored, 1 = redraw, 2 = continue boot / exit
static u8 handle_page_key(const KrxApiTable* api, u8 key) {
    if (key == KEY_MENU || key == KEY_EXIT) return 2u;
    if (!g_st.has_slot) return 0u;

    g_st.boot_rem = PANIC_HOLD_SENTINEL;
    if (key == KEY_DOWN || key == KEY_HASH) {
        g_st.page = (g_st.page + 1u >= PAGE_COUNT) ? 0u : (u8)(g_st.page + 1u);
        return 1u;
    }
    if (key == KEY_UP || key == KEY_STAR) {
        g_st.page = (g_st.page == 0u) ? (u8)(PAGE_COUNT - 1u) : (u8)(g_st.page - 1u);
        return 1u;
    }
    if (key >= KEY_1 && key <= KEY_5) {
        g_st.page = (u8)(key - KEY_1);
        return 1u;
    }
    if (key == KEY_SIDE1) { do_report(api, REPORT_SERIAL); return 1u; }
    if (key == KEY_SIDE2) { do_report(api, REPORT_FILE); return 1u; }
    if (key == KEY_0) {
        krx_fs_remove(api, (const u8*)"/proc/boot", 10u);
        krx_sys_reboot(api, 0u);
        return 2u;
    }
    return 1u;
}

static void run_boot_loop(const KrxApiTable* api) {
    krx_wake_backlight(api);
    const u8 orig_mode = g_st.mode;
    u32 start_ms = krx_ticks_ms(api);
    u8 prev_k = krx_poll_key(api);
    g_st.dirty = 1u;

    while (1) {
        u8 k = krx_poll_key(api);
        u8 edge = (k != 0u && prev_k == 0u) ? k : 0u;
        prev_k = k;

        u32 el = krx_ticks_ms(api) - start_ms;

        if (g_st.toast_ticks > 0u) {
            g_st.toast_ticks--;
            if (g_st.toast_ticks == 0u) g_st.dirty = 1u;
        }

        if (g_st.mode == MODE_SAFE_MENU) {
            if (el < 200u) edge = 0u;
            if (edge == KEY_UP) {
                g_st.menu_sel = (g_st.menu_sel == 0u) ? 3u : (u8)(g_st.menu_sel - 1u);
                g_st.dirty = 1u;
            } else if (edge == KEY_DOWN) {
                g_st.menu_sel = (u8)((g_st.menu_sel + 1u) & 3u);
                g_st.dirty = 1u;
            } else if (edge == KEY_SIDE1) {
                do_report(api, REPORT_SERIAL);
            } else if (edge == KEY_SIDE2) {
                do_report(api, REPORT_FILE);
            } else if (edge == KEY_EXIT) {
                return;
            } else if (edge == KEY_MENU) {
                if (g_st.menu_sel == 0u) {
                    krx_fs_remove(api, (const u8*)"/proc/boot", 10u);
                    return;
                } else if (g_st.menu_sel == 1u) {
                    g_st.mode = MODE_INSPECT;
                    g_st.page = PAGE_MAIN;
                    g_st.dirty = 1u;
                } else if (g_st.menu_sel == 2u) {
                    do_reset_prefs(api);
                    return;
                } else {
                    krx_fs_remove(api, (const u8*)"/proc/boot", 10u);
                    krx_sys_reboot(api, 0u);
                    return;
                }
            }
        } else {
            if (g_st.mode == MODE_BOOT_DIALOG) {
                if (el < 250u) edge = 0u;
                if (g_st.boot_rem != PANIC_HOLD_SENTINEL) {
                    if (el >= (u32)PANIC_TIMEOUT_SECS * 1000u) return;
                    u8 rem = PANIC_TIMEOUT_SECS;
                    u32 t = el;
                    while (t >= 1000u && rem > 0u) { t -= 1000u; rem--; }
                    if (rem != g_st.boot_rem) { g_st.boot_rem = rem; g_st.dirty = 1u; }
                }
            }
            if (edge != 0u) {
                if (edge == KEY_EXIT && orig_mode == MODE_SAFE_MENU) {
                    g_st.mode = MODE_SAFE_MENU;
                    g_st.dirty = 1u;
                } else {
                    u8 r = handle_page_key(api, edge);
                    if (r == 2u) return;
                    if (r == 1u) g_st.dirty = 1u;
                }
            }
        }

        if (g_st.dirty) {
            g_st.dirty = 0u;
            render_current(api);
            krx_commit_frame(api);
        }
        krx_delay_ms(api, 15u);
    }
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        u8 mode = (u8)(u32)param;
        init_panic_data(api, mode);
        if (mode == MODE_BOOT_DIALOG || mode == MODE_SAFE_MENU) {
            run_boot_loop(api);
        } else {
            __attribute__((aligned(4))) KrxStatusBar sb;
            for (u16 i = 0; i < (u16)sizeof(sb); i++) ((u8*)&sb)[i] = 0u;
            sb.tray_icon_id = KRX_ICON_NONE;
            krx_ui_set_status_bar(api, &sb);
        }
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        if (!param) return 0;
        const u8* ev = (const u8*)param;
        if (ev[0] == KRX_EV_TICK && g_st.toast_ticks > 0u) {
            g_st.toast_ticks--;
            return (g_st.toast_ticks == 0u) ? 1 : 0;
        }
        if (ev[0] == KRX_EV_ENCODER && g_st.has_slot) {
            i32 ed = *(const i32*)(ev + 4);
            if (ed > 0) g_st.page = (g_st.page + 1u >= PAGE_COUNT) ? 0u : (u8)(g_st.page + 1u);
            else if (ed < 0) g_st.page = (g_st.page == 0u) ? (u8)(PAGE_COUNT - 1u) : (u8)(g_st.page - 1u);
            return 1;
        }
        if (ev[0] == KRX_EV_KEY) {
            u8 key = ev[1];
            u32 state = *(const u32*)(ev + 4);
            if (state == 0u) {
                u8 r = handle_page_key(api, key);
                if (r == 2u) return 0;
            }
            return 1;
        }
        return 0;
    }

    if (op == KRX_OP_RENDER) {
        render_current(api);
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        krx_ui_set_status_bar(api, 0);
        return 0;
    }

    return 0;
}
