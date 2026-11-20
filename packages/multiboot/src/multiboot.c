/*
 * KERN Multiboot & Golden Backup Root Module (`/apps/multiboot.mod`, P0 Structured Paged)
 */
#include "mb_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((visibility("default"), naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile(
        ".hidden mb_root_base\n"
        "mb_root_base:\n"
        "push {lr}\n bl krx_main\n pop {pc}\n"
    );
}

__attribute__((visibility("hidden"))) extern const u8 mb_root_base[];

static MbState g_mb;

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

static void op_fmt_hex32(char* dst, u32 val) {
    for (u32 i = 0u; i < 8u; i++) {
        u8 nib = (u8)(val >> 28);
        val <<= 4;
        dst[i] = (char)(nib + (nib < 10u ? '0' : ('A' - 10)));
    }
    dst[8] = 0;
}

static u16 op_rd_at(const KrxApiTable* api, const char* path, u8 plen, u32 off, u8* buf, u16 len) {
    return krx_fs_read_at(api, (const u8*)path, plen, off, buf, len);
}

static u16 op_pwr_get(const KrxApiTable* api, u8 sub) {
    return krx_pwr_u16(api, sub);
}

static u32 op_crc32_step(u32 crc, const u8* data, u32 len) {
    for (u32 i = 0u; i < len; i++) {
        crc ^= (u32)data[i];
        for (u8 b = 0u; b < 8u; b++) {
            crc = (crc >> 1) ^ ((crc & 1u) ? 0xEDB88320u : 0u);
        }
    }
    return crc;
}

static u8 vecs_valid(const u32* v) {
    if ((v[0] & 3u) || (v[0] - 0x20000400u) > 0x3C00u) return 0u;
    for (u32 i = 1u; i <= 3u; i++) {
        if (!(v[i] & 1u) || (v[i] - (MB_APP_FLASH_BASE + 0x40u)) >= 0x1D7C0u) return 0u;
    }
    return 1u;
}

static void op_probe(const KrxApiTable* api, MbState* st) {
    /* Measure active internal App Flash to last non-0xFF 256-byte page */
    u32 pages = 0u;
    for (u32 p = MB_APP_MAX_PAGES; p && !pages; p--) {
        const volatile u32* ptr = (const volatile u32*)(MB_APP_FLASH_BASE + ((p - 1u) << 8));
        for (u32 w = 0u; w < 64u; w++) if (~ptr[w]) { pages = p; break; }
    }
    st->app_bytes = pages << 8;
    st->vec_ok = vecs_valid((const u32*)MB_APP_FLASH_BASE);
    st->app_crc = ~op_crc32_step(0xFFFFFFFFu, (const u8*)MB_APP_FLASH_BASE, st->app_bytes);

    /* Probe Golden OS backup (/boot/kern_golden.bin) */
    static const char GPATH[] = "/boot/kern_golden.bin";
    u8 gplen = (u8)(sizeof(GPATH) - 1u);
    st->golden_ok = 0u;
    st->golden_synced = 0u;
    st->golden_bytes = 0u;
    st->golden_crc = 0u;

    u32 gsz = krx_fs_size(api, (const u8*)GPATH, gplen);
    if (gsz >= 256u && gsz <= MB_APP_MAX_BYTES) {
        u8 buf[64];
        if (op_rd_at(api, GPATH, gplen, 0u, buf, 16u) == 16u && vecs_valid((const u32*)buf)) {
            u32 crc = 0xFFFFFFFFu;
            for (u32 off = 0u; off < gsz; off += 64u) {
                u32 rem = gsz - off;
                u16 n = op_rd_at(api, GPATH, gplen, off, buf, (rem > 64u) ? 64u : (u16)rem);
                if (!n) { crc = 0u; break; }
                crc = op_crc32_step(crc, buf, n);
            }
            st->golden_ok = 1u;
            st->golden_bytes = gsz;
            st->golden_crc = ~crc;
            if (gsz == st->app_bytes && st->golden_crc == st->app_crc) st->golden_synced = 1u;
        }
    }
}

/* ── SRAM-Resident Bare-Metal SPI2 / ST7565 Helpers ───────────────────────── */

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

static u8 mod_exists(const KrxApiTable* api, const char* p) {
    return krx_fs_exists(api, (const u8*)p, (u8)krx_strlen(p)) ? 1u : 0u;
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api || op != KRX_OP_START) return 0;

    g_mb.ui.softbar    = op_softbar;
    g_mb.ui.kv         = op_kv;
    g_mb.ui.draw_ctr   = op_draw_ctr;
    g_mb.ui.draw_box   = op_draw_box;
    g_mb.ui.wait_key   = op_wait_key;
    g_mb.ui.u32_dec    = op_u32_dec;
    g_mb.ui.nth_s      = op_nth_s;
    g_mb.ui.fmt_hex32  = op_fmt_hex32;
    g_mb.ui.rd_at      = op_rd_at;
    g_mb.ui.pwr_get    = op_pwr_get;
    g_mb.ui.crc32_step = op_crc32_step;
    g_mb.ui.probe      = op_probe;
    g_mb.ui.py25_read  = op_py25_read;
    g_mb.ui.lcd_bar    = op_lcd_bar;
    g_mb.sc            = (MbScratch*)((u32)mb_root_base + MB_SCRATCH_OFFSET);

    g_mb.boot_mode    = (param != 0) ? 1u : 0u;
    g_mb.has_dfu      = g_mb.boot_mode && mod_exists(api, "/apps/dfu.mod");
    g_mb.has_recovery = g_mb.boot_mode && mod_exists(api, "/apps/recovery.mod");
    g_mb.menu_sel     = 0u;
    op_probe(api, &g_mb);

    for (;;) {
        g_mb.next_leaf = 0u;
        g_mb.exit_code = MB_EXIT_BACK;
        krx_leaf_call(api, MB_LEAF_MENU, (u32)&g_mb);
        if (g_mb.next_leaf == 0u) {
            if (!g_mb.boot_mode) krx_call(api, OP_SYS_EVENT, 0u, 4u, 0u); /* CloseApp */
            return (int)g_mb.exit_code;
        }
        u8 leaf = g_mb.next_leaf;
        while (leaf != 0u) {
            g_mb.next_leaf = 0u;
            krx_leaf_call(api, leaf, (u32)&g_mb);
            leaf = g_mb.next_leaf;
        }
    }
}
