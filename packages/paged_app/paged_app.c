#include <krx.h>

static u32 g_paged_res = 0;
static u32 g_status = 0;

typedef u32 (*Block1Fn)(const KrxApiTable* api, u32 arg);

__attribute__((section(".text.entry")))
u32 krx_entry(u32 op, const KrxApiTable* api, void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_status = 1;
        krx_serial_write(api, (const u8*)"ROOT: P1 PAGED START\n", 21u);

        u32 res = KRX_PAGED_INVOKE(api, 1u, Block1Fn, 0u, 0u, api, 40u);
        g_paged_res = res;

        if (res == 47u) {
            g_status = 2;
            krx_serial_write(api, (const u8*)"ROOT: DEMAND STREAM RES=47 PASS\n", 32u);
        } else {
            g_status = 3;
            krx_serial_write(api, (const u8*)"ROOT: DEMAND STREAM MISMATCH\n", 29u);
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
        krx_draw_str_ex(api, 4, 12, (const u8*)"PAGED P1 DEMAND", 15u, 0u, 1u);
        if (g_status == 2u) {
            krx_draw_str_ex(api, 4, 24, (const u8*)"STREAM OK: 47", 13u, 0u, 1u);
        } else if (g_status == 3u) {
            krx_draw_str_ex(api, 4, 24, (const u8*)"STREAM FAILED", 13u, 0u, 1u);
        }
        return 0;
    }

    return 0;
}
