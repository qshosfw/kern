#include <krx.h>

static u32 g_start_hz;
static u32 g_step_hz;
static i16 g_bins[64];

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("b krx_main\n");
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;
    if (op == KRX_OP_START) {
        g_start_hz = 433000000u;
        g_step_hz = 25000u;
        return 0;
    }
    if (op == KRX_OP_EVENT && param) {
        const KrxEvent* ev = (const KrxEvent*)param;
        if (ev->kind == KRX_EV_KEY) {
            if (ev->key == KEY_EXIT) return 0;
            if (ev->key == KEY_UP) g_start_hz += 1000000u;
            if (ev->key == KEY_DOWN && g_start_hz > 20000000u) g_start_hz -= 1000000u;
        }
        return 1;
    }
    if (op == KRX_OP_RENDER) {
        struct { u32 start_hz; u32 step_hz; i16* out_rssi; u16 count; u16 _pad; } req = {
            g_start_hz, g_step_hz, g_bins, 64u, 0u
        };
        krx_clear(api);
        krx_call(api, OP_RF_BULK, SUB_RF_BULK_SWEEP, (u32)&req, 0u);
        krx_draw_str_ex(api, 2, 1, (const u8*)"SPECTRUM", 8u, 0u, 1u);
        for (int i = 0; i < 64; i++) {
            int db = (int)g_bins[i] + 130;
            int h = db > 48 ? 48 : (db < 1 ? 1 : db);
            krx_fill_rect(api, (i16)(i * 2), (i16)(63 - h), 2, (i16)h, 1u);
        }
        return 0;
    }
    return 0;
}
