#include "../bench/krx.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("b krx_main\n");
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        if (api->radio && api->radio->rf_tune) {
            api->radio->rf_tune(145500000, 25, 12500);
        }
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        if (param) {
            const KrxEvent* ev = (const KrxEvent*)param;
            if (ev->kind == KRX_EV_KEY && ev->key == KEY_EXIT) {
                return 0; // Unhandled -> request OS exit
            }
        }
        return 1; // Consumed
    }

    if (op == KRX_OP_RENDER) {
        if (api->display) {
            if (api->display->clear) api->display->clear();
            if (api->display->draw_str) {
                api->display->draw_str(16, 26, (const u8*)"BEACON 145.500", 14, 0, 1);
            }
        }
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        return 0;
    }

    return 0;
}
