/*
 * KERN System Information Module (/apps/sysinfo.krx)
 *
 * P0 Structured Paged KRX Module (`KrxExecType::Module`) using full KRX ABI
 * UI widgets (`ui_draw_capsule_button`, `ui_draw_menu_item`, `ui_draw_scrollbar`,
 * `ui_draw_tag_badge`, `ui_draw_battery`, `ui_draw_progress_bar`, `draw_bitmap`).
 *
 * 4 Tabbed Views (`INFO` / `HW` / `LIVE` / `REG`):
 * - `INFO`: Device & Firmware card (`KERN OS`, version, model badge, commit, build) + MCU/Boot/Uptime
 * - `HW`:   Interactive MenuView list (Serial, MAC, ID64, optional QS-1, UID1, UID2)
 * - `LIVE`: Live 1 Hz Power, Stack/RAM headroom, Flash & CPU/Thermal telemetry meters
 * - `REG`:  Clean FCC & CE conformity emblem card + Part 15 compliance notice
 */

#include "sysinfo_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl krx_main\n pop {pc}\n");
}

#define TAB_INFO     0
#define TAB_HW       1
#define TAB_LIVE     2
#define TAB_REG      3
#define TAB_COUNT    4
#define HW_VISIBLE   4

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

static const char STR_TAB_0[] = "INFO";
static const char STR_TAB_1[] = "HW";
static const char STR_TAB_2[] = "LIVE";
static const char STR_TAB_3[] = "REG";
static const char STR_SB[]    = "System Info";
static const char STR_KERN[]  = "KERN OS";
static const char STR_REV[]   = "REV ";
static const char STR_MCU[]   = "MCU";
static const char STR_BOOT[]  = "Boot";
static const char STR_UP[]    = "Uptime";

static const char STR_HW_0[]  = "Serial";
static const char STR_HW_1[]  = "MAC";
static const char STR_HW_2[]  = "ID64";
static const char STR_HW_3[]  = "QS-1";
static const char STR_HW_4[]  = "UID1";
static const char STR_HW_5[]  = "UID2";

static const char STR_FCC_1[] = "47 CFR";
static const char STR_FCC_2[] = "15B / 97";
static const char STR_CE_1[]  = "CE RED";
static const char STR_CE_2[]  = "2014/53/EU";
static const char STR_REG_1[] = "Complies with Part 15 FCC Rules.";
static const char STR_REG_2[] = "(1) May not cause interference;";
static const char STR_REG_3[] = "(2) Must accept any received.";

static const char STR_STK[]   = "STK";
static const char STR_RAM[]   = "RAM";
static const char STR_FL[]    = "FLASH";

static u8           g_tab;
static u8           g_hw_sel;
static u8           g_hw_scroll;
static u32          g_last_sec;
static SysInfoState g_state;

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
    si_copy(sb.title, (const u8*)STR_SB, 11);
    api->ui->ui_set_status_bar(&sb);
}

static void draw_round_card(const CapDisplayVTable* d, i16 x, i16 y, i16 w, i16 h) {
    d->draw_rect(x, y, w, h, COLOR_FILL);
    d->clear_corners(x, y, w, h);
}

static void fmt_uptime(const KrxApiTable* api, u32 secs, char* out) {
    u32 s = 0, mins = si_udivmod(api, secs, 60u, &s);
    u32 m = 0, h = si_udivmod(api, mins > 5999u ? 5999u : mins, 60u, &m);
    si_put_2d(api, out + 0, h);
    out[2] = ':';
    si_put_2d(api, out + 3, m);
    out[5] = ':';
    si_put_2d(api, out + 6, s);
    out[8] = 0;
}

static void render_tabs(const CapUiVTable* ui) {
    if (!ui || !ui->ui_draw_capsule_button) return;
    const char* labels[TAB_COUNT] = { STR_TAB_0, STR_TAB_1, STR_TAB_2, STR_TAB_3 };
    const i16 xs[TAB_COUNT]       = { 2, 33, 64, 95 };
    const u8  ws[TAB_COUNT]       = { 29, 29, 29, 31 };
    for (u8 i = 0; i < TAB_COUNT; i++) {
        ui->ui_draw_capsule_button(xs[i], 9, ws[i], 9, (const u8*)labels[i], si_len(labels[i]), g_tab == i);
    }
}

static void render_tab_info(const KrxApiTable* api) {
    const CapDisplayVTable* d = api->display;
    const CapUiVTable* ui = api->ui;

    draw_round_card(d, 2, 20, 124, 17);
    d->draw_str(6, 27, (const u8*)STR_KERN, 7, 1, COLOR_FILL);
    d->draw_str(50, 27, (const u8*)g_state.ver_short, si_len(g_state.ver_short), 0, COLOR_FILL);

    u8 mlen = si_len(g_state.model);
    if (ui && ui->ui_draw_tag_badge && mlen > 0) {
        i16 tw = d->str_width ? d->str_width((const u8*)g_state.model, mlen, 0) : (i16)(mlen * 4);
        i16 bx = 123 - (tw + 6);
        ui->ui_draw_tag_badge(bx, 21, (const u8*)g_state.model, mlen, 1);
    }

    char rev[14];
    si_copy((u8*)rev, (const u8*)STR_REV, 4);
    u8 ghl = si_len(g_state.git_hash);
    si_copy((u8*)(rev + 4), (const u8*)g_state.git_hash, ghl);
    rev[4 + ghl] = 0;
    d->draw_str(6, 34, (const u8*)rev, (u16)(4 + ghl), 0, COLOR_FILL);
    d->draw_str_align(122, 34, (const u8*)g_state.build, si_len(g_state.build), 0, COLOR_FILL, 2);

    char up_buf[9];
    u32 secs = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0;
    fmt_uptime(api, secs, up_buf);

    const char* keys[3] = { STR_MCU, STR_BOOT, STR_UP };
    const char* vals[3] = { g_state.mcu, g_state.boot, up_buf };
    for (u8 i = 0; i < 3; i++) {
        i16 y = (i16)(46 + i * 8);
        d->draw_str(4, y, (const u8*)keys[i], si_len(keys[i]), 1, COLOR_FILL);
        d->draw_str_align(124, y, (const u8*)vals[i], si_len(vals[i]), 1, COLOR_FILL, 2);
    }
}

static void render_tab_hw(const KrxApiTable* api) {
    const CapUiVTable* ui = api->ui;
    if (!ui || !ui->ui_draw_menu_item) return;

    u8 total = (u8)(5 + g_state.has_qs1);
    const char* titles[6];
    const char* vals[6];
    u8 n = 0;
    titles[n] = STR_HW_0; vals[n++] = g_state.sn;
    titles[n] = STR_HW_1; vals[n++] = g_state.mac;
    titles[n] = STR_HW_2; vals[n++] = g_state.hex_id64;
    if (g_state.has_qs1) {
        titles[n] = STR_HW_3; vals[n++] = g_state.qs1;
    }
    titles[n] = STR_HW_4; vals[n++] = g_state.hex_uid_hi;
    titles[n] = STR_HW_5; vals[n++] = g_state.hex_uid_lo;

    for (u8 v = 0; v < HW_VISIBLE; v++) {
        u8 idx = g_hw_scroll + v;
        if (idx >= total) break;
        i16 y = (i16)(20 + v * 11);
        ui->ui_draw_menu_item(
            y,
            (const u8*)titles[idx], si_len(titles[idx]),
            (const u8*)vals[idx], si_len(vals[idx]),
            idx == g_hw_sel,
            0
        );
    }
    if (ui->ui_draw_scrollbar) {
        ui->ui_draw_scrollbar(124, 20, 43, g_hw_scroll, total, HW_VISIBLE);
    }
}

static void render_tab_live(const KrxApiTable* api) {
    const CapDisplayVTable* d = api->display;
    const CapUiVTable* ui = api->ui;
    char buf[16];

    u16 mv = (api->power && api->power->sys_battery_mv) ? api->power->sys_battery_mv() : 0;
    u8 pct = (api->power && api->power->sys_battery_pct) ? api->power->sys_battery_pct() : 0;
    i16 temp_c = (api->power && api->power->sys_mcu_temp) ? api->power->sys_mcu_temp() : 0;
    u32 mhz = (api->power && api->power->sys_cpu_freq_hz)
        ? si_udivmod(api, api->power->sys_cpu_freq_hz(), 1000000u, 0) : 48u;
    u16 stk_free = (api->service && api->service->sys_stack_margin)
        ? api->service->sys_stack_margin() : 0;
    u16 ram_free = (api->service && api->service->sys_ram_available)
        ? api->service->sys_ram_available() : 0;

    // 1. Battery voltage + icon + progress bar
    if (ui && ui->ui_draw_battery) ui->ui_draw_battery(4, 22, pct, 0);
    u32 rem = 0, volts = si_udivmod(api, mv, 1000u, &rem);
    u32 centi = si_udivmod(api, rem, 10u, 0);
    u8 p = si_put_uint(api, buf, volts);
    buf[p++] = '.';
    si_put_2d(api, buf + p, centi);
    p += 2;
    buf[p++] = 'V';
    buf[p] = 0;
    d->draw_str(19, 28, (const u8*)buf, p, 1, COLOR_FILL);
    if (ui && ui->ui_draw_progress_bar) ui->ui_draw_progress_bar(56, 21, 48, 7, pct, 1);
    p = si_put_uint(api, buf, pct);
    buf[p++] = '%';
    buf[p] = 0;
    d->draw_str_align(125, 27, (const u8*)buf, p, 0, COLOR_FILL, 2);

    // 2. Stack margin bar
    d->draw_str(4, 38, (const u8*)STR_STK, 3, 1, COLOR_FILL);
    u32 stk_pct = si_udivmod(api, (u32)stk_free * 100u, 2560u, 0);
    if (stk_pct > 100u) stk_pct = 100u;
    if (ui && ui->ui_draw_progress_bar) ui->ui_draw_progress_bar(29, 32, 62, 6, (u8)stk_pct, 1);
    p = si_put_uint(api, buf, stk_free);
    buf[p++] = 'B';
    buf[p] = 0;
    d->draw_str_align(125, 38, (const u8*)buf, p, 0, COLOR_FILL, 2);

    // 3. RAM free / Flash capacity row
    d->draw_str(4, 48, (const u8*)STR_RAM, 3, 1, COLOR_FILL);
    u32 ram_pct = si_udivmod(api, (u32)ram_free * 100u, 4096u, 0);
    if (ram_pct > 100u) ram_pct = 100u;
    if (ui && ui->ui_draw_progress_bar) ui->ui_draw_progress_bar(29, 42, 62, 6, (u8)ram_pct, 1);
    p = si_put_uint(api, buf, ram_free);
    buf[p++] = 'B';
    buf[p] = 0;
    d->draw_str_align(125, 48, (const u8*)buf, p, 0, COLOR_FILL, 2);

    // 4. CPU frequency, Flash & Temperature footer
    d->draw_line_h(2, 51, 124, COLOR_FILL);
    p = si_put_uint(api, buf, mhz);
    buf[p++] = ' '; buf[p++] = 'M'; buf[p++] = 'H'; buf[p++] = 'z';
    buf[p] = 0;
    d->draw_str(4, 61, (const u8*)buf, p, 1, COLOR_FILL);

    p = si_put_uint(api, buf, g_state.flash_kb);
    buf[p++] = 'K'; buf[p++] = ' ';
    si_copy((u8*)(buf + p), (const u8*)STR_FL, 5);
    p += 5;
    d->draw_str_align(66, 61, (const u8*)buf, p, 0, COLOR_FILL, 1);

    p = 0;
    if (temp_c < 0) { buf[p++] = '-'; temp_c = (i16)(-temp_c); }
    p += si_put_uint(api, buf + p, (u32)(u16)temp_c);
    buf[p++] = ' '; buf[p++] = 'C'; buf[p] = 0;
    d->draw_str_align(125, 61, (const u8*)buf, p, 1, COLOR_FILL, 2);
}

static void render_tab_reg(const KrxApiTable* api) {
    const CapDisplayVTable* d = api->display;

    draw_round_card(d, 2, 20, 124, 17);
    d->draw_bitmap(6, 23, 15, 11, FCC_BMP, sizeof(FCC_BMP));
    d->draw_str(24, 27, (const u8*)STR_FCC_1, si_len(STR_FCC_1), 0, COLOR_FILL);
    d->draw_str(24, 34, (const u8*)STR_FCC_2, si_len(STR_FCC_2), 0, COLOR_FILL);

    d->draw_line_v(63, 22, 13, COLOR_FILL);

    d->draw_bitmap(67, 23, 14, 10, CE_BMP, sizeof(CE_BMP));
    d->draw_str(84, 27, (const u8*)STR_CE_1, si_len(STR_CE_1), 0, COLOR_FILL);
    d->draw_str(84, 34, (const u8*)STR_CE_2, si_len(STR_CE_2), 0, COLOR_FILL);

    d->draw_line_v(3, 40, 21, COLOR_FILL);
    d->draw_str(7, 45, (const u8*)STR_REG_1, si_len(STR_REG_1), 0, COLOR_FILL);
    d->draw_str(7, 53, (const u8*)STR_REG_2, si_len(STR_REG_2), 0, COLOR_FILL);
    d->draw_str(7, 61, (const u8*)STR_REG_3, si_len(STR_REG_3), 0, COLOR_FILL);
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_tab = TAB_INFO;
        g_hw_sel = 0;
        g_hw_scroll = 0;
        g_last_sec = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0;
        si_zero((u8*)&g_state, sizeof(g_state));
        g_state.api = api;
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

        if (kind == KRX_EV_TICK) {
            u32 now = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0;
            if (now != g_last_sec) {
                g_last_sec = now;
                return (g_tab == TAB_INFO || g_tab == TAB_LIVE) ? 1 : 0;
            }
            return 0;
        }

        u8 total_hw = (u8)(5 + g_state.has_qs1);
        i8 dir = 0;

        if (kind == KRX_EV_KEY) {
            u8 key = ev[1];
            u32 state = *(const u32*)(ev + 4);
            if (key == KEY_EXIT && state == 0) {
                return 0; // Unhandled -> orchestrator pops back to Settings
            }
            if (state == 0) {
                if (key == KEY_1) { g_tab = TAB_INFO; return 1; }
                if (key == KEY_2) { g_tab = TAB_HW; return 1; }
                if (key == KEY_3) { g_tab = TAB_LIVE; return 1; }
                if (key == KEY_4) { g_tab = TAB_REG; return 1; }
                if (key == KEY_HASH || key == KEY_MENU) {
                    g_tab = (u8)((g_tab + 1) % TAB_COUNT);
                    return 1;
                }
                if (key == KEY_STAR) {
                    g_tab = (u8)((g_tab + TAB_COUNT - 1) % TAB_COUNT);
                    return 1;
                }
                if (key == KEY_F && api->service && api->service->leaf_call) {
                    api->service->leaf_call(3, (u32)api);
                    return 1;
                }
            }
            if (state == 0 || state == 2) {
                if (key == KEY_UP) dir = -1;
                else if (key == KEY_DOWN) dir = 1;
            }
        } else if (kind == KRX_EV_ENCODER) {
            i32 ed = *(const i32*)(ev + 4);
            if (ed < 0) dir = -1;
            else if (ed > 0) dir = 1;
        }

        if (dir != 0 && g_tab == TAB_HW) {
            if (dir < 0 && g_hw_sel > 0) g_hw_sel--;
            else if (dir > 0 && (u8)(g_hw_sel + 1) < total_hw) g_hw_sel++;
            if (g_hw_sel < g_hw_scroll) g_hw_scroll = g_hw_sel;
            if (g_hw_sel >= g_hw_scroll + HW_VISIBLE) {
                g_hw_scroll = (u8)(g_hw_sel - HW_VISIBLE + 1);
            }
            return 1;
        }
        return (kind == KRX_EV_KEY);
    }

    if (op == KRX_OP_RENDER) {
        if (!api->display) return 0;
        api->display->clear();
        render_tabs(api->ui);
        if (g_tab == TAB_INFO)      render_tab_info(api);
        else if (g_tab == TAB_HW)   render_tab_hw(api);
        else if (g_tab == TAB_LIVE) render_tab_live(api);
        else                        render_tab_reg(api);
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        set_sb(api, 0);
        return 0;
    }

    return 0;
}
