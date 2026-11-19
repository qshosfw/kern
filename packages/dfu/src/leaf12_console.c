/*
 * DFU Leaf 12: Live-Streaming Scrollable Recovery Console (Android-Recovery Style)
 */
#include "dfu_shared.h"

#define CON_MAX_LINES 18u
#define CON_VIS       8u

typedef struct {
    const KrxApiTable*    api;
    const DfuSharedState* st;
    char*                 ring;
    u8                    count;
    u8                    top;
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

__attribute__((noinline))
static u16 read_chunk(const KrxApiTable* api, const char* path, u8 plen, u32 off, u8* buf) {
    return krx_fs_read_at(api, (const u8*)path, plen, off, buf, 64u);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf12_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;
    const DfuToolsScratch* ts = (const DfuToolsScratch*)st->stage_buf;

    ConState con = { api, st, (char*)(st->stage_buf + 384u), 0u, 0u };
    const char* path = ts->sel_path;
    u8 plen = (u8)krx_strlen(path);

    con_log2(&con, "> ", path);

    u8 buf[64];
    u16 rd = read_chunk(api, path, plen, 0u, buf);
    if (rd < 16u) {
        con_log2(&con, "ERR: Read fail", 0);
    } else {
        u32 id = (u32)buf[0] | ((u32)buf[1] << 8) | ((u32)buf[2] << 16);
        con_log2(&con, "Fmt: ", (id == 0x0058524Bu) ? "KRX OK" : "Raw");
        con_log_bytes(&con, "Size: ", ts->sel_size);

        u32 crc = krx_crc32(api, buf, rd);
        char hx[9];
        for (u32 i = 0u; i < 8u; i++) {
            u8 nib = (u8)(crc >> 28); crc <<= 4;
            hx[i] = (char)(nib + (nib < 10u ? '0' : ('A' - 10)));
        }
        hx[8] = 0;
        con_log2(&con, "CRC: 0x", hx);
        con_log2(&con, "Verifying...", 0);

        u32 off = 0u;
        while (off < ts->sel_size) {
            u16 n = read_chunk(api, path, plen, off, buf);
            if (n == 0u) break;
            off += n;
        }
        con_log_bytes(&con, "Read: ", off);
        con_log2(&con, (off == ts->sel_size) ? "OK" : "Short read", 0);
    }
    con_log2(&con, "Done.", 0);

    for (;;) {
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) { st->next_leaf = DFU_LEAF_FILES; return 0u; }
        if (k == KEY_UP && con.top > 0u) { con.top--; con_render(&con); }
        if (k == KEY_DOWN && con.top + CON_VIS < con.count) { con.top++; con_render(&con); }
    }
}
