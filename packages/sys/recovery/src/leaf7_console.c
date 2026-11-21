/*
 * Recovery Leaf 7: Borderless 8-Row Live-Streaming Scrollable Applet Console
 */
#include "rec_shared.h"

#define CON_MAX_LINES 18u
#define CON_VIS       8u

typedef struct {
    const KrxApiTable* api;
    const RecState*    st;
    char*              ring;
    u8                 count;
    u8                 top;
} ConState;

__attribute__((noinline))
static void con_render(const ConState* c) {
    krx_clear(c->api);
    for (u8 r = 0u; r < CON_VIS; r++) {
        u8 idx = (u8)(c->top + r);
        if (idx >= c->count) break;
        const char* ln = c->ring + ((u32)idx << 5);
        krx_draw_str_ex(c->api, 2, (i16)(6 + r * 6), (const u8*)ln, krx_strlen(ln), KRX_FONT_TOMTHUMB, 1u);
    }
    if (c->count > CON_VIS) {
        krx_ui_draw_scrollbar(c->api, 125u, 1u, 50u, (u16)c->top, (u16)c->count, CON_VIS);
    }
    c->st->ui.softbar(c->api, "", "", "EXIT", "Back");
    krx_commit_frame(c->api);
}

__attribute__((noinline))
static void con_log2(ConState* c, const char* a, const char* b) {
    if (c->count >= CON_MAX_LINES) return;
    char* dst = c->ring + ((u32)c->count << 5);
    u32 n = 0u;
    while (*a && n < 30u) dst[n++] = *a++;
    if (b) { while (*b && n < 30u) dst[n++] = *b++; }
    dst[n] = 0;
    c->count++;
    c->top = (c->count > CON_VIS) ? (u8)(c->count - CON_VIS) : 0u;
    con_render(c);
}

__attribute__((noinline))
static void con_log_bytes(ConState* c, const char* pfx, u32 val) {
    char num[12];
    u32 nn = c->st->ui.u32_dec(num, val);
    num[nn++] = 'B'; num[nn] = 0;
    con_log2(c, pfx, num);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf7_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    RecState* st = (RecState*)ctx->arg;
    const RecScratch* sc = st->sc;

    ConState con = { api, st, st->sc->con_ring, 0u, 0u };
    const char* path = sc->sel_path;
    u8 plen = (u8)krx_strlen(path);

    con_log2(&con, "> ", path);

    u8 buf[64];
    u16 rd = st->ui.rd_at(api, path, plen, 0u, buf, 64u);
    if (rd < 16u) {
        con_log2(&con, "ERR: Read fail", 0);
    } else {
        u32 id = (u32)buf[0] | ((u32)buf[1] << 8) | ((u32)buf[2] << 16);
        con_log2(&con, "Fmt: ", (id == 0x0058524Bu) ? "KRX OK" : "Raw");
        con_log_bytes(&con, "Size: ", sc->sel_size);

        u32 crc = krx_crc32(api, buf, rd);
        u8 cb[4] = { (u8)(crc >> 24), (u8)(crc >> 16), (u8)(crc >> 8), (u8)crc };
        char hx[9];
        st->ui.fmt_hex(hx, cb, 4u);
        con_log2(&con, "CRC: 0x", hx);
        con_log2(&con, "Verifying...", 0);

        u32 off = 0u;
        while (off < sc->sel_size) {
            u16 n = st->ui.rd_at(api, path, plen, off, buf, 64u);
            if (!n) break;
            off += n;
        }
        con_log_bytes(&con, "Read: ", off);
        con_log2(&con, (off == sc->sel_size) ? "OK" : "Short read", 0);
    }
    con_log2(&con, "Done.", 0);

    for (;;) {
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) { st->next_leaf = REC_LEAF_FILES; return 0u; }
        if (k == KEY_UP && con.top) { con.top--; con_render(&con); }
        if (k == KEY_DOWN && con.top + CON_VIS < con.count) { con.top++; con_render(&con); }
    }
}
