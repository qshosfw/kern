#include "../bench/krx.h"

static u32 g_paged_res = 0;
static u32 g_status = 0;

typedef u32 (*Block1Fn)(const KrxApiTable* api, u32 arg);

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
        p_log(api, "ROOT: P1 PAGED START\n", 21);

        u32 fn_addr = api->service->paged_call(1, 0, 0);
        if (fn_addr == 0) {
            g_status = 3;
            p_log(api, "ROOT: PAGED_CALL FAIL\n", 22);
            return 0;
        }

        Block1Fn fn = (Block1Fn)fn_addr;
        u32 res = fn(api, 40);
        g_paged_res = res;

        u32 resume_addr = api->service->paged_return();
        (void)resume_addr;

        if (res == 47) {
            g_status = 2;
            p_log(api, "ROOT: DEMAND STREAM RES=47 PASS\n", 32);
        } else {
            g_status = 3;
            p_log(api, "ROOT: DEMAND STREAM MISMATCH\n", 29);
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
        if (api->display) {
            api->display->clear();
            api->display->draw_str(2, 2, (const u8*)"PAGED P1 STREAM", 15, 0, 1);
            if (g_status == 2) {
                api->display->draw_str(2, 16, (const u8*)"STREAM OK: 47", 13, 0, 1);
            } else if (g_status == 3) {
                api->display->draw_str(2, 16, (const u8*)"STREAM FAILED", 13, 0, 1);
            }
        }
        return 0;
    }

    return 0;
}
