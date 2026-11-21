#include <krx.h>

static u8 g_contrast = 0;

__attribute__((section(".text.entry")))
u32 krx_entry(u32 op, const KrxApiTable* api, void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        g_contrast = krx_prop_get(api, KRX_PROP_CONTRAST);
        return 0;
    }

    if (op == KRX_OP_EVENT) {
        if (param) {
            const KrxEvent* ev = (const KrxEvent*)param;
            if (ev->kind == KRX_EV_KEY && ev->payload == 0u) {
                if (ev->key == KEY_EXIT) {
                    return 0;
                }
                g_contrast = krx_prop_get(api, KRX_PROP_CONTRAST);
                return 1;
            }
        }
        return 0;
    }

    if (op == KRX_OP_RENDER) {
        krx_clear(api);
        krx_draw_rect(api, 4, 10, 120, 48, COLOR_FILL);
        krx_draw_str_align(api, 64, 24, (const u8*)"HELLO FROM C", 12u, 0u, COLOR_FILL, 1u);
        krx_draw_str_align(api, 64, 38, (const u8*)"EXIT: Close App", 15u, 0u, COLOR_FILL, 1u);
        u8 q = (u8)((((u16)g_contrast) * 205u) >> 11);
        u8 r = g_contrast - (u8)(q * 10u);
        u8 q2 = (u8)((((u16)q) * 205u) >> 11);
        u8 tens = q - (u8)(q2 * 10u);
        u8 digits[3] = {
            (u8)('0' + tens),
            (u8)('0' + r),
            (u8)'%',
        };
        krx_draw_str_align(api, 64, 50, digits, 3u, 0u, COLOR_FILL, 1u);
        krx_commit_frame(api);
        return 0;
    }

    return 0;
}
