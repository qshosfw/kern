/*
 * DFU Leaf 3: Key-Release Guard, Square Soft Bar & Double-Line Strip Pre-Renderer
 *
 * Renders and stages into TRANSIENT_ARENA[4160..5120) (`st->stage_buf`):
 *   - Page 4 primary titles (5 x 64 B = 320 B, baseline y=38, cols 32..95)
 *     shown when progress bar is hidden (states 0, 1, 2, 3, 6)
 *   - Page 5 status subtitles (8 x 80 B = 640 B, baseline y=46, cols 24..103)
 *     100% inside Page 5 including lowercase descenders (zero Page 6 bleed)
 *   - Pages 2-3 connection arrow (2 x 19 B = 38 B, cols 49..67)
 *   - Page 1 USB VID:PID link badge (40 B, cols 76..115, centered at x=96)
 */
#include "dfu_shared.h"

/* 5 agnostic Page-4 titles for non-progress-bar states: 0, 1, 2, 3, 6 */
static const u8 DFU_TITLES_PACKED[] =
    "\x08" "DFU Mode"
    "\x0d" "USB connected"
    "\x0a" "Host ready"
    "\x0b" "Link active"
    "\x10" "Serial connected";

/* 8 agnostic Page-5 subtitles / transfer statuses for states 0..=7 */
static const u8 DFU_BANNERS_PACKED[] =
    "\x15" "Plug in USB or serial"
    "\x13" "Waiting for host..."
    "\x12" "Ready for transfer"
    "\x12" "Exchanging data..."
    "\x0d" "Writing page "
    "\x11" "Transfer complete"
    "\x15" "Ready on 2-pin serial"
    "\x0f" "Aborted - retry";

static void draw_page_line(const KrxApiTable* api, u8 page, i16 x, i16 y, u8 align, const char* s, u16 len) {
    u8* fb = krx_get_framebuffer(api);
    dfu_zero_bytes(fb + ((u32)page << 7), 128u);
    krx_draw_str_align(api, x, y, (const u8*)s, len, KRX_FONT_TOMTHUMB, 1u, align);
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf3_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    st->ui.softbar(api, "MENU", "Tools", "EXIT", "Reboot");
    draw_page_line(api, 4u, 64, 38, KRX_ALIGN_CENTER, (const char*)(DFU_TITLES_PACKED + 1), 8u);

    if (st->keys_held) {
        static const char rel[] = "Release side key...";
        draw_page_line(api, 5u, 64, 46, KRX_ALIGN_CENTER, rel, sizeof(rel) - 1u);
        krx_commit_frame(api);
        while (((*(const volatile u32*)0x50000410u) & 0xC000u) != 0xC000u) {
            krx_delay_ms(api, 10u);
        }
        krx_delay_ms(api, 20u);
    }

    draw_page_line(api, 5u, 64, 46, KRX_ALIGN_CENTER, (const char*)(DFU_BANNERS_PACKED + 1), 21u);
    krx_commit_frame(api);

    if (st->kbhp.bl_kind == KBHP_BL_STOCK_K1) {
        u8* fb = krx_get_framebuffer(api);
        u8* stg = st->stage_buf;
        const u8* p = DFU_BANNERS_PACKED;
        for (u8 idx = 0u; idx < 8u; idx++) {
            u32 len = *p++;
            draw_page_line(
                api, 5u,
                (idx == 4u) ? (i16)DFU_FLASH_PREFIX_X : 64, 46,
                (idx == 4u) ? KRX_ALIGN_LEFT : KRX_ALIGN_CENTER,
                (const char*)p, (u16)len
            );
            p += len;
            dfu_copy_bytes(stg + (u32)idx * 80u, fb + (5u << 7) + 24u, 80u);
        }
        p = DFU_TITLES_PACKED;
        for (u8 idx = 0u; idx < 5u; idx++) {
            u32 len = *p++;
            draw_page_line(api, 4u, 64, 38, KRX_ALIGN_CENTER, (const char*)p, (u16)len);
            p += len;
            dfu_copy_bytes(stg + DFU_BANNERS_BYTES + ((u32)idx << 6), fb + (4u << 7) + 32u, 64u);
        }
        for (u32 page = 0u; page < 2u; page++) {
            dfu_copy_bytes(st->sram_arrow + page * 19u, fb + ((2u + page) << 7) + 49u, 19u);
        }
        char vp[9];
        u32 tv = st->kbhp.bl_vid_pid;
        for (u32 j = 0u; j < 9u; j++) {
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
