/*
 * DFU Leaf 2: OS Settings Hold-Confirm Dialog & Clean Exit Back to Settings
 */
#include "dfu_shared.h"

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    if (st->op == KRX_OP_START) {
        static const char t[] = "Enter DFU Mode";
        static const char m[] = "Hold MENU to reboot into\nUSB-C DFU flash mode.";
        static const char c[] = "Hold";
        static const char x[] = "Cancel";
        krx_ui_show_confirm_labels(
            api, TAG_CONFIRM_DFU,
            (const u8*)t, sizeof(t) - 1,
            (const u8*)m, sizeof(m) - 1,
            (const u8*)c, sizeof(c) - 1,
            (const u8*)x, sizeof(x) - 1
        );
        krx_ui_set_modal_hold(api, 1000u);
        return 0;
    }

    if (st->op == KRX_OP_EVENT) {
        const KrxEvent* ev = (const KrxEvent*)st->param;
        if (ev && ev->kind == KRX_EV_MODAL && ev->key == TAG_CONFIRM_DFU
            && ev->flags == MODAL_RESULT_CONFIRMED) {
            krx_sys_flush_config(api);
            krx_sys_reboot(api, 1u);
            return 1;
        }
    }

    return 0;
}
