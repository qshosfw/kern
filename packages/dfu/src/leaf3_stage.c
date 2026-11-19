/*
 * DFU Leaf 3: Key-Release Guard, Two-Sided Soft Bar & SRAM Strip Pre-Renderer
 *
 * Renders:
 *   - Page 7 two-sided soft bar: `[EXIT] Reboot` (left) · `[MENU] Tools` (right)
 *   - Page 5 status banners (8 x 84 B = 672 B, strictly inside Page 5, baseline y=47)
 *   - Pages 2-3 connection arrow (2 x 19 B = 38 B, x=49..67)
 *   - Page 1 USB VID:PID link badge (40 B, x=76..115, centered at x=96)
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
    dfu_zero_bytes(fb + (5u << 7), 128u);
    krx_draw_str_align(api, x, 47, (const u8*)s, len, KRX_FONT_TOMTHUMB, 1u, align);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf3_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    st->ui.softbar(api, "MENU", "Tools", "EXIT", "Reboot");

    if (st->keys_held) {
        static const char rel[] = "Release side key...";
        clear_draw_page5(api, 64, KRX_ALIGN_CENTER, rel, sizeof(rel) - 1u);
        krx_commit_frame(api);
        while (((*(const volatile u32*)0x50000410u) & 0xC000u) != 0xC000u) {
            krx_delay_ms(api, 10u);
        }
        krx_delay_ms(api, 20u);
    }

    clear_draw_page5(api, 64, KRX_ALIGN_CENTER, (const char*)(DFU_BANNERS_PACKED + 1), 13u);
    krx_commit_frame(api);

    if (st->kbhp.bl_kind == KBHP_BL_STOCK_K1) {
        u8* fb = krx_get_framebuffer(api);
        const u8* p = DFU_BANNERS_PACKED;
        for (u8 idx = 0; idx < 8u; idx++) {
            u32 len = *p++;
            clear_draw_page5(
                api,
                (idx == 4u) ? (i16)DFU_FLASH_PREFIX_X : 64,
                (idx == 4u) ? KRX_ALIGN_LEFT : KRX_ALIGN_CENTER,
                (const char*)p,
                (u16)len
            );
            p += len;
            dfu_copy_bytes(st->buf.sram_banners + (u32)idx * 84u, fb + (5u << 7) + 22u, 84u);
        }
        for (u32 page = 0; page < 2u; page++) {
            dfu_copy_bytes(st->sram_arrow + page * 19u, fb + ((2u + page) << 7) + 49u, 19u);
        }
        char vp[9];
        u32 tv = st->kbhp.bl_vid_pid;
        for (u32 j = 0; j < 9u; j++) {
            if (j == 4u) {
                vp[j] = ':';
            } else {
                u8 nib = (u8)(tv >> 28);
                tv <<= 4;
                vp[j] = (char)(nib + (nib < 10u ? '0' : ('A' - 10)));
            }
        }
        krx_draw_str_align(api, 96, 13, (const u8*)vp, 9u, KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
        dfu_copy_bytes(st->sram_vidpid, fb + (1u << 7) + 76u, 40u);
        st->has_stage = 1u;
        st->kbhp.flags |= KBHP_FLAG_STAGE_ASSETS_OK;
        st->kbhp.header_checksum = kbhp_checksum(&st->kbhp);
    } else {
        st->has_stage = 0u;
    }
    return 0u;
}
