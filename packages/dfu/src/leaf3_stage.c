/*
 * DFU Leaf 3: Key-release wait, Live Status Banner & VID:PID SRAM Pre-renderer
 */
#include "dfu_shared.h"

static const u8 DFU_BANNERS_PACKED[] =
    "\x0d" "Plug in cable"
    "\x0d" "USB connected"
    "\x0e" "Ready to flash"
    "\x0b" "Link active"
    "\x0e" "Flashing page "
    "\x0e" "Flash complete"
    "\x0e" "UART connected"
    "\x11" "Aborted - reflash";

static void clear_draw_page5(const KrxApiTable* api, i16 x, u8 align, const char* s, u16 len) {
    u8* fb = krx_get_framebuffer(api);
    for (u32 c = 0; c < 128u; c++) {
        fb[(5u << 7) + c] &= 0x03u;
        fb[(6u << 7) + c] &= 0xF8u;
    }
    krx_draw_str_align(api, x, 49, (const u8*)s, len, KRX_FONT_TOMTHUMB, 1u, align);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf3_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    static const char pre[] = "Press ";
    static const char key[] = "EXIT";
    static const char post[] = "to reboot";
    const i16 x0 = (128 - (24 + 20 + 5 + 36)) >> 1;
    const i16 bx = x0 + 24;
    krx_draw_str_ex(api, x0, 58, (const u8*)pre, sizeof(pre) - 1, KRX_FONT_TOMTHUMB, 1u);
    krx_fill_rect(api, bx, 53, 20, 7, 1u);
    krx_draw_str_ex(api, bx + 2, 58, (const u8*)key, sizeof(key) - 1, KRX_FONT_TOMTHUMB, 0u);
    krx_draw_str_ex(api, bx + 25, 58, (const u8*)post, sizeof(post) - 1, KRX_FONT_TOMTHUMB, 1u);

    if (st->keys_held) {
        static const char rel[] = "Release side key...";
        clear_draw_page5(api, 64, KRX_ALIGN_CENTER, rel, sizeof(rel) - 1);
        krx_commit_frame(api);
        while (((*(const volatile u32*)0x50000410u) & 0xC000u) != 0xC000u) {
            krx_delay_ms(api, 10u);
        }
        krx_delay_ms(api, 20u);
    }

    clear_draw_page5(api, 64, KRX_ALIGN_CENTER, (const char*)(DFU_BANNERS_PACKED + 1), 13);
    krx_commit_frame(api);

    if (st->vid_pid == 0x36B70001u) {
        u8* fb = krx_get_framebuffer(api);
        const u8* p = DFU_BANNERS_PACKED;
        for (u8 idx = 0; idx < 8u; idx++) {
            u32 len = *p++;
            clear_draw_page5(api, (idx == 4u) ? (i16)DFU_FLASH_PREFIX_X : 64,
                             (idx == 4u) ? KRX_ALIGN_LEFT : KRX_ALIGN_CENTER,
                             (const char*)p, (u16)len);
            p += len;
            u8* dst = st->sram_banners + (u32)idx * 168u;
            dfu_copy_bytes(dst, fb + (5u << 7) + 22u, 84u);
            dfu_copy_bytes(dst + 84u, fb + (6u << 7) + 22u, 84u);
        }
        for (u32 page = 0; page < 2u; page++) {
            dfu_copy_bytes(st->sram_arrow + page * 19u, fb + ((2u + page) << 7) + 45u, 19u);
        }
        char vp[9];
        u32 tv = st->vid_pid;
        for (u32 j = 0; j < 9u; j++) {
            if (j == 4u) vp[j] = ':';
            else { u8 nib = (u8)(tv >> 28); tv <<= 4; vp[j] = (char)(nib + (nib < 10 ? '0' : ('A' - 10))); }
        }
        krx_draw_str_align(api, 93, 13, (const u8*)vp, 9, KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
        dfu_copy_bytes(st->sram_vidpid, fb + (1u << 7) + 73u, 40u);
        st->has_stage = 1u;
    } else {
        st->has_stage = 0u;
    }
    return 0;
}
