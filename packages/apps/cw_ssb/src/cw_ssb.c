#include <krx.h>

static u8 g_wpm;

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("b krx_main\n");
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;
    if (op == KRX_OP_START) {
        g_wpm = 18;
        return 0;
    }
    if (op == KRX_OP_EVENT && param) {
        const KrxEvent* ev = (const KrxEvent*)param;
        if (ev->kind == KRX_EV_KEY) {
            if (ev->key == KEY_EXIT) return 0;
            if (ev->key == KEY_UP && g_wpm < 40) g_wpm++;
            if (ev->key == KEY_DOWN && g_wpm > 8) g_wpm--;
        }
        return 1;
    }
    if (op == KRX_OP_RENDER) {
        krx_clear(api);
        krx_draw_str_ex(api, 4, 2, (const u8*)"CW / HF SSB", 11u, 0u, 1u);
        krx_draw_str_ex(api, 4, 24, (const u8*)"IAMBIC KEYER READY", 18u, 0u, 1u);
        krx_draw_str_ex(api, 4, 48, (const u8*)"UP/DN:WPM  EXIT:BACK", 20u, 0u, 1u);
        return 0;
    }
    return 0;
}
