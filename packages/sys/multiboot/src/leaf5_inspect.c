/*
 * Multiboot Leaf 5: Pre-Flight Slot Inspector (.kfw v1.0, .uf2, Raw ARM .bin)
 */
#include "mb_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf5_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    MbState* st = (MbState*)ctx->arg;
    MbScratch* sc = st->sc;
    MbFlashJob* job = &sc->job;
    mb_zero((u8*)job, sizeof(MbFlashJob));

    const char* path = sc->sel_path;
    u8 plen = (u8)krx_strlen(path);
    const char* base = path + 6; /* skip "/boot/" */
    job->is_golden = (base[0] == 'k' && base[5] == 'g' && base[11] == '.') ? 1u : 0u;

    u32 hdr[16];
    u16 rd = st->ui.rd_at(api, path, plen, 0u, (u8*)hdr, 64u);

    static const char KINDS[] = "Binary\0KFW\0KFW Raw\0KFW RLE\0UF2\0ARM Bin\0";
    static const char FITS[]  = "Unsupported\0Ready\0Need OTA\0Too Large\0Bad Target\0Bad Bounds\0";
    u8 ki = 0u, fi = 0u;
    u32 show_sz = sc->sel_size;

    if (rd >= 64u && hdr[0] == 0x57464B89u && hdr[1] == 0x0A1A0A0Du && (((u8*)hdr)[8] >> 4) == 1u) {
        ki = 1u;
        u32 pcnt = (hdr[2] >> 16) & 0xFFFFu;
        if (pcnt > 16u) pcnt = 16u;
        for (u32 i = 0u; i < pcnt; i++) {
            u32 part[8];
            if (st->ui.rd_at(api, path, plen, 64u + (i << 5), (u8*)part, 32u) < 32u) break;
            if ((part[0] & 0xFFu) == 0x10u) {
                u8 pf = (u8)(part[0] >> 8), comp = pf & 7u;
                show_sz = part[3];
                if ((pf & 0x18u) || comp > 1u) fi = 2u;
                else if (!part[3] || part[3] > MB_APP_MAX_BYTES || part[1] + part[2] > sc->sel_size) fi = 3u;
                else {
                    ki = comp ? 3u : 2u; fi = 1u;
                    job->fmt = comp ? FLASH_FMT_KFW_RLE : FLASH_FMT_KFW_RAW;
                    job->data_off = part[1]; job->stored_size = part[2]; job->raw_size = part[3];
                }
                break;
            }
        }
    } else if (rd >= 32u && hdr[0] == 0x0A324655u && hdr[1] == 0x9E5D5157u) {
        ki = 4u;
        u32 nblk = hdr[6];
        show_sz = nblk << 8;
        if ((hdr[2] & 0x2000u) && hdr[7] != 0x7D7A66EFu) fi = 4u;
        else if (hdr[4] != 256u || hdr[3] < MB_APP_FLASH_BASE || !nblk || nblk > MB_APP_MAX_PAGES) fi = 5u;
        else {
            fi = 1u;
            job->fmt = FLASH_FMT_UF2;
            job->stored_size = nblk << 9; job->raw_size = show_sz;
        }
    } else if (rd >= 16u && !(hdr[0] & 3u) && (hdr[0] - 0x20000400u) <= 0x3C00u &&
               (hdr[1] & 1u) && (hdr[1] - MB_APP_FLASH_BASE) < 0x1D800u) {
        ki = 5u;
        if (sc->sel_size && sc->sel_size <= MB_APP_MAX_BYTES) {
            fi = 1u;
            job->fmt = FLASH_FMT_RAW;
            job->stored_size = sc->sel_size; job->raw_size = sc->sel_size;
        } else fi = 3u;
    }

    char sz[14];
    u32 sn = st->ui.u32_dec(sz, show_sz);
    sz[sn++] = ' '; sz[sn++] = 'B'; sz[sn] = 0;

    static const char LBL[] = "Slot\0Format\0Size\0Status\0";
    const char* vals[4] = { base, st->ui.nth_s(KINDS, ki), sz, st->ui.nth_s(FITS, fi) };
    krx_clear(api);
    for (u8 i = 0u; i < 4u; i++) st->ui.kv(api, (i16)(11 + i * 10), st->ui.nth_s(LBL, i), vals[i]);
    st->ui.softbar(api, job->fmt ? "MENU" : "", job->fmt ? "Switch" : "", "EXIT", "Back");
    krx_commit_frame(api);

    for (;;) {
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) { st->next_leaf = MB_LEAF_SLOTS; return 0u; }
        if (k == KEY_MENU && job->fmt) { st->next_leaf = MB_LEAF_FLASH_CHK; return 0u; }
    }
}
