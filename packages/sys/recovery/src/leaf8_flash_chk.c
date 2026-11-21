/*
 * Recovery Leaf 8: Anti-Brick Pre-Flash Battery, Extent & Page-0 Vector Check
 */
#include "rec_shared.h"

typedef struct {
    const KrxApiTable* api;
    const RecState*    st;
    const char*        path;
    u8                 plen;
    u32                off;
    u32                end;
    u8                 buf[64];
    u16                pos;
    u16                len;
} RleReader;

__attribute__((noinline))
static int rle_getc(RleReader* r) {
    if (r->pos >= r->len) {
        if (r->off >= r->end) return -1;
        u32 rem = r->end - r->off;
        u16 n = r->st->ui.rd_at(r->api, r->path, r->plen, r->off, r->buf, (rem > 64u) ? 64u : (u16)rem);
        if (!n) return -1;
        r->off += n; r->pos = 0u; r->len = n;
    }
    return (int)r->buf[r->pos++];
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf8_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    RecState* st = (RecState*)ctx->arg;
    RecScratch* sc = st->sc;
    RecFlashJob* job = &sc->job;

    const char* path = sc->sel_path;
    u8 plen = (u8)krx_strlen(path);
    u8 err = 0u;

    u16 mv = st->ui.pwr_get(api, SUB_IO_POWER_BATT_MV);
    if ((st->ui.pwr_get(api, SUB_IO_POWER_CRITICAL) || (mv && mv < 6800u)) &&
        !st->ui.pwr_get(api, SUB_IO_POWER_CHARGING)) {
        err = 1u;
    } else if (krx_fs_extents(api, (const u8*)path, plen, &job->ext) != sizeof(KrxFileExtents) ||
               !job->ext.depth) {
        err = 2u;
    } else {
        for (u32 i = 0u; i < 256u; i++) job->page0[i] = 0xFFu;
        u8 ok = 0u;
        if (job->fmt == FLASH_FMT_RAW || job->fmt == FLASH_FMT_KFW_RAW) {
            ok = st->ui.rd_at(api, path, plen, job->data_off, job->page0, 256u) >= 16u;
        } else if (job->fmt == FLASH_FMT_UF2) {
            for (u32 b = 0u, n = job->stored_size >> 9; b < n; b++) {
                u32 h[8];
                if (st->ui.rd_at(api, path, plen, b << 9, (u8*)h, 32u) < 32u) break;
                if (h[3] == REC_APP_FLASH_BASE) {
                    ok = st->ui.rd_at(api, path, plen, (b << 9) + 32u, job->page0, 256u) == 256u;
                    break;
                }
            }
        } else {
            RleReader rr;
            rr.api = api; rr.st = st; rr.path = path; rr.plen = plen;
            rr.off = job->data_off; rr.end = job->data_off + job->stored_size;
            rr.pos = 0u; rr.len = 0u;
            u32 dst = 0u;
            while (dst < 256u) {
                int ctrl = rle_getc(&rr);
                if (ctrl < 0) break;
                if (ctrl <= 127) {
                    for (int c = ctrl + 1; c-- && dst < 256u; ) {
                        int v = rle_getc(&rr);
                        if (v < 0) { dst = 0u; break; }
                        job->page0[dst++] = (u8)v;
                    }
                } else if (ctrl >= 129) {
                    int v = rle_getc(&rr);
                    if (v < 0) { dst = 0u; break; }
                    for (int c = 257 - ctrl; c-- && dst < 256u; ) job->page0[dst++] = (u8)v;
                }
            }
            ok = dst >= 16u;
        }
        if (!ok) err = 3u;
        else {
            const u32* vec = (const u32*)job->page0;
            if ((vec[0] & 3u) || (vec[0] - 0x20000400u) > 0x3C00u) err = 4u;
            for (u32 i = 1u; i <= 3u && !err; i++) {
                if (!(vec[i] & 1u) || (vec[i] - (REC_APP_FLASH_BASE + 0x40u)) >= 0x1D7C0u) err = 4u;
            }
        }
    }

    st->flash_err = err;
    st->next_leaf = REC_LEAF_FLASH_UI;
    return 0u;
}
