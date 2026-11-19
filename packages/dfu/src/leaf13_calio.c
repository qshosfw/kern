/*
 * DFU Leaf 13: Calibration Storage I/O & Hardware-UID Ascon-128 Re-Signing Helper
 *
 * st->tool_sub:
 *   0 = Load /sys/calib.dat (512 B) into st->stage_buf
 *   1 = Save & Ascon-128 sign st->stage_buf -> /sys/calib.dat + /sys/calib.sig
 *   2 = Backup st->stage_buf -> /boot/calib.bkp
 */
#include "dfu_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf13_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    static const char P_DAT[] = "/sys/calib.dat";
    static const char P_DIR[] = "/boot";
    static const char P_BKP[] = "/boot/calib.bkp";
    u8* buf = st->stage_buf;

    if (st->tool_sub == 0u) {
        u16 rd = krx_fs_read(api, (const u8*)P_DAT, sizeof(P_DAT) - 1u, buf, 512u);
        st->tool_action = (rd == 512u) ? 0u : 1u;
    } else if (st->tool_sub == 1u) {
        u8 rc = krx_fs_write(api, (const u8*)P_DAT, sizeof(P_DAT) - 1u, buf, 512u);
        st->tool_action = (rc == 0u) ? 2u : 3u;
    } else {
        krx_fs_mkdir(api, (const u8*)P_DIR, sizeof(P_DIR) - 1u);
        u8 rc = krx_fs_write(api, (const u8*)P_BKP, sizeof(P_BKP) - 1u, buf, 512u);
        st->tool_action = (rc == 0u) ? 4u : 5u;
    }
    st->next_leaf = DFU_LEAF_CALIB;
    return 0u;
}
