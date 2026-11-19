/*
 * DFU Leaf 2: OS Settings Hold-Confirm Modal & Battery Brownout Warning
 */
#include "dfu_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    if (st->op == KRX_OP_START) {
        static const char title[]    = "DFU Update";
        static const char msg_ok[]   = "Hold MENU to restart into\nUSB-C DFU update mode.";
        static const char msg_low[]  = "Battery is low! Plug in USB\npower, then hold MENU.";
        static const char lbl_hold[] = "Hold";
        static const char lbl_exit[] = "Cancel";

        u32 is_chg = krx_call(api, OP_IO_POWER, SUB_IO_POWER_CHARGING, 0u, 0u);
        u32 is_low = krx_call(api, OP_IO_POWER, SUB_IO_POWER_LOW, 0u, 0u) |
                     krx_call(api, OP_IO_POWER, SUB_IO_POWER_CRITICAL, 0u, 0u);

        const char* msg = (is_low && !is_chg) ? msg_low : msg_ok;
        u8 msg_len = (is_low && !is_chg) ? (u8)(sizeof(msg_low) - 1u) : (u8)(sizeof(msg_ok) - 1u);

        krx_ui_show_confirm_labels(
            api, TAG_CONFIRM_DFU,
            (const u8*)title, sizeof(title) - 1u,
            (const u8*)msg, msg_len,
            (const u8*)lbl_hold, sizeof(lbl_hold) - 1u,
            (const u8*)lbl_exit, sizeof(lbl_exit) - 1u
        );
        krx_ui_set_modal_hold(api, 1000u);
        return 0u;
    }

    if (st->op == KRX_OP_EVENT) {
        const KrxEvent* ev = (const KrxEvent*)st->param;
        if (ev && ev->kind == KRX_EV_MODAL && ev->key == TAG_CONFIRM_DFU
            && ev->flags == MODAL_RESULT_CONFIRMED) {
            krx_sys_flush_config(api);
            krx_sys_reboot(api, 1u);
            return 1u;
        }
    }

    return 0u;
}
