/*
 * DFU Leaf 7: Calibration Manager (/sys/calib.dat, Ascon MAC, QS-1 & Backup)
 */
#include "dfu_shared.h"

static u8 backup_calibration(const KrxApiTable* api) {
    static const char src[] = "/sys/calib.dat";
    static const char dir[] = "/boot";
    static const char dst[] = "/boot/calib.bkp";
    u8 buf[512];
    if (krx_fs_read(api, (const u8*)src, sizeof(src) - 1u, buf, 512u) != 512u) {
        return 0u;
    }
    krx_fs_mkdir(api, (const u8*)dir, sizeof(dir) - 1u);
    return (krx_fs_write(api, (const u8*)dst, sizeof(dst) - 1u, buf, 512u) == 0u) ? 1u : 0u;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf7_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    static const char p_dat[] = "/sys/calib.dat";
    static const char p_sig[] = "/sys/calib.sig";
    static const char p_bkp[] = "/boot/calib.bkp";
    const char* note = "";

    for (;;) {
        u32 dat_sz = krx_fs_size(api, (const u8*)p_dat, sizeof(p_dat) - 1u);
        u8  has_sig = krx_fs_exists(api, (const u8*)p_sig, sizeof(p_sig) - 1u);
        u8  has_bkp = krx_fs_exists(api, (const u8*)p_bkp, sizeof(p_bkp) - 1u);

        krx_clear(api);
        st->ui.header(api, "Calibration");
        st->ui.kv(api, 17, "calib.dat", (dat_sz == 512u) ? "512 B OK" : "Missing");
        st->ui.kv(api, 25, "Ascon MAC", has_sig ? "Present" : "Missing");
        st->ui.kv(api, 33, "Factory Page",
                  (st->kbhp.flags & KBHP_FLAG_FACTORY_PAGE_OK) ? "QS-1 OK" : "Unset");
        st->ui.kv(api, 41, "Backup", has_bkp ? "/boot/calib.bkp" : "None");
        if (note[0]) {
            krx_draw_str_align(api, 64, 50, (const u8*)note, krx_strlen(note),
                               KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
        }
        st->ui.softbar(api, "MENU", "Backup", "EXIT", "Back");
        krx_commit_frame(api);

        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (k == KEY_MENU) {
            note = backup_calibration(api) ? "Backup saved" : "Backup failed";
        }
    }
}
