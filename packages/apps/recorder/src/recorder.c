#include <krx.h>

static u8 g_rec;

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("b krx_main\n");
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;
    if (op == KRX_OP_START) {
        g_rec = 0;
        return 0;
    }
    if (op == KRX_OP_EVENT && param) {
        const KrxEvent* ev = (const KrxEvent*)param;
        if (ev->kind == KRX_EV_KEY) {
            if (ev->key == KEY_EXIT) return 0;
            if (ev->key == KEY_MENU) g_rec ^= 1u;
        }
        return 1;
    }
    if (op == KRX_OP_RENDER) {
        krx_clear(api);
        krx_draw_str_ex(api, 4, 2, (const u8*)"QSO RECORDER", 12u, 0u, 1u);
        krx_draw_str_ex(api, 4, 26, g_rec ? (const u8*)"REC IMA-ADPCM 6KB/S" : (const u8*)"STANDBY / PARROT", g_rec ? 19u : 16u, 0u, 1u);
        krx_draw_str_ex(api, 4, 48, (const u8*)"M:REC   EXIT:BACK", 17u, 0u, 1u);
        return 0;
    }
    return 0;
}
