/*
 * P0 Leaf 1: Flash-resident Legal & Regulatory Tab Renderer (< 1 KB).
 * Content aligned with the Quansheng UV-K1 / K5 V3 User Manuals:
 * - Page 0: FCC Part 15B / 97 & CE RED 2014/53/EU marks
 * - Page 1: FCC Part 15.19 compliance statement
 * - Page 2: RF Exposure / SAR (25mm face, belt clip) & operating limits (0-40 C, <2000m)
 * - Page 3: Battery & TX safety precautions
 */

#include "sysinfo_shared.h"

// 15x11 MSB-first row-major FCC monogram bitmap (22B)
static const u8 FCC_BMP[22] = {
    0xF8, 0xFC, 0xF9, 0xFE, 0xC3, 0x9E, 0xF3, 0x3C, 0xF3, 0x30, 0xC3, 0x30,
    0xC3, 0x30, 0xC3, 0x3C, 0xC3, 0x9E, 0xC1, 0xFE, 0xC0, 0xFC
};

// 14x10 MSB-first row-major CE conformity bitmap (20B)
static const u8 CE_BMP[20] = {
    0x3C, 0x7C, 0x7C, 0xFC, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xF8, 0xC0, 0xF8,
    0xC0, 0xC0, 0xC0, 0xC0, 0x7C, 0xFC, 0x3C, 0x7C
};

static const char REG_STR[] =
    "FCC Part 15B / 97\0"
    "Class B & Amateur\0"
    "CE RED 2014/53/EU\0"
    "SAR EN 62209-1528\0"
    "FCC PART 15\0"
    "47 CFR 15.19\0"
    "Complies with FCC Part 15.\0"
    "1. No harmful interf.\0"
    "2. Accept all interf.\0"
    "Mods void user authority.\0"
    "RF SAFETY\0"
    "2014/53/EU SAR\0"
    "Hold TX ~25mm from face.\0"
    "Belt clip for body-worn.\0"
    "0-40 C, alt < 2000m.\0"
    "Valid license required.\0"
    "SAFETY\0"
    "BATTERY & TX\0"
    "Use std pack/charger.\0"
    "Never short/burn cell.\0"
    "No TX without antenna.\0"
    "Tech service only.";

static void draw_mark_card(
    const KrxApiTable* api, u8 y,
    const u8* bmp, u8 bw, u8 bh, u8 bsz,
    u8 title_idx, u8 sub_idx
) {
    const CapDisplayVTable* d = api->display;
    const char* t = si_nth(REG_STR, title_idx);
    const char* s = si_nth(REG_STR, sub_idx);
    krx_draw_rounded_box(api, 4, y, 117, 22);
    d->draw_bitmap(9, (i16)(y + (u8)((22u - bh) >> 1)), bw, bh, bmp, bsz);
    d->draw_str(28, (i16)(y + 9), (const u8*)t, si_len(t), 1, COLOR_FILL);
    d->draw_str(28, (i16)(y + 17), (const u8*)s, si_len(s), 0, COLOR_FILL);
}

__attribute__((section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !api->display) return 1;

    const CapDisplayVTable* d = api->display;
    const CapUiVTable* ui = api->ui;
    u8 page = (u8)ctx->arg;
    if (page > 3u) page = 3u;

    if (page == 0u) {
        draw_mark_card(api, 13, FCC_BMP, 15, 11, sizeof(FCC_BMP), 0, 1);
        draw_mark_card(api, 37, CE_BMP, 14, 10, sizeof(CE_BMP), 2, 3);
    } else {
        u8 base = (u8)(4u + (page - 1u) * 6u);
        const char* tag = si_nth(REG_STR, base);
        const char* sub = si_nth(REG_STR, (u8)(base + 1u));
        if (ui && ui->ui_draw_tag_badge) {
            ui->ui_draw_tag_badge(5, 13, (const u8*)tag, si_len(tag), 1);
        }
        d->draw_str_align(120, 19, (const u8*)sub, si_len(sub), 0, COLOR_FILL, 2);
        d->draw_dotted_line_h(5, 120, 24, 2, COLOR_FILL);
        for (u8 i = 0; i < 4u; i++) {
            const char* ln = si_nth(REG_STR, (u8)(base + 2u + i));
            d->draw_str(6, (i16)(33 + i * 8), (const u8*)ln, si_len(ln), 0, COLOR_FILL);
        }
    }

    if (ui && ui->ui_draw_scrollbar) {
        ui->ui_draw_scrollbar(123, 11, 51, page, 4, 1);
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
