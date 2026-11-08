#include "../bench/krx.h"

static u32 g_paged_res = 0;
static u32 g_status = 0;

typedef u32 (*Block1Fn)(const KrxApiTable* api, u32 arg);

__attribute__((section(".text.entry")))
u32 krx_entry(u32 op, const KrxApiTable* api, void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_status = 1;
        krx_print(api, "ROOT: P1 PAGED START\n");

        u32 fn_addr = api->service->paged_call(1, 0, 0);
        if (fn_addr == 0) {
            g_status = 3;
            krx_print(api, "ROOT: PAGED_CALL FAIL\n");
            return 0;
        }

        Block1Fn fn = (Block1Fn)fn_addr;
        u32 res = fn(api, 40);
        g_paged_res = res;

        u32 resume_addr = api->service->paged_return();
        (void)resume_addr;

        if (res == 47) {
            g_status = 2;
            krx_print(api, "ROOT: DEMAND STREAM RES=47 PASS\n");
        } else {
            g_status = 3;
            krx_print(api, "ROOT: DEMAND STREAM MISMATCH\n");
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
        krx_draw_str(api, 2, 2, "PAGED P1 STREAM");
        if (g_status == 2) {
            krx_draw_str(api, 2, 16, "STREAM BLK 1 OK");
            krx_draw_str(api, 2, 30, "RESULT: 47 PASS");
            krx_draw_str(api, 2, 44, "SPEC: VERIFIED");
        } else if (g_status == 3) {
            krx_draw_str(api, 2, 16, "STREAM: FAILED");
        } else {
            krx_draw_str(api, 2, 16, "RUNNING...");
        }
        return 0;
    }

    return 0;
}
