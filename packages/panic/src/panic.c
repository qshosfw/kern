/*
 * KERN Panic & Post-Mortem Forensics Module (/apps/panic.mod)
 *
 * P0 Structured Paged KRX Module (`KrxExecType::Module`, `FLAG_SYSTEM_MENU`).
 * Handles all heavy panic/HardFault UI, Safe Mode recovery menu,
 * crash history browsing, Thumb-16 disassembly, and Serial/KernFS reports:
 * - param == 0: In-OS 5-tab Crash Inspector (Settings -> System -> Crash Logs)
 * - param == 1: Boot Crash Alert Dialog (auto-dismiss or inspect/UART dump)
 * - param == 2: Boot Safe Mode Recovery Menu (Continue / Inspect / Reset / Reboot)
 */
#include "panic_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((visibility("default"), naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl krx_main\n pop {pc}\n");
}

static const char LOG_ITEMS[] =
    "Next Record\0"
    "Serial Dump\0"
    "Save /log/crash.txt\0"
    "Clear All Logs";

static PanicState g_st;

static void update_status_bar(const KrxApiTable* api) {
    if (g_st.mode != MODE_IN_OS) return;
    KrxStatusBar sb;
    for (u16 i = 0; i < (u16)sizeof(sb); i++) ((u8*)&sb)[i] = 0;
    sb.flags = KRX_STATUS_VISIBLE | KRX_STATUS_CUSTOM_TITLE | KRX_STATUS_DIVIDER;
    p_copy_cstr((char*)sb.title, "Crash Logs", 15);
    if (g_st.has_slot) {
        u8 total = g_st.total_logs > 0u ? g_st.total_logs : 1u;
        u8 p = p_put_uint(api, (char*)sb.badge, (u32)g_st.cur_idx + 1u);
        sb.badge[p++] = '/';
        p += p_put_uint(api, (char*)sb.badge + p, total);
        sb.badge[p] = 0;
    } else {
        p_copy_cstr((char*)sb.badge, "CLEAN", 7);
    }
    krx_ui_set_status_bar(api, &sb);
}

static void show_toast(const char* msg) {
    p_copy_cstr(g_st.toast_msg, msg, 23);
    g_st.toast_ticks = 40u;
}

static u8 slots_equal(const BootSlot* a, const BootSlot* b) {
    return (a->fault_pc == b->fault_pc &&
            a->reset_reason == b->reset_reason &&
            a->crash_count == b->crash_count &&
            a->panic_line == b->panic_line) ? 1u : 0u;
}

static void select_record(const KrxApiTable* api, u8 idx) {
    for (u16 i = 0; i < (u16)sizeof(g_st.cur_slot); i++) ((u8*)&g_st.cur_slot)[i] = 0;
    g_st.has_slot = 0u;
    g_st.extra_applies = 0u;
    g_st.scroll = 0u;

    if (g_st.total_logs > 0u) {
        if (idx >= g_st.total_logs) idx = 0u;
        g_st.cur_idx = idx;
        u32 file_idx = (u32)(g_st.total_logs - 1u - idx);
        u16 n = krx_fs_read_at(
            api, (const u8*)"/log/crash.bin", 14u, file_idx * 128u, (u8*)&g_st.cur_slot, 128u
        );
        if (n == 128u && p_slot_valid(api, &g_st.cur_slot)) {
            g_st.has_slot = 1u;
        }
    }

    const BootSlot* live = p_latest_slot(api, &g_st.snap.rec);
    if (!g_st.has_slot && live) {
        for (u16 i = 0; i < 128u; i++) ((u8*)&g_st.cur_slot)[i] = ((const u8*)live)[i];
        g_st.has_slot = 1u;
        g_st.cur_idx = 0u;
    }

    if (g_st.has_slot && live && g_st.snap.extra.valid == FAULT_EXTRA_IDENT && slots_equal(&g_st.cur_slot, live)) {
        g_st.extra_applies = 1u;
    }

    if (g_st.has_slot) {
        krx_leaf_call(api, LEAF_PREP, (u32)&g_st);
        if (g_st.cur_slot.reset_reason == REASON_HARD) {
            krx_leaf_call(api, LEAF_CAUSE, (u32)&g_st);
        }
    }
    update_status_bar(api);
}

static void init_panic_data(const KrxApiTable* api, u8 mode) {
    for (u16 i = 0; i < (u16)sizeof(g_st); i++) ((u8*)&g_st)[i] = 0;
    g_st.mode = mode;
    g_st.dirty = 1u;

    krx_fs_read(api, (const u8*)"/proc/boot", 10u, (u8*)&g_st.snap, (u16)sizeof(g_st.snap));
    u32 sz = krx_fs_size(api, (const u8*)"/log/crash.bin", 14u);
    u32 cnt = sz >> 7;
    g_st.total_logs = (u8)(cnt > 16u ? 16u : cnt);
    select_record(api, 0u);
}

static void do_report(const KrxApiTable* api, u8 kind) {
    if (!g_st.has_slot) return;
    g_st.leaf_arg = kind;
    krx_leaf_call(api, LEAF_RPT_HEAD, (u32)&g_st);
    krx_leaf_call(api, LEAF_RPT_TAIL, (u32)&g_st);
    show_toast(kind == REPORT_UART ? "Serial Dump Sent" : "Saved crash.txt");
    g_st.dirty = 1u;
}

static void do_clear_all(const KrxApiTable* api, u8 also_prefs) {
    if (also_prefs) {
        static const char PREFS[] =
            "/pref/system.cfg\0/pref/display.cfg\0"
            "/pref/audio.cfg\0/pref/keys.cfg\0/pref/battery.cfg";
        for (u8 i = 0; i < 5u; i++) {
            const char* pf = p_nth(PREFS, i);
            krx_fs_remove(api, (const u8*)pf, p_len(pf));
        }
    } else {
        krx_fs_remove(api, (const u8*)"/log/crash.bin", 14u);
    }
    krx_fs_remove(api, (const u8*)"/proc/boot", 10u);
}

static void render_tabs(const KrxApiTable* api) {
    krx_clear(api);

    if (g_st.tab == TAB_ASM) {
        krx_leaf_call(api, LEAF_DIS_LO, (u32)&g_st);
        krx_leaf_call(api, LEAF_DIS_MID, (u32)&g_st);
        krx_leaf_call(api, LEAF_DIS_HI, (u32)&g_st);
        krx_leaf_call(api, LEAF_ASM_DRAW, (u32)&g_st);
    } else if (g_st.tab == TAB_QR) {
        krx_leaf_call(api, LEAF_QR_ENCODE, (u32)&g_st);
        krx_leaf_call(api, LEAF_QR_MATRIX, (u32)&g_st);
        krx_leaf_call(api, LEAF_QR_DRAW,   (u32)&g_st);
    } else if (g_st.tab == TAB_LOG) {
        char val[16];
        for (u8 i = 0; i < 4u; i++) {
            const char* k = p_nth(LOG_ITEMS, i);
            u8 vlen = 0;
            if (i == 0u && g_st.total_logs > 0u) {
                vlen = p_put_uint(api, val, (u32)g_st.cur_idx + 1u);
                val[vlen++] = '/';
                vlen += p_put_uint(api, val + vlen, g_st.total_logs);
            }
            krx_ui_draw_menu_item(api, (i16)(13 + i * 12), (const u8*)k, p_len(k), (const u8*)val, vlen, i == g_st.menu_sel, 0u);
        }
    } else {
        krx_leaf_call(api, (u8)(LEAF_TAB_SUM + g_st.tab), (u32)&g_st);
    }

    if (!(g_st.tab == TAB_QR && g_st.qr_full)) {
        krx_leaf_call(api, LEAF_TAB_CHROME, (u32)&g_st);
    }

    if (g_st.toast_ticks > 0u) {
        krx_fill_rect(api, 12, 24, 104, 15, COLOR_FILL);
        krx_fill_rect(api, 13, 25, 102, 13, COLOR_CLEAR);
        krx_draw_str_align(api, 64, 29, (const u8*)g_st.toast_msg, p_len(g_st.toast_msg), FONT_TOMTHUMB, COLOR_FILL, ALIGN_CENTER);
    }
}

static void render_boot_screen(const KrxApiTable* api) {
    if (g_st.mode == MODE_BOOT_DIALOG) {
        krx_leaf_call(api, LEAF_SPLASH, (u32)&g_st);
        krx_leaf_call(api, LEAF_SPLASH_INFO, (u32)&g_st);
    } else {
        krx_leaf_call(api, LEAF_SAFE_MENU, (u32)&g_st);
    }
    krx_commit_frame(api);
}

static void next_record(const KrxApiTable* api) {
    if (g_st.total_logs == 0u) return;
    u8 nxt = (u8)(g_st.cur_idx + 1u);
    if (nxt >= g_st.total_logs) nxt = 0u;
    select_record(api, nxt);
}

// Returns 1 if state/UI needs redraw, 2 if caller should exit module cleanly
static u8 handle_tabs_key(const KrxApiTable* api, u8 key) {
    if (key == KEY_EXIT) {
        if (g_st.tab == TAB_QR && g_st.qr_full) {
            g_st.qr_full = 0u;
            return 1u;
        }
        return 2u;
    }
    if (key >= KEY_1 && key <= KEY_6) {
        g_st.tab = (u8)(key - KEY_1);
        g_st.scroll = 0u;
        g_st.qr_full = 0u;
        return 1u;
    }
    if (key == KEY_HASH) { g_st.tab = (g_st.tab + 1u >= TAB_COUNT) ? 0u : (u8)(g_st.tab + 1u); g_st.scroll = 0u; g_st.qr_full = 0u; return 1u; }
    if (key == KEY_STAR) { g_st.tab = (g_st.tab == 0u) ? (u8)(TAB_COUNT - 1u) : (u8)(g_st.tab - 1u); g_st.scroll = 0u; g_st.qr_full = 0u; return 1u; }
    if (key == KEY_SIDE1) { do_report(api, REPORT_UART); return 1u; }
    if (key == KEY_SIDE2 && g_st.total_logs > 1u) {
        next_record(api);
        return 1u;
    }

    if (g_st.tab == TAB_QR) {
        if (key == KEY_MENU) { g_st.qr_full ^= 1u; return 1u; }
        return 0u;
    }

    if (g_st.tab == TAB_LOG) {
        if (key == KEY_UP) { g_st.menu_sel = (g_st.menu_sel == 0u) ? 3u : (u8)(g_st.menu_sel - 1u); return 1u; }
        if (key == KEY_DOWN) { g_st.menu_sel = (u8)((g_st.menu_sel + 1u) & 3u); return 1u; }
        if (key == KEY_MENU) {
            if (g_st.menu_sel == 0u) {
                next_record(api);
            } else if (g_st.menu_sel == 1u) {
                do_report(api, REPORT_UART);
            } else if (g_st.menu_sel == 2u) {
                do_report(api, REPORT_FILE);
            } else if (g_st.menu_sel == 3u) {
                do_clear_all(api, 0u);
                g_st.total_logs = 0u;
                select_record(api, 0u);
                show_toast("Logs Cleared");
            }
            return 1u;
        }
    } else {
        if (key == KEY_UP && g_st.scroll > 0u) { g_st.scroll--; return 1u; }
        if (key == KEY_DOWN && g_st.scroll < 6u) { g_st.scroll++; return 1u; }
        if (key == KEY_MENU) { g_st.tab = (g_st.tab + 1u >= TAB_COUNT) ? 0u : (u8)(g_st.tab + 1u); g_st.scroll = 0u; return 1u; }
    }
    return 0u;
}

static void run_boot_loop(const KrxApiTable* api) {
    krx_wake_backlight(api);
    const u8 orig_mode = g_st.mode;
    u32 start_ms = krx_ticks_ms(api);
    u8 prev_k = krx_poll_key(api);
    g_st.boot_rem = 30u;
    g_st.boot_anim = 0u;
    g_st.dirty = 1u;

    while (1) {
        u8 k = krx_poll_key(api);
        u8 edge = (k != 0u && prev_k == 0u) ? k : 0u;
        prev_k = k;

        if (g_st.mode == MODE_BOOT_DIALOG) {
            u32 el = krx_ticks_ms(api) - start_ms;
            if (el < 300u) edge = 0u;

            if (g_st.boot_rem != 255u) {
                if (el >= 30000u) return;
                u8 rem = 30u;
                u32 t = el;
                while (t >= 1000u && rem > 0u) { t -= 1000u; rem--; }
                if (rem != g_st.boot_rem) { g_st.boot_rem = rem; g_st.dirty = 1u; }
            }
            u8 anim = (u8)((el >> 9) & 1u);
            if (anim != g_st.boot_anim) { g_st.boot_anim = anim; g_st.dirty = 1u; }

            if (edge != 0u) {
                g_st.boot_rem = 255u;
                if (edge == KEY_MENU || edge == KEY_EXIT) return;
                if (edge == KEY_SIDE1) { do_report(api, REPORT_UART); }
                else if (edge == KEY_STAR) {
                    g_st.mode = MODE_SAFE_INSPECT;
                    g_st.tab = TAB_QR;
                    g_st.dirty = 1u;
                } else if (edge == KEY_UP || edge == KEY_DOWN || edge == KEY_HASH || (edge >= KEY_1 && edge <= KEY_6)) {
                    g_st.mode = MODE_SAFE_INSPECT;
                    g_st.tab = (edge >= KEY_1 && edge <= KEY_6) ? (u8)(edge - KEY_1) : 0u;
                    g_st.dirty = 1u;
                }
            }
            if (g_st.dirty) { g_st.dirty = 0u; render_boot_screen(api); }
        } else if (g_st.mode == MODE_SAFE_MENU) {
            if (edge == KEY_UP) { g_st.menu_sel = (g_st.menu_sel == 0u) ? 4u : (u8)(g_st.menu_sel - 1u); g_st.dirty = 1u; }
            else if (edge == KEY_DOWN) { g_st.menu_sel = (g_st.menu_sel >= 4u) ? 0u : (u8)(g_st.menu_sel + 1u); g_st.dirty = 1u; }
            else if (edge == KEY_MENU) {
                if (g_st.menu_sel == 0u) {
                    krx_fs_remove(api, (const u8*)"/proc/boot", 10u);
                    return;
                }
                else if (g_st.menu_sel == 1u) { g_st.mode = MODE_SAFE_INSPECT; g_st.menu_sel = 0u; g_st.dirty = 1u; }
                else if (g_st.menu_sel == 2u) { do_report(api, REPORT_UART); }
                else if (g_st.menu_sel == 3u) { do_clear_all(api, 1u); return; }
                else if (g_st.menu_sel == 4u) {
                    krx_fs_remove(api, (const u8*)"/proc/boot", 10u);
                    krx_sys_reboot(api, 0u);
                    return;
                }
            } else if (edge == KEY_EXIT) {
                return;
            }
            if (g_st.dirty) { g_st.dirty = 0u; render_boot_screen(api); }
        } else {
            if (edge != 0u) {
                u8 r = handle_tabs_key(api, edge);
                if (r == 2u) {
                    if (orig_mode == MODE_SAFE_MENU) {
                        g_st.mode = MODE_SAFE_MENU;
                        g_st.dirty = 1u;
                    } else {
                        return;
                    }
                }
                if (r == 1u) g_st.dirty = 1u;
            }
            if (g_st.toast_ticks > 0u) {
                g_st.toast_ticks--;
                if (g_st.toast_ticks == 0u) g_st.dirty = 1u;
            }
            if (g_st.dirty) { g_st.dirty = 0u; render_tabs(api); krx_commit_frame(api); }
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
        if (ev[0] == KRX_EV_KEY) {
            u8 key = ev[1];
            u32 state = *(const u32*)(ev + 4);
            if (state == 0u || (state == 2u && (key == KEY_UP || key == KEY_DOWN))) {
                u8 r = handle_tabs_key(api, key);
                if (r == 2u) return 0;
                return (r == 1u) ? 1 : 0;
            }
            return (key == KEY_EXIT) ? 0 : 1;
        }
        return 0;
    }

    if (op == KRX_OP_RENDER) {
        render_tabs(api);
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        krx_ui_set_status_bar(api, 0);
        return 0;
    }

    return 0;
}
