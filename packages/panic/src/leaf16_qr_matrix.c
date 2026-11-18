/*
 * P0 Leaf 16: QR V2-L 25x25 Matrix Fixed Patterns, Format BCH & Zigzag Placement (<= 1 KB).
 */
#include "panic_shared.h"

static void qr_set(u32* g, u32* res, u8 r, u8 c, u8 v) {
    if (r >= 25u || c >= 25u) return;
    u32 m = 1u << c;
    if (v) g[r] |= m; else g[r] &= ~m;
    res[r] |= m;
}

static void draw_finder(u32* g, u32* res, i8 r0, i8 c0) {
    for (i8 dr = -1; dr <= 7; dr++) {
        i8 r = (i8)(r0 + dr);
        if (r < 0 || r >= 25) continue;
        for (i8 dc = -1; dc <= 7; dc++) {
            i8 c = (i8)(c0 + dc);
            if (c < 0 || c >= 25) continue;
            u8 blk = 0u;
            if (dr >= 0 && dr <= 6 && dc >= 0 && dc <= 6) {
                if (dr == 0 || dr == 6 || dc == 0 || dc == 6 ||
                    (dr >= 2 && dr <= 4 && dc >= 2 && dc <= 4)) blk = 1u;
            }
            qr_set(g, res, (u8)r, (u8)c, blk);
        }
    }
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf16_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op; (void)api;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;

    u32* g = st->qr_rows;
    u32 res[25];
    for (u8 i = 0; i < 25u; i++) { g[i] = 0u; res[i] = 0u; }

    draw_finder(g, res, 0, 0);
    draw_finder(g, res, 0, 18);
    draw_finder(g, res, 18, 0);

    for (u8 dr = 0; dr < 5u; dr++) {
        for (u8 dc = 0; dc < 5u; dc++) {
            u8 blk = (dr == 0u || dr == 4u || dc == 0u || dc == 4u || (dr == 2u && dc == 2u)) ? 1u : 0u;
            qr_set(g, res, (u8)(16u + dr), (u8)(16u + dc), blk);
        }
    }

    for (u8 i = 8u; i <= 16u; i++) {
        qr_set(g, res, 6u, i, (i & 1u) ? 0u : 1u);
        qr_set(g, res, i, 6u, (i & 1u) ? 0u : 1u);
    }
    qr_set(g, res, 17u, 8u, 1u);

    // Mask 0 ((r + c) & 1 == 0) with ECC-L BCH format word 0x77C4
    const u16 fmt = 0x77C4u;
    for (u8 k = 0; k < 15u; k++) {
        u8 b = (u8)((fmt >> k) & 1u);
        if (k < 6u) qr_set(g, res, k, 8u, b);
        else if (k == 6u) qr_set(g, res, 7u, 8u, b);
        else if (k == 7u) qr_set(g, res, 8u, 8u, b);
        else if (k == 8u) qr_set(g, res, 8u, 7u, b);
        else qr_set(g, res, 8u, (u8)(14u - k), b);

        if (k < 8u) qr_set(g, res, 8u, (u8)(24u - k), b);
        else qr_set(g, res, (u8)(32u - k), 8u, b);
    }

    const u8* cw = st->qr_cw;
    u16 bi = 0u;
    i8 dir = -1;
    i8 col = 24;
    while (col > 0) {
        if (col == 6) col--;
        i8 row = (dir < 0) ? 24 : 0;
        while (row >= 0 && row < 25) {
            for (u8 dc = 0; dc < 2u; dc++) {
                u8 c = (u8)(col - (i8)dc);
                u8 r = (u8)row;
                if ((res[r] & (1u << c)) == 0u) {
                    u8 bit = (bi < 352u) ? (u8)((cw[bi >> 3] >> (7u - (bi & 7u))) & 1u) : 0u;
                    bi++;
                    u8 m = (u8)(((r + c) & 1u) == 0u);
                    if (bit ^ m) g[r] |= (1u << c);
                }
            }
            row += dir;
        }
        dir = (i8)(-dir);
        col -= 2;
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
