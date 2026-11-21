/*
 * Multiboot Leaf 3: CRC-32 Deduplicated Golden OS Snapshot (/boot/kern_golden.bin)
 */
#include "mb_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf3_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    MbState* st = (MbState*)ctx->arg;

    static const char BDIR[]  = "/boot";
    static const char GPATH[] = "/boot/kern_golden.bin";
    const char* msg = 0;

    if (!st->vec_ok || !st->app_bytes) {
        msg = "Active vectors invalid";
    } else if (st->golden_synced) {
        msg = "Golden backup up to date";
    } else {
        while (krx_poll_key(api) != 0u) krx_delay_ms(api, 10u);
        u8 hold = 0u;
        for (;;) {
            krx_clear(api);
            st->ui.draw_ctr(api, 15, "kern_golden.bin");
            st->ui.draw_ctr(api, 26, "Hold MENU to save");
            st->ui.draw_box(api, 14, 34, 100, 6, 1u);
            st->ui.draw_box(api, 15, 35, 98, 4, 0u);
            if (hold) st->ui.draw_box(api, 16, 36, (u8)(hold * 4u), 2, 1u);
            st->ui.softbar(api, "MENU", "Hold", "EXIT", "Cancel");
            krx_commit_frame(api);

            u8 k = krx_poll_key(api);
            if (k == KEY_EXIT) return 0u;
            if (k == KEY_MENU) { if (++hold >= 24u) break; }
            else hold = 0u;
            krx_delay_ms(api, 25u);
        }

        krx_fs_mkdir(api, (const u8*)BDIR, sizeof(BDIR) - 1u);

        u32 pages = st->app_bytes >> 8;
        u8 ok = 1u;
        for (u32 p = 0u; p < pages; p++) {
            const u8* src = (const u8*)(MB_APP_FLASH_BASE + (p << 8));
            KrxFsReq req;
            req.path     = (const u8*)GPATH;
            req.path_len = (u8)(sizeof(GPATH) - 1u);
            req.offset   = 0u;
            req.buf      = (u8*)src;
            req.len      = 256u;
            if (krx_call(api, OP_FS_WRITE, p ? SUB_FS_WRITE_APPEND : SUB_FS_WRITE_FILE, (u32)&req, 0u) != 0u) {
                ok = 0u;
                break;
            }
            if ((p & 7u) == 0u || p + 1u == pages) {
                u32 w = 0u, rem = (p + 1u) * 96u;
                while (rem >= pages && w < 96u) { rem -= pages; w++; }
                krx_clear(api);
                st->ui.draw_ctr(api, 18, "Saving Golden OS...");
                st->ui.draw_box(api, 14, 34, 100, 6, 1u);
                st->ui.draw_box(api, 15, 35, 98, 4, 0u);
                if (w) st->ui.draw_box(api, 16, 36, (u8)w, 2, 1u);
                krx_commit_frame(api);
            }
        }
        if (ok) {
            st->ui.probe(api, st);
            msg = st->golden_synced ? "Golden backup verified" : "Verify CRC mismatch";
        } else {
            msg = "Write failed (FS full?)";
        }
    }

    char hc[16];
    mb_copy((u8*)hc, (const u8*)"CRC ", 4u);
    st->ui.fmt_hex32(hc + 4, st->golden_ok ? st->golden_crc : st->app_crc);

    krx_clear(api);
    st->ui.draw_ctr(api, 20, msg);
    st->ui.draw_ctr(api, 31, hc);
    st->ui.softbar(api, "", "", "EXIT", "Back");
    krx_commit_frame(api);
    while (st->ui.wait_key(api) != KEY_EXIT) {}
    return 0u;
}
