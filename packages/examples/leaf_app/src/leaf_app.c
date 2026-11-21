#include <krx.h>

static u32 g_leaf_res = 0;
static u32 g_status = 0;

__attribute__((section(".text.entry")))
u32 krx_entry(u32 op, const KrxApiTable* api, void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_status = 1;
        krx_serial_write(api, (const u8*)"ROOT: P0 LEAF START\n", 20u);
        u32 res = krx_leaf_call(api, 1u, 42u);
        g_leaf_res = res;
        if (res == 104u) {
            g_status = 2;
            krx_serial_write(api, (const u8*)"ROOT: LEAF PASS(104)\n", 21u);
        } else {
            g_status = 3;
            krx_serial_write(api, (const u8*)"ROOT: LEAF FAIL\n", 16u);
        }
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        if (param) {
            const KrxEvent* ev = (const KrxEvent*)param;
            if (ev->kind == KRX_EV_KEY && ev->key == KEY_EXIT) {
                return 0;
            }
        }
        return 1;
    }

    if (op == KRX_OP_RENDER) {
        krx_clear(api);
        krx_draw_str_ex(api, 4, 12, (const u8*)"LEAF P0 OVERLAY", 15u, 0u, 1u);
        if (g_status == 2u) {
            krx_draw_str_ex(api, 4, 24, (const u8*)"TRAMP OK: 104", 13u, 0u, 1u);
        } else if (g_status == 3u) {
            krx_draw_str_ex(api, 4, 24, (const u8*)"CHAIN FAILED", 12u, 0u, 1u);
        }
        return 0;
    }

    return 0;
}
