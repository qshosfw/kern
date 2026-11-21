#include <krx.h>

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("b krx_main\n");
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        krx_rf_tune(api, 145500000u, KRX_RF_WIDTH_25K, 12500u);
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
        krx_draw_str_ex(api, 16, 26, (const u8*)"BEACON 145.500", 14u, 0u, 1u);
        return 0;
    }

    return 0;
}
