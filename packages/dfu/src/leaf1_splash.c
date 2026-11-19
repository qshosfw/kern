/*
 * DFU Leaf 1: Splash RLE Decompressor & Bootloader-Aware Top Header
 */
#include "dfu_shared.h"

static const u8 DFU_SPLASH_RLE[141] = {
    0xF0, 0x00, 0x02, 0xFC, 0xFE, 0xFC, 0xF6, 0x00, 0x05, 0x80, 0xC0, 0x40, 0xC0, 0xC0, 0x80, 0x9A,
    0x00, 0x05, 0xE0, 0xF0, 0xF8, 0xFC, 0xFF, 0xFF, 0xFE, 0x0F, 0x0A, 0xEC, 0x2C, 0xCC, 0x0C, 0xEC,
    0xAC, 0x2C, 0x0C, 0xEC, 0x0C, 0xEF, 0xFD, 0x0F, 0x03, 0xFF, 0xFC, 0xFC, 0xF8, 0x9D, 0x00, 0x01,
    0x03, 0x77, 0xFD, 0xFF, 0xFE, 0xB8, 0x04, 0xBB, 0xBA, 0xB9, 0xB8, 0xBB, 0xFE, 0xB8, 0x02, 0xBB,
    0xBA, 0xBB, 0xFD, 0xB8, 0xFD, 0xFF, 0xF8, 0x00, 0x05, 0x20, 0x70, 0xF8, 0xFC, 0xFE, 0xFF, 0xF5,
    0xF8, 0x00, 0x70, 0xF8, 0x00, 0x00, 0x70, 0xFA, 0xF8, 0x01, 0x00, 0xFE, 0xEF, 0xFF, 0x01, 0xFE,
    0x00, 0xFD, 0xFC, 0xF0, 0x70, 0xF4, 0x00, 0x01, 0x07, 0x7F, 0xFE, 0xFF, 0xEF, 0xED, 0xFE, 0xFF,
    0x00, 0x7F, 0xF5, 0x00, 0x02, 0x01, 0x03, 0x07, 0xE2, 0x00, 0x00, 0x03, 0xEF, 0x07, 0x01, 0x03,
    0x00, 0xFD, 0x01, 0x81, 0x00, 0x81, 0x00, 0x81, 0x00, 0x81, 0x00, 0xF0, 0x00,
};

static void decode_rle_1024(u8* fb, const u8* rle, u32 rle_len) {
    u32 si = 0, di = 0;
    while (si < rle_len && di < 1024u) {
        u8 hdr = rle[si++];
        if (hdr < 128u) {
            u32 cnt = (u32)hdr + 1u;
            while (cnt-- && si < rle_len && di < 1024u) fb[di++] = rle[si++];
        } else {
            u32 cnt = 257u - (u32)hdr;
            u8 val = (si < rle_len) ? rle[si++] : 0u;
            while (cnt-- && di < 1024u) fb[di++] = val;
        }
    }
}

static void read_bl_build_date(u8 date_out[8]) {
    dfu_zero_bytes(date_out, 8u);
    const u8* rom = (const u8*)0x08001F00u;
    const u32 rom_len = 0x0800u;
    u8 i_serial = 0u;
    u32 i = 0u;
    while (i + 18u <= rom_len) {
        if (rom[i] == 0x12u && rom[i + 1] == 0x01u &&
            rom[i + 2] == 0x00u && rom[i + 3] == 0x02u && rom[i + 7] == 0x40u) {
            i_serial = rom[i + 16];
            i += 18u;
            break;
        }
        i++;
    }
    if (i_serial == 0u) return;
    while (i + 4u <= rom_len) {
        if (rom[i] == 0x04u && rom[i + 1] == 0x03u &&
            rom[i + 2] == 0x09u && rom[i + 3] == 0x04u) {
            u32 pos = i + 4u;
            u8 idx = 1u;
            while (pos + 2u <= rom_len) {
                u32 len = rom[pos];
                if (len < 2u || (len & 1u) != 0u || rom[pos + 1] != 0x03u || pos + len > rom_len) break;
                if (idx == i_serial && len >= 18u) {
                    u8 ok = 1u;
                    for (u32 k = 0; k < 8u; k++) {
                        u8 lo = rom[pos + 2u + k * 2u];
                        if (rom[pos + 3u + k * 2u] != 0u || (u8)(lo - '0') > 9u) { ok = 0u; break; }
                        date_out[k] = lo;
                    }
                    if (!ok || date_out[0] != '2') dfu_zero_bytes(date_out, 8u);
                    return;
                }
                pos += len;
                idx++;
            }
            return;
        }
        i++;
    }
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    krx_wake_backlight(api);
    *(volatile u32*)0x50001418u = 1u << 8; /* PF8 backlight ON */

    st->keys_held = ((*(const volatile u32*)0x50000410u) & 0xC000u) != 0xC000u;

    u8* fb = krx_get_framebuffer(api);
    decode_rle_1024(fb, DFU_SPLASH_RLE, sizeof(DFU_SPLASH_RLE));

    u8 bl_date[8];
    read_bl_build_date(bl_date);

    char line[32];
    u32 hdr_len = 0u;
    const char* ver = st->kbhp.bl_ver;
    if (ver[0] >= '0' && ver[0] <= '9') {
        line[hdr_len++] = 'v';
    }
    for (u32 k = 0; k < 8u && ver[k] != 0; k++) {
        line[hdr_len++] = ver[k];
    }
    u32 base_len = hdr_len;
    if (bl_date[0] != 0u) {
        line[hdr_len++] = ' '; line[hdr_len++] = '(';
        for (u32 k = 0; k < 8u; k++) {
            if (k == 4u || k == 6u) line[hdr_len++] = '-';
            line[hdr_len++] = (char)bl_date[k];
        }
        line[hdr_len++] = ')';
    }

    KrxTextReq tr;
    tr.text = (const u8*)line; tr.x = 125; tr.y = 5; tr.len = (u16)hdr_len;
    tr.arg = KRX_ALIGN_RIGHT; tr.font = KRX_FONT_TOMTHUMB; tr.color = 1u; tr.flags = 1u; tr._pad = 0u;
    if ((i16)krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u) > 84) tr.len = (u16)base_len;
    tr.flags = 0u;
    krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u);
    return 0u;
}
