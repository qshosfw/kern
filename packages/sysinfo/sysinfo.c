/*
 * KERN System Information & Regulatory Module (/apps/sysinfo.krx)
 *
 * P0 Structured Paged KRX Module (KrxExecType::Module):
 * - Hidden from Launcher; exposed in Settings -> System when installed,
 *   CRC-valid, and capability-authorized.
 * - Resident Root renders the UI using native KRX ABI widgets (tag badge,
 *   proportional scrollbar, horizontal/vertical lines, FCC/CE bitmaps, status bar)
 *   while offloading heavy crypto/ROM/date initialization and live MCU/Power
 *   popup formatting into 1 KiB P0 Leaf overlays (`leaf1`, `leaf2`, `leaf3`).
 */

#include "sysinfo_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl krx_main\n pop {pc}\n");
}

#define VISIBLE_ROWS  7
#define BASE_ROWS    20

// 15x11 MSB-first row-major FCC monogram bitmap (22B)
static const u8 FCC_BMP[22] = {
    0xF8, 0xFC, 0xF9, 0xFE, 0xC3, 0x9E, 0xF3, 0x3C, 0xF3, 0x30, 0xC3, 0x30,
    0xC3, 0x30, 0xC3, 0x3C, 0xC3, 0x9E, 0xC1, 0xFE, 0xC0, 0xFC
};

// 14x10 MSB-first row-major CE conformity bitmap (20B)
static const u8 CE_BMP[20] = {
    0x3C, 0x7C, 0x7C, 0xFC, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xF8, 0xC0, 0xF8,
    0xC0, 0xC0, 0xC0, 0xC0, 0x7C, 0xFC, 0x3C, 0x7C
};

// Flat NUL-delimited strings (zero .rodata pointer relocations under -fPIC)
static const char STRINGS[] =
    "SYSTEM\0"                          // 0
    "HARDWARE ID\0"                     // 1
    "REGULATORY\0"                      // 2
    "47 CFR PART 15B / 97\0"            // 3
    "CE  RED 2014/53/EU\0"              // 4
    "MCU\0"                             // 5
    "BOOT\0"                            // 6
    "BUILD\0"                           // 7
    "PWR\0"                             // 8
    "UPTIME\0"                          // 9
    "S/N\0"                             // 10
    "MAC\0"                             // 11
    "QS-1\0"                            // 12
    "This device complies with\0"       // 13
    "Part 15 of the FCC Rules.\0"       // 14
    "Operation is subject to two\0"     // 15
    "conditions: (1) must not\0"        // 16
    "cause harmful interference,\0"     // 17
    "(2) must accept any received.\0"   // 18
    "Live MCU & Power stats\0"          // 19
    "KERN\0"                            // 20
    "MENU\0"                            // 21
    "System Info\0";                    // 22

static u8           g_scroll;
static u32          g_last_sec;
static SysInfoState g_state;

static u8 c_len(const char* s) {
    u8 n = 0;
    while (s[n]) n++;
    return n;
}

static const char* nth_str(u8 idx) {
    const char* p = STRINGS;
    while (idx > 0) {
        while (*p) p++;
        p++;
        idx--;
    }
    return p;
}

static void format_uptime(const KrxApiTable* api, u32 secs, char* out) {
    u32 s = 0, mins = si_udivmod(api, secs, 60u, &s);
    u32 m = 0, h = si_udivmod(api, mins > 5999u ? 5999u : mins, 60u, &m);
    si_put_2d(api, out + 0, h);
    out[2] = ':';
    si_put_2d(api, out + 3, m);
    out[5] = ':';
    si_put_2d(api, out + 6, s);
    out[8] = 0;
}

static void format_power(const KrxApiTable* api, char* out) {
    u16 mv = (api->power && api->power->sys_battery_mv) ? api->power->sys_battery_mv() : 0;
    u8 pct = (api->power && api->power->sys_battery_pct) ? api->power->sys_battery_pct() : 0;
    u32 rem = 0;
    u32 volts = si_udivmod(api, mv, 1000u, &rem);
    u32 centi = si_udivmod(api, rem, 10u, 0);
    u8 p = si_put_uint(api, out, volts);
    out[p++] = '.';
    si_put_2d(api, out + p, centi);
    p += 2;
    out[p++] = 'V';
    out[p++] = ' ';
    p += si_put_uint(api, out + p, pct);
    out[p++] = '%';
    out[p] = 0;
}

static void set_sb(const KrxApiTable* api, u8 enable) {
    if (!api->ui || !api->ui->ui_set_status_bar) return;
    if (!enable) {
        api->ui->ui_set_status_bar(0);
        return;
    }
    KrxStatusBar sb;
    si_zero((u8*)&sb, sizeof(sb));
    sb.flags = KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER | KRX_STATUS_CUSTOM_TITLE;
    sb.tray_icon_id = KRX_ICON_NONE;
    si_copy(sb.title, (const u8*)nth_str(22), 11);
    api->ui->ui_set_status_bar(&sb);
}

static void draw_clipped_bmp(
    const CapDisplayVTable* d,
    i16 x,
    i16 y,
    u8 w,
    u8 h,
    const u8* bmp
) {
    i16 skip = (y < 8) ? (8 - y) : 0;
    i16 end_y = y + (i16)h;
    if (end_y > 64) end_y = 64;
    i16 vis_h = end_y - (y + skip);
    if (vis_h > 0) {
        d->draw_bitmap(x, (i16)(y + skip), w, (u8)vis_h, bmp + skip * 2, COLOR_FILL);
    }
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_scroll = 0;
        g_last_sec = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0;
        si_zero((u8*)&g_state, sizeof(g_state));
        g_state.api = api;
        // Leaf 1 populates crypto ID + QS-1, then chains via trampoline to Leaf 2
        if (api->service && api->service->leaf_call) {
            api->service->leaf_call(1, (u32)&g_state);
        }
        set_sb(api, 1);
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        if (!param) return 0;
        const u8* ev = (const u8*)param;
        u8 kind = ev[0];
        u8 max_scroll = (u8)((BASE_ROWS + g_state.has_qs1) - VISIBLE_ROWS);

        if (kind == KRX_EV_TICK) {
            u32 now = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0;
            if (now != g_last_sec) {
                g_last_sec = now;
                return 1;
            }
            return 0;
        }

        i8 dir = 0;
        if (kind == KRX_EV_KEY) {
            u8 key = ev[1];
            u32 state = *(const u32*)(ev + 4);
            if (key == KEY_EXIT && state == 0) {
                return 0; // Unhandled -> orchestrator pops back to Settings
            }
            if (key == KEY_MENU && state == 0 && api->service && api->service->leaf_call) {
                api->service->leaf_call(3, (u32)api);
                return 1;
            }
            if (state == 0 || state == 2) {
                if (key == KEY_UP) dir = -1;
                else if (key == KEY_DOWN) dir = 1;
                else if (key == KEY_2) { g_scroll = 0; return 1; }
                else if (key == KEY_8) { g_scroll = max_scroll; return 1; }
            }
        } else if (kind == KRX_EV_ENCODER) {
            i32 ed = *(const i32*)(ev + 4);
            if (ed < 0) dir = -1;
            else if (ed > 0) dir = 1;
        }

        if (dir < 0) {
            if (g_scroll > 0) g_scroll--;
            return 1;
        }
        if (dir > 0) {
            if (g_scroll < max_scroll) g_scroll++;
            return 1;
        }
        return (kind == KRX_EV_KEY);
    }

    if (op == KRX_OP_RENDER) {
        const CapDisplayVTable* d = api->display;
        const CapUiVTable* ui = api->ui;
        if (!d) return 0;
        d->clear();

        u8 total_rows = (u8)(BASE_ROWS + g_state.has_qs1);
        u8 max_scroll = total_rows - VISIBLE_ROWS;
        u8 scroll = (g_scroll > max_scroll) ? max_scroll : g_scroll;

        // Regulatory FCC & CE emblems flanking regulatory rows (`norm == 11..12`)
        i16 badge_top = 14 + (i16)((11 + g_state.has_qs1) - (i16)scroll) * 8 - 4;
        if (badge_top < 64 && badge_top + 11 > 8) {
            i16 vy0 = (badge_top < 8) ? 8 : badge_top;
            i16 vy1 = (badge_top + 11 > 64) ? 64 : (badge_top + 11);
            if (vy1 > vy0) {
                d->draw_line_v(19, vy0, vy1 - vy0, COLOR_FILL);
                d->draw_line_v(105, vy0, vy1 - vy0, COLOR_FILL);
            }
            draw_clipped_bmp(d, 2, badge_top, 15, 11, FCC_BMP);
            draw_clipped_bmp(d, 108, badge_top, 14, 10, CE_BMP);
        }

        char dyn_buf[14];
        for (u8 vis = 0; vis < VISIBLE_ROWS; vis++) {
            u8 row = scroll + vis;
            if (row >= total_rows) break;
            u8 y = (u8)(14 + vis * 8);
            u8 left_x = 2;
            const char* left = 0;
            const char* right = 0;
            u8 align = 0;

            if (g_state.has_qs1 && row == 10) {
                left = nth_str(12);
                right = g_state.qs1;
            } else {
                u8 norm = (g_state.has_qs1 && row > 10) ? (row - 1) : row;
                if (norm == 0) {
                    if (ui && ui->ui_draw_tag_badge) {
                        ui->ui_draw_tag_badge(2, 8, (const u8*)nth_str(20), 4, 1);
                    }
                    d->draw_str(27, 14, (const u8*)g_state.fw_badge, c_len(g_state.fw_badge), 0, COLOR_FILL);
                    d->draw_str_align(122, 14, (const u8*)g_state.model, c_len(g_state.model), 0, COLOR_FILL, 2);
                    d->draw_line_h(2, 16, 120, COLOR_FILL);
                } else if (norm == 1 || norm == 7 || norm == 10) {
                    d->fill_rect(2, (i16)y - 5, 2, 5, COLOR_FILL);
                    d->draw_line_h(2, (i16)y + 1, 120, COLOR_FILL);
                    left_x = 6;
                    left = nth_str((norm == 1) ? 0 : (norm == 7 ? 1 : 2));
                } else if (norm == 11 || norm == 12) {
                    left_x = 62;
                    align = 1;
                    left = nth_str(norm - 8);
                } else if (norm >= 13 && norm <= 18) {
                    d->draw_line_v(2, (i16)y - 5, 6, COLOR_FILL);
                    left_x = 4;
                    left = nth_str(norm);
                } else if (norm == 19) {
                    if (ui && ui->ui_draw_key_badge) {
                        ui->ui_draw_key_badge(2, (i16)y - 5, (const u8*)nth_str(21), 4,
                            (const u8*)nth_str(19), c_len(nth_str(19)));
                    }
                } else {
                    if (norm == 2)      { left = nth_str(5);  right = g_state.mcu; }
                    else if (norm == 3) { left = nth_str(6);  right = g_state.boot; }
                    else if (norm == 4) { left = nth_str(7);  right = g_state.build; }
                    else if (norm == 5) {
                        format_power(api, dyn_buf);
                        left = nth_str(8);
                        right = dyn_buf;
                    } else if (norm == 6) {
                        u32 secs = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0;
                        format_uptime(api, secs, dyn_buf);
                        left = nth_str(9);
                        right = dyn_buf;
                    }
                    else if (norm == 8) { left = nth_str(10); right = g_state.sn; }
                    else                { left = nth_str(11); right = g_state.mac; }
                }
            }

            if (left) {
                d->draw_str_align(left_x, y, (const u8*)left, c_len(left), 0, COLOR_FILL, align);
            }
            if (right) {
                d->draw_str_align(122, y, (const u8*)right, c_len(right), 0, COLOR_FILL, 2);
            }
        }

        if (ui && ui->ui_draw_scrollbar) {
            ui->ui_draw_scrollbar(124, 9, 54, scroll, total_rows, VISIBLE_ROWS);
        }
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        set_sb(api, 0);
        return 0;
    }

    return 0;
}
