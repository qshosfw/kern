#include "../bench/krx.h"

static u32 g_leaf_res = 0;
static u32 g_status = 0;

__attribute__((section(".text.entry")))
u32 krx_entry(u32 op, const KrxApiTable* api, void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_status = 1;
        krx_print(api, "ROOT: P0 LEAF START\n");
        u32 res = api->service->leaf_call(1, 42);
        g_leaf_res = res;
        if (res == 104) {
            g_status = 2;
            krx_print(api, "ROOT: LEAF PASS(104)\n");
        } else {
            g_status = 3;
            krx_print(api, "ROOT: LEAF FAIL\n");
        }
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        const u8* ev = (const u8*)param;
        if (ev && ev[0] == 1 && ev[1] == 6) {
            return 0;
        }
        return 1;
    }

    if (op == KRX_OP_RENDER) {
        api->display->clear();
        krx_draw_str(api, 2, 2, "LEAF P0 OVERLAY");
        if (g_status == 2) {
            krx_draw_str(api, 2, 16, "TRAMP OK: 104");
        } else if (g_status == 3) {
            krx_draw_str(api, 2, 16, "CHAIN FAILED");
        }
        return 0;
    }

    return 0;
}
