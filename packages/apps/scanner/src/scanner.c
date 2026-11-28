#include <krx.h>

static u32 g_freq_hz;
static u8 g_running;

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("b krx_main\n");
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;
    if (op == KRX_OP_START) {
        g_freq_hz = 144000000u;
        g_running = 1;
        return 0;
    }
    if (op == KRX_OP_EVENT && param) {
        const KrxEvent* ev = (const KrxEvent*)param;
        if (ev->kind == KRX_EV_KEY) {
            if (ev->key == KEY_EXIT) return 0;
            if (ev->key == KEY_MENU) g_running ^= 1u;
        }
        return 1;
    }
    if (op == KRX_OP_RENDER) {
        if (g_running) {
            g_freq_hz += 12500u;
            if (g_freq_hz > 148000000u) g_freq_hz = 144000000u;
            krx_rf_tune(api, g_freq_hz, KRX_RF_WIDTH_12K5, 12500u);
        }
        krx_clear(api);
        krx_draw_str_ex(api, 4, 2, (const u8*)"RF SCANNER", 10u, 0u, 1u);
        krx_draw_str_ex(api, 4, 24, g_running ? (const u8*)"SCANNING 144-148" : (const u8*)"PAUSED", g_running ? 16u : 6u, 0u, 1u);
        krx_draw_str_ex(api, 4, 48, (const u8*)"M:PAUSE  EXIT:STOP", 18u, 0u, 1u);
        return 0;
    }
    return 0;
}
