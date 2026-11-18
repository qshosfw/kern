/*
 * KERN System Information Module (/apps/sysinfo.krx)
 *
 * P0 Structured Paged KRX Module (`KrxExecType::Module`) using shared `gfx`
 * widgets (`TabbedSheet` via `krx_draw_tabbed_sheet`, `ui_draw_menu_item`,
 * `ui_draw_scrollbar`) and typed `ServiceApi::sys_info(&KrxSysInfo)`.
 *
 * 4 Standard Tabs (`System` / `Device` / `Status` / `Legal`):
 * - `System`: Firmware version, Build Date, Commit (branch-hash), Bootloader, Calib, CPU Clock
 * - `Device`: Model, MCU, Memory, Serial, MAC, condensed Chip ID, validated QS-1 (✓/✗)
 * - `Status`: Battery, Batt Pack, MCU Temp (°C), Uptime, Stack Free (now / min)
 * - `Legal`:  Flash-paged FCC / CE RED compliance marks & statements (Leaf 1)
 */

#include "sysinfo_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl krx_main\n pop {pc}\n");
}

#define TAB_SYSTEM   0
#define TAB_DEVICE   1
#define TAB_STATUS   2
#define TAB_LEGAL    3
#define TAB_COUNT    4
#define LIST_VISIBLE 4

static const char TAB_TITLES[] = "System\0Device\0Status\0Legal";
static const char SYSTEM_KEYS[] =
    "Firmware\0Build Date\0Commit\0Bootloader\0CPU Clock\0OS ABI";
static const char DEVICE_KEYS[] =
    "Model\0MCU\0Memory\0Serial\0MAC\0Chip ID\0QS-1";
static const char STATUS_KEYS[] =
    "Battery\0Batt Pack\0Batt ADC\0MCU Temp\0Uptime\0Stack Free";
static const char LEGAL_KEYS[] =
    "FCC\0CE RED\0SAR\0Limits";
static const char LEGAL_VALS[] =
    "Part 15B / 97\0" "2014/53/EU\0" "25mm / Belt Clip\0" "0-40 C <2000m";
static const char BATT_PACKS[] =
    "1600mAh\0" "2200mAh\0" "3500mAh\0" "K1 1500mAh\0" "K1 2500mAh\0" "External";

static const char S_VALID[]    = "Valid";
static const char S_NONE[]     = "(none)";
static const char S_EMPTY[]    = "(empty)";
static const char S_INVALID[]  = "(invalid)";
static const char S_UNK[]      = "-";

/* 5x5 monochrome glyphs (top 5 bits MSB-first per row) */
static const u8 GLYPH_CHECK[5] = { 0x08, 0x10, 0xA0, 0x40, 0x00 };
static const u8 GLYPH_CROSS[5] = { 0x88, 0x50, 0x20, 0x50, 0x88 };

static u8         g_tab;
static u8         g_sel[TAB_COUNT];
static u8         g_scroll[TAB_COUNT];
static u32        g_last_sec;
static KrxSysInfo g_info;
static char       g_mac_str[18];
static char       g_chip_id[11];

static void set_sb(const KrxApiTable* api, u8 enable) {
    if (!api->ui || !api->ui->ui_set_status_bar) return;
    if (!enable) {
        api->ui->ui_set_status_bar(0);
        return;
    }
    KrxStatusBar sb;
    for (u8 i = 0; i < sizeof(sb); i++) ((u8*)&sb)[i] = 0;
    sb.flags = KRX_STATUS_VISIBLE | KRX_STATUS_PARTIAL;
    sb.tray_icon_id = KRX_ICON_NONE;
    api->ui->ui_set_status_bar(&sb);
}

static u8 tab_item_count(u8 tab) {
    if (tab == TAB_SYSTEM) return 6u;
    if (tab == TAB_DEVICE) return 7u;
    if (tab == TAB_STATUS) return 6u;
    return 4u;
}

static void scroll_tab(u8 tab, i8 dir) {
    u8 total = tab_item_count(tab);
    u8 vis = (tab == TAB_LEGAL) ? 1u : LIST_VISIBLE;
    u8 sel = g_sel[tab];
    if (dir < 0 && sel > 0u) sel--;
    else if (dir > 0 && (u8)(sel + 1u) < total) sel++;
    g_sel[tab] = sel;

    u8 sc = g_scroll[tab];
    if (sel < sc) sc = sel;
    if (sel >= (u8)(sc + vis)) sc = (u8)(sel - vis + 1u);
    g_scroll[tab] = sc;
}

static u8 put_str(char* dst, const char* src, u8 max_n) {
    u8 n = 0;
    while (n < max_n && src[n]) {
        dst[n] = src[n];
        n++;
    }
    dst[n] = 0;
    return n;
}

static void draw_glyph_5x5(const CapDisplayVTable* d, i16 x, i16 y, const u8* rows, u8 color) {
    if (!d || !d->draw_pixel) return;
    for (u8 r = 0; r < 5u; r++) {
        u8 bits = rows[r];
        for (u8 c = 0; c < 5u; c++) {
            if (bits & (0x80u >> c)) {
                d->draw_pixel((i16)(x + c), (i16)(y + r), color);
            }
        }
    }
}

static void format_static_fields(void) {
    for (u8 i = 0; i < 6u; i++) {
        u8 b = g_info.mac[i];
        g_mac_str[i * 3 + 0] = si_hex_nib(b >> 4);
        g_mac_str[i * 3 + 1] = si_hex_nib(b & 0x0Fu);
        g_mac_str[i * 3 + 2] = (i < 5u) ? ':' : 0;
    }
    /* Condensed 8-hex Chip ID (first 2 bytes + ".." + last 2 bytes of 96-bit UID) */
    g_chip_id[0] = si_hex_nib(g_info.mcu_uid[0] >> 4);
    g_chip_id[1] = si_hex_nib(g_info.mcu_uid[0] & 0x0Fu);
    g_chip_id[2] = si_hex_nib(g_info.mcu_uid[1] >> 4);
    g_chip_id[3] = si_hex_nib(g_info.mcu_uid[1] & 0x0Fu);
    g_chip_id[4] = '.';
    g_chip_id[5] = '.';
    g_chip_id[6] = si_hex_nib(g_info.mcu_uid[10] >> 4);
    g_chip_id[7] = si_hex_nib(g_info.mcu_uid[10] & 0x0Fu);
    g_chip_id[8] = si_hex_nib(g_info.mcu_uid[11] >> 4);
    g_chip_id[9] = si_hex_nib(g_info.mcu_uid[11] & 0x0Fu);
    g_chip_id[10] = 0;
}

static const char* format_system_val(const KrxApiTable* api, u8 idx, char* buf) {
    if (idx == 0u) {
        u8 p = put_str(buf, "KERN v", 6u);
        p += si_put_uint(api, buf + p, g_info.fw_major);
        buf[p++] = '.';
        p += si_put_uint(api, buf + p, g_info.fw_minor);
        buf[p++] = '.';
        p += si_put_uint(api, buf + p, g_info.fw_patch);
        buf[p] = 0;
        return buf;
    }
    if (idx == 1u) return g_info.build_date[0] ? g_info.build_date : S_UNK;
    if (idx == 2u) return g_info.git_hash[0] ? g_info.git_hash : S_UNK;
    if (idx == 3u) return g_info.boot_ver[0] ? g_info.boot_ver : S_UNK;
    if (idx == 4u) {
        u32 mhz = (api->power && api->power->sys_cpu_freq_hz)
            ? si_udivmod(api, api->power->sys_cpu_freq_hz(), 1000000u, 0) : 48u;
        u8 p = si_put_uint(api, buf, mhz);
        put_str(buf + p, " MHz", 4u);
        return buf;
    }
    u8 p = 0;
    buf[p++] = 'v';
    p += si_put_uint(api, buf + p, (api->abi_version >> 16) & 0xFFu);
    buf[p++] = '.';
    p += si_put_uint(api, buf + p, (api->abi_version >> 8) & 0xFFu);
    buf[p] = 0;
    return buf;
}

static const char* format_device_val(const KrxApiTable* api, u8 idx, char* buf) {
    if (idx == 0u) return g_info.model[0] ? g_info.model : S_UNK;
    if (idx == 1u) return g_info.mcu[0] ? g_info.mcu : S_UNK;
    if (idx == 2u) {
        u8 p = si_put_uint(api, buf, g_info.flash_kb);
        p += put_str(buf + p, " KB / ", 6u);
        p += si_put_uint(api, buf + p, g_info.sram_kb);
        p += put_str(buf + p, " KB", 3u);
        return buf;
    }
    if (idx == 3u) return g_info.serial[0] ? g_info.serial : S_UNK;
    if (idx == 4u) return g_mac_str;
    if (idx == 5u) return g_chip_id;

    if (g_info.qs1_status == KRX_QS1_VERIFIED || g_info.qs1_status == KRX_QS1_UNVERIFIED) {
        return g_info.qs1[0] ? g_info.qs1 : S_INVALID;
    }
    if (g_info.qs1_status == KRX_QS1_EMPTY) return S_EMPTY;
    if (g_info.qs1_status == KRX_QS1_INVALID) return S_INVALID;
    return S_NONE;
}

static const char* format_status_val(const KrxApiTable* api, u8 idx, char* buf) {
    if (idx == 0u) {
        u16 mv = (api->power && api->power->sys_battery_mv) ? api->power->sys_battery_mv() : 0u;
        u8 pct = (api->power && api->power->sys_battery_pct) ? api->power->sys_battery_pct() : 0u;
        u8 chg = (api->power && api->power->sys_is_charging) ? api->power->sys_is_charging() : 0u;
        u8 crit = (api->power && api->power->sys_is_critical) ? api->power->sys_is_critical() : 0u;
        u8 low = (api->power && api->power->sys_is_low) ? api->power->sys_is_low() : 0u;
        u32 rem = 0, volts = si_udivmod(api, mv, 1000u, &rem);
        u32 centi = si_udivmod(api, rem, 10u, 0);
        u8 p = si_put_uint(api, buf, volts);
        buf[p++] = '.';
        si_put_2d(api, buf + p, centi);
        p += 2u;
        buf[p++] = 'V';
        buf[p++] = ' ';
        p += si_put_uint(api, buf + p, pct);
        buf[p++] = '%';
        if (chg) {
            put_str(buf + p, " CHG", 4u);
        } else if (crit) {
            put_str(buf + p, " CRIT", 5u);
        } else if (low) {
            put_str(buf + p, " LOW", 4u);
        } else {
            buf[p] = 0;
        }
        return buf;
    }
    if (idx == 1u) {
        u8 btype = (api->power && api->power->sys_battery_type) ? api->power->sys_battery_type() : 0u;
        if (btype > 5u) btype = 5u;
        return si_nth(BATT_PACKS, btype);
    }
    if (idx == 2u) {
        u16 raw = (api->power && api->power->sys_raw_battery_adc) ? api->power->sys_raw_battery_adc() : 0u;
        u8 p = put_str(buf, "raw ", 4u);
        p += si_put_uint(api, buf + p, raw);
        buf[p] = 0;
        return buf;
    }
    if (idx == 3u) {
        i16 deci_c = (api->power && api->power->sys_mcu_temp) ? api->power->sys_mcu_temp() : 0;
        u8 p = 0;
        if (deci_c < 0) {
            buf[p++] = '-';
            deci_c = (i16)(-deci_c);
        }
        u32 tenths = 0;
        u32 whole = si_udivmod(api, (u32)(u16)deci_c, 10u, &tenths);
        p += si_put_uint(api, buf + p, whole);
        buf[p++] = '.';
        buf[p++] = (char)('0' + tenths);
        buf[p++] = ' ';
        buf[p++] = 'C';
        buf[p] = 0;
        return buf;
    }
    if (idx == 4u) {
        u32 secs = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0u;
        u32 s = 0, mins = si_udivmod(api, secs, 60u, &s);
        u32 m = 0, h = si_udivmod(api, mins > 5999u ? 5999u : mins, 60u, &m);
        si_put_2d(api, buf + 0, h);
        buf[2] = ':';
        si_put_2d(api, buf + 3, m);
        buf[5] = ':';
        si_put_2d(api, buf + 6, s);
        buf[8] = 0;
        return buf;
    }
    u16 cur = (api->service && api->service->sys_stack_margin)
        ? api->service->sys_stack_margin() : 0u;
    u16 min_b = (api->service && api->service->sys_stack_free_min)
        ? api->service->sys_stack_free_min() : 0u;
    u8 p = si_put_uint(api, buf, cur);
    buf[p++] = 'B';
    if (min_b > 0u && min_b != cur) {
        p += put_str(buf + p, " / ", 3u);
        p += si_put_uint(api, buf + p, min_b);
        buf[p++] = 'B';
    }
    buf[p] = 0;
    return buf;
}

static void render_kv_tab(const KrxApiTable* api, u8 tab, u8 inner_y, u8 inner_h) {
    const CapUiVTable* ui = api->ui;
    if (!ui || !ui->ui_draw_menu_item) return;

    const char* keys =
        (tab == TAB_SYSTEM) ? SYSTEM_KEYS : ((tab == TAB_DEVICE) ? DEVICE_KEYS : STATUS_KEYS);
    u8 total = tab_item_count(tab);
    u8 sc = g_scroll[tab];
    u8 sel = g_sel[tab];
    char val_buf[24];

    for (u8 v = 0; v < LIST_VISIBLE; v++) {
        u8 idx = (u8)(sc + v);
        if (idx >= total) break;
        const char* val;
        if (tab == TAB_SYSTEM) val = format_system_val(api, idx, val_buf);
        else if (tab == TAB_DEVICE) val = format_device_val(api, idx, val_buf);
        else val = format_status_val(api, idx, val_buf);

        const char* k = si_nth(keys, idx);
        i16 y = (i16)(inner_y + 1u + v * 12u);
        u8 is_sel = (idx == sel) ? 1u : 0u;
        ui->ui_draw_menu_item(y, (const u8*)k, si_len(k), (const u8*)val, si_len(val), is_sel, 0);

        if (tab == TAB_DEVICE && idx == 6u && api->display) {
            const u8* glyph =
                (g_info.qs1_status == KRX_QS1_VERIFIED) ? GLYPH_CHECK : GLYPH_CROSS;
            draw_glyph_5x5(api->display, 32, (i16)(y + 3), glyph, is_sel ? COLOR_CLEAR : COLOR_FILL);
        }
    }
    if (ui->ui_draw_scrollbar) {
        ui->ui_draw_scrollbar(123, inner_y, inner_h, sc, total, LIST_VISIBLE);
    }
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_tab = TAB_SYSTEM;
        for (u8 i = 0; i < TAB_COUNT; i++) {
            g_sel[i] = 0;
            g_scroll[i] = 0;
        }
        g_last_sec = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0u;
        for (u8 i = 0; i < sizeof(g_info); i++) ((u8*)&g_info)[i] = 0;
        if (api->service && api->service->sys_info) {
            api->service->sys_info(&g_info);
        }
        if (g_info.qs1_len > 0u) {
            u32 ok = (api->service && api->service->leaf_call) ? api->service->leaf_call(2, 0) : 0u;
            g_info.qs1_status = (ok == 1u) ? KRX_QS1_VERIFIED : KRX_QS1_UNVERIFIED;
        } else if (g_info.qs1_status == 0u) {
            g_info.qs1_status = (g_info.flags & KRX_SYS_INFO_FLAG_HAS_CERT)
                ? KRX_QS1_INVALID : KRX_QS1_NONE;
        }
        format_static_fields();
        set_sb(api, 1);
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        if (!param) return 0;
        const u8* ev = (const u8*)param;
        u8 kind = ev[0];

        if (kind == KRX_EV_TICK) {
            u32 now = (api->timer && api->timer->sys_uptime_secs) ? api->timer->sys_uptime_secs() : 0u;
            if (now != g_last_sec) {
                g_last_sec = now;
                return (g_tab == TAB_STATUS) ? 1 : 0;
            }
            return 0;
        }

        if (kind == KRX_EV_POWER) {
            return (g_tab == TAB_STATUS) ? 1 : 0;
        }

        i8 dir = 0;
        if (kind == KRX_EV_KEY) {
            u8 key = ev[1];
            u32 state = *(const u32*)(ev + 4);
            if (key == KEY_EXIT && state == 0u) return 0;
            if (state == 0u) {
                if (key >= KEY_1 && key <= KEY_4) {
                    g_tab = (u8)(key - KEY_1);
                    return 1;
                }
                if (key == KEY_HASH || key == KEY_MENU) {
                    g_tab = (u8)((g_tab + 1u) % TAB_COUNT);
                    return 1;
                }
                if (key == KEY_STAR) {
                    g_tab = (u8)((g_tab + TAB_COUNT - 1u) % TAB_COUNT);
                    return 1;
                }
            }
            if (state == 0u || state == 2u) {
                if (key == KEY_UP) dir = -1;
                else if (key == KEY_DOWN) dir = 1;
            }
        } else if (kind == KRX_EV_ENCODER) {
            i32 ed = *(const i32*)(ev + 4);
            if (ed < 0) dir = -1;
            else if (ed > 0) dir = 1;
        }

        if (dir != 0) {
            scroll_tab(g_tab, dir);
            return 1;
        }
        return (kind == KRX_EV_KEY);
    }

    if (op == KRX_OP_RENDER) {
        if (!api->display) return 0;
        api->display->clear();

        if (g_tab == TAB_LEGAL) {
            u32 rc = (api->service && api->service->leaf_call)
                ? api->service->leaf_call(1, (u32)g_scroll[TAB_LEGAL])
                : 1u;
            if (rc != 0u && api->ui && api->ui->ui_draw_menu_item) {
                u8 sel = g_sel[TAB_LEGAL] & 3u;
                for (u8 i = 0; i < 4u; i++) {
                    const char* k = si_nth(LEGAL_KEYS, i);
                    const char* v = si_nth(LEGAL_VALS, i);
                    api->ui->ui_draw_menu_item(
                        (i16)(12u + i * 12u),
                        (const u8*)k, si_len(k),
                        (const u8*)v, si_len(v),
                        i == sel, 0
                    );
                }
            }
        } else {
            render_kv_tab(api, g_tab, 11, 51);
        }

        krx_draw_tabbed_sheet(
            api, 0, 0, 128, 64,
            g_tab, TAB_COUNT,
            KRX_TAB_FLAG_INTEGRATED_86 | KRX_TAB_FLAG_BOTTOM_BORDER,
            TAB_TITLES, (u16)sizeof(TAB_TITLES)
        );
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        set_sb(api, 0);
        return 0;
    }

    return 0;
}
