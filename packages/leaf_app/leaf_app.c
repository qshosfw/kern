#include "../bench/krx.h"

static u32 g_leaf_res = 0;
static u32 g_status = 0;

static void p_log(const KrxApiTable* api, const char* s, u16 len) {
    if (api && api->serial && api->serial->serial_write) {
        api->serial->serial_write((const u8*)s, len);
    }
}

__attribute__((section(".text.entry")))
u32 krx_entry(u32 op, const KrxApiTable* api, void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_status = 1;
        p_log(api, "ROOT: P0 LEAF START\n", 20);
        u32 res = api->service->leaf_call(1, 42);
        g_leaf_res = res;
        if (res == 104) {
            g_status = 2;
            p_log(api, "ROOT: LEAF PASS(104)\n", 21);
        } else {
            g_status = 3;
            p_log(api, "ROOT: LEAF FAIL\n", 16);
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
        if (api->display) {
            api->display->clear();
            api->display->draw_str(4, 12, (const u8*)"LEAF P0 OVERLAY", 15, 0, 1);
            if (g_status == 2) {
                api->display->draw_str(4, 24, (const u8*)"TRAMP OK: 104", 13, 0, 1);
            } else if (g_status == 3) {
                api->display->draw_str(4, 24, (const u8*)"CHAIN FAILED", 12, 0, 1);
            }
        }
        return 0;
    }

    return 0;
}
