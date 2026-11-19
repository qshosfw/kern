/*
 * DFU Leaf 8: Peer Flash (Spacious DFU-Matched Radio -> Radio View)
 *
 * Mirrors the exact visual balance of the DFU screen:
 *   - y=1..31:  Two DFU-identical UV-K1 radios + centered right-pointing arrow
 *   - y=39/47:  Spacious two-line centered status (zero cramped list boxes)
 *   - y=54..60: Native square soft bar ([MENU] Start / [EXIT] Back)
 */
#include "dfu_shared.h"

static const u8 PEER_RLE[96] = {
    0xFD, 0x00, 0x02, 0xFC, 0xFE, 0xFC, 0xF3, 0x00, 0x01, 0x80, 0x80, 0xDC, 0x00, 0x13, 0xE0, 0xF8,
    0xFC, 0xFE, 0xFF, 0x0F, 0x0F, 0x0C, 0xCC, 0x4C, 0x8C, 0x0C, 0xCC, 0x4C, 0x4C, 0x0C, 0xCC, 0x0C,
    0xCC, 0x0C, 0xFE, 0x0F, 0x03, 0xFF, 0xFC, 0xFC, 0xF8, 0xE0, 0x00, 0x00, 0x67, 0xFD, 0xFF, 0xFE,
    0xE0, 0x0A, 0xE7, 0xE4, 0xE3, 0xE0, 0xE7, 0xE1, 0xE0, 0xE0, 0xE7, 0xE4, 0xE7, 0xFD, 0xE0, 0xFD,
    0xFF, 0xF3, 0x00, 0x00, 0x70, 0xF5, 0xF8, 0x07, 0xFF, 0xFE, 0xFC, 0xF8, 0x70, 0x20, 0x00, 0x7F,
    0xFD, 0xFF, 0xF1, 0xF6, 0xFD, 0xFF, 0x00, 0x7F, 0xE6, 0x00, 0x02, 0x07, 0x03, 0x01, 0xFE, 0x00,
};

static void draw_peer_art(u8* fb, u8* tmp, u8 show_arrow) {
    u32 si = 0u, di = 0u;
    while (si < sizeof(PEER_RLE) && di < 240u) {
        u8 h = PEER_RLE[si++];
        if (h < 128u) {
            u32 c = (u32)h + 1u;
            while (c-- && si < sizeof(PEER_RLE) && di < 240u) tmp[di++] = PEER_RLE[si++];
        } else {
            u32 c = 257u - (u32)h;
            u8 v = (si < sizeof(PEER_RLE)) ? PEER_RLE[si++] : 0u;
            while (c-- && di < 240u) tmp[di++] = v;
        }
    }
    for (u32 p = 0u; p < 4u; p++) {
        dfu_copy_bytes(fb + (p << 7) + 13u, tmp + p * 60u, show_arrow ? 60u : 27u);
        dfu_copy_bytes(fb + (p << 7) + 88u, tmp + p * 60u, 27u);
    }
    /* Left source radio LCD: clean 3-bar icon; right peer radio keeps "DFU" */
    for (u32 x = 21u; x <= 32u; x++) {
        fb[128u + x] = (u8)((fb[128u + x] & 0x0Fu) | ((x > 21u && x < 32u) ? 0x40u : 0u));
        fb[256u + x] = (u8)((fb[256u + x] & 0xE0u) | ((x > 21u && x < 30u) ? 0x05u : 0u));
    }
}

__attribute__((noinline))
static void draw_ctr(const KrxApiTable* api, i16 y, const char* s) {
    krx_draw_str_align(api, 64, y, (const u8*)s, (u16)krx_strlen(s),
                       KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
}

__attribute__((noinline))
static void render_peer(const KrxApiTable* api, DfuSharedState* st, u8 waiting, u8 arrow) {
    krx_clear(api);
    draw_peer_art(krx_get_framebuffer(api), st->stage_buf, arrow);
    draw_ctr(api, 39, waiting ? "Waiting for peer..." : "Peer Flash");
    draw_ctr(api, 47, waiting ? "Hold PTT + power on" : "Connect 2-pin cable");
    if (waiting) {
        st->ui.softbar(api, "", "", "EXIT", "Cancel");
    } else {
        st->ui.softbar(api, "MENU", "Start", "EXIT", "Back");
    }
    krx_commit_frame(api);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf8_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    for (;;) {
        render_peer(api, st, 0u, 1u);
        u8 k = st->ui.wait_key(api);
        if (k == KEY_EXIT) return 0u;
        if (k == KEY_MENU) {
            while (krx_poll_key(api) == KEY_MENU) krx_delay_ms(api, 10u);
            u8 arrow = 1u;
            for (u32 tick = 0u; ; tick++) {
                if ((tick & 31u) == 0u) {
                    render_peer(api, st, 1u, arrow);
                    arrow ^= 1u;
                }
                if (krx_poll_key(api) == KEY_EXIT) {
                    while (krx_poll_key(api) == KEY_EXIT) krx_delay_ms(api, 10u);
                    break;
                }
                krx_delay_ms(api, 15u);
            }
        }
    }
}
