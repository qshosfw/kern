/*
 * KERN Recovery Suite Root Module (`/apps/recovery.mod`, P0 Structured Paged)
 *
 * Hosts shared resident UI & I/O helpers (~1.8 KB, zero dfu_hook bloat),
 * detects optional companion modules (/apps/dfu.mod, /apps/multiboot.mod,
 * /apps/calib.mod), and orchestrates 9 borderless Recovery leaves.
 */
#include "rec_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((visibility("default"), naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile(
        ".hidden rec_root_base\n"
        "rec_root_base:\n"
        "push {lr}\n bl krx_main\n pop {pc}\n"
    );
}

__attribute__((visibility("hidden"))) extern const u8 rec_root_base[];

static RecState g_rec;

static void op_emit_badge(const KrxApiTable* api, i16 x, const char* key, const char* lbl) {
    KrxWidget d;
    d.p0 = (const u8*)key; d.len0 = krx_strlen(key);
    d.p1 = (const u8*)lbl; d.len1 = krx_strlen(lbl);
    d.x = x; d.y = 54; d.a = 0u;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_KEY_BADGE, (u32)&d, 0u);
}

static void op_softbar(
    const KrxApiTable* api, const char* lk, const char* ll, const char* rk, const char* rl
) {
    if (lk && lk[0]) op_emit_badge(api, 2, lk, ll);
    if (rk && rk[0]) {
        KrxTextReq tr;
        tr.text = (const u8*)rk; tr.x = 0; tr.y = 0; tr.len = krx_strlen(rk);
        tr.arg = 0u; tr.font = KRX_FONT_TOMTHUMB; tr.color = 1u; tr.flags = 1u; tr._pad = 0u;
        i16 tw = (i16)krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u) + 4;
        if (rl && rl[0]) {
            tr.text = (const u8*)rl; tr.len = krx_strlen(rl);
            tw += 3 + (i16)krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u);
        }
        op_emit_badge(api, 126 - tw, rk, rl);
    }
}

static void op_kv(const KrxApiTable* api, i16 y, const char* k, const char* v) {
    KrxTextReq r;
    r.text = (const u8*)k; r.x = 3; r.y = y; r.len = krx_strlen(k);
    r.arg = KRX_ALIGN_LEFT; r.font = KRX_FONT_TOMTHUMB; r.color = 1u; r.flags = 0u; r._pad = 0u;
    krx_call(api, OP_GFX_TEXT, (u32)&r, 0u, 0u);
    r.text = (const u8*)v; r.x = 121; r.len = krx_strlen(v); r.arg = KRX_ALIGN_RIGHT;
    krx_call(api, OP_GFX_TEXT, (u32)&r, 0u, 0u);
}

static void op_draw_ctr(const KrxApiTable* api, i16 y, const char* s) {
    krx_draw_str_align(api, 64, y, (const u8*)s, (u16)krx_strlen(s),
                       KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
}

static void op_draw_box(const KrxApiTable* api, i16 x, i16 y, u8 w, u8 h, u8 c) {
    krx_fill_rect(api, x, y, w, h, c);
}

static u8 op_wait_key(const KrxApiTable* api) {
    while (krx_poll_key(api) != 0u) krx_delay_ms(api, 12u);
    for (;;) {
        u8 k = krx_poll_key(api);
        if (k != 0u) {
            krx_delay_ms(api, 18u);
            if (krx_poll_key(api) == k) return k;
        }
        krx_delay_ms(api, 12u);
    }
}

static u32 op_u32_dec(char* dst, u32 val) {
    char tmp[10];
    u32 n = 0u;
    do {
        u32 q = 0u;
        while (val >= 10u) { val -= 10u; q++; }
        tmp[n++] = (char)('0' + val);
        val = q;
    } while (val > 0u);
    for (u32 i = 0u; i < n; i++) dst[i] = tmp[n - 1u - i];
    return n;
}

static const char* op_nth_s(const char* p, u8 i) {
    while (i--) { while (*p) p++; p++; }
    return p;
}

static void op_fmt_hex(char* dst, const u8* src, u32 n) {
    for (u32 i = 0u; i < n; i++) {
        u8 b = src[i], hi = b >> 4, lo = b & 0xFu;
        dst[i * 2u]     = (char)(hi + (hi < 10u ? '0' : ('A' - 10)));
        dst[i * 2u + 1] = (char)(lo + (lo < 10u ? '0' : ('A' - 10)));
    }
    dst[n * 2u] = 0;
}

static u16 op_rd_at(const KrxApiTable* api, const char* path, u8 plen, u32 off, u8* buf, u16 len) {
    return krx_fs_read_at(api, (const u8*)path, plen, off, buf, len);
}

static u16 op_pwr_get(const KrxApiTable* api, u8 sub) {
    return krx_pwr_u16(api, sub);
}

/* ── SRAM-Resident Bare-Metal SPI2 / ST7565 / Bootloader Helpers ──────────── */

__attribute__((noinline))
static u8 op_spi_xfer(u32 base, u8 b) {
    volatile u32* sr = (volatile u32*)(base + 8u);
    volatile u8*  dr = (volatile u8*)(base + 12u);
    while (!(*sr & 2u)) {}
    *dr = b;
    while (!(*sr & 1u)) {}
    return *dr;
}

static void op_py25_read(u32 addr, u8* dst, u32 len) {
    volatile u32* ga = (volatile u32*)0x50000000u;
    ga[10] = 8u; /* PA3 FLASH_CS LOW */
    op_spi_xfer(0x40003800u, 0x03u);
    op_spi_xfer(0x40003800u, (u8)(addr >> 16));
    op_spi_xfer(0x40003800u, (u8)(addr >> 8));
    op_spi_xfer(0x40003800u, (u8)addr);
    for (u32 i = 0u; i < len; i++) dst[i] = op_spi_xfer(0x40003800u, 0xFFu);
    while (*(volatile u32*)0x40003808u & 0x80u) {}
    ga[6] = 8u; /* PA3 FLASH_CS HIGH */
}

static void op_lcd_bar(u32 done, u32 total) {
    u32 fill = 0u, rem = done * 100u;
    if (total) while (rem >= total && fill < 100u) { rem -= total; fill++; }
    volatile u32* ga = (volatile u32*)0x50000000u;
    volatile u32* gb = (volatile u32*)0x50000400u;
    gb[10] = 4u; ga[10] = 64u;
    op_spi_xfer(0x40013000u, 0xB4u);
    op_spi_xfer(0x40013000u, 0x10u);
    op_spi_xfer(0x40013000u, 0x0Cu);
    while (*(volatile u32*)0x40013008u & 0x80u) {}
    ga[6] = 64u;
    for (u32 i = 0u; i < 104u; i++) {
        op_spi_xfer(0x40013000u, (!i || i == 103u) ? 0x78u : ((i > 1u && i <= fill + 1u) ? 0xB4u : 0x84u));
    }
    while (*(volatile u32*)0x40013008u & 0x80u) {}
    gb[6] = 4u;
}

static u32 op_probe_bl(char* out, u32* app_kb_out, u8* vec_ok_out) {
    const u8* rom = (const u8*)0x08000000u;
    u32 vp = 0u;
    for (u32 i = 0x1800u; i + 18u <= 0x26FFu; i++) {
        if (rom[i] == 0x12u && rom[i + 1] == 0x01u && rom[i + 2] == 0x00u &&
            rom[i + 3] == 0x02u && rom[i + 7] == 0x40u) {
            u16 vid = (u16)rom[i + 8] | ((u16)rom[i + 9] << 8);
            u16 pid = (u16)rom[i + 10] | ((u16)rom[i + 11] << 8);
            vp = ((u32)vid << 16) | pid;
            break;
        }
    }
    u32 n = 0u;
    if (vp == 0x36B70001u && rom[0x209Au] == '7' && rom[0x209Bu] == '.') {
        const char* p = "Stock ";
        while (*p) out[n++] = *p++;
        for (u32 i = 0u; i < 8u && rom[0x209Au + i] >= 0x20u && rom[0x209Au + i] <= 0x7Eu; i++) {
            out[n++] = (char)rom[0x209Au + i];
        }
    } else {
        const char* p = (vp == 0x36B7FFFFu) ? "Moto UF2" : "Custom BL";
        while (*p) out[n++] = *p++;
    }
    out[n] = 0;

    u32 app_kb = 0u;
    for (u32 kb = 118u; kb && !app_kb; kb--) {
        const volatile u32* p = (const volatile u32*)(REC_APP_FLASH_BASE + ((kb - 1u) << 10));
        for (u32 w = 0u; w < 256u; w++) if (~p[w]) { app_kb = kb; break; }
    }
    *app_kb_out = app_kb;

    const volatile u32* v0 = (const volatile u32*)REC_APP_FLASH_BASE;
    *vec_ok_out = (!(v0[0] & 3u) && (v0[0] - 0x20000400u) <= 0x3C00u &&
                   (v0[1] & 1u) && (v0[1] - REC_APP_FLASH_BASE) < 0x1D800u) ? 1u : 0u;
    return vp;
}

static u8 mod_exists(const KrxApiTable* api, const char* path) {
    return krx_fs_exists(api, (const u8*)path, (u8)krx_strlen(path)) ? 1u : 0u;
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    (void)param;
    if (!api || op != KRX_OP_START) return 0;

    g_rec.ui.softbar   = op_softbar;
    g_rec.ui.kv        = op_kv;
    g_rec.ui.draw_ctr  = op_draw_ctr;
    g_rec.ui.draw_box  = op_draw_box;
    g_rec.ui.wait_key  = op_wait_key;
    g_rec.ui.u32_dec   = op_u32_dec;
    g_rec.ui.nth_s     = op_nth_s;
    g_rec.ui.fmt_hex   = op_fmt_hex;
    g_rec.ui.rd_at     = op_rd_at;
    g_rec.ui.pwr_get   = op_pwr_get;
    g_rec.ui.py25_read = op_py25_read;
    g_rec.ui.lcd_bar   = op_lcd_bar;
    g_rec.ui.probe_bl  = op_probe_bl;
    g_rec.sc           = (RecScratch*)((u32)rec_root_base + REC_SCRATCH_OFFSET);

    g_rec.has_dfu       = mod_exists(api, "/apps/dfu.mod");
    g_rec.has_multiboot = mod_exists(api, "/apps/multiboot.mod");
    g_rec.has_calib     = mod_exists(api, "/apps/calib.mod");
    g_rec.menu_sel      = 0u;

    for (;;) {
        g_rec.next_leaf = 0u;
        g_rec.exit_code = REC_EXIT_OS;
        krx_leaf_call(api, REC_LEAF_MENU, (u32)&g_rec);
        if (g_rec.next_leaf == 0u) {
            return (int)g_rec.exit_code;
        }
        u8 leaf = g_rec.next_leaf;
        while (leaf != 0u) {
            g_rec.next_leaf = 0u;
            krx_leaf_call(api, leaf, (u32)&g_rec);
            leaf = g_rec.next_leaf;
        }
    }
}
