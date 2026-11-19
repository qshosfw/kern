/*
 * DFU Leaf 1: Splash RLE, ROM VID:PID/Date parser & Initial Screen UI
 */
#include "dfu_shared.h"

static const u8 DFU_SPLASH_RLE[168] = {
    0xF1, 0x00, 0x03, 0xE0, 0xF0, 0xF0, 0xE0, 0x8B, 0x00, 0x02, 0xF8, 0xF8, 0xFC, 0xFE, 0xFE, 0xFE,
    0x07, 0x0A, 0xE7, 0x26, 0xC6, 0x06, 0xE6, 0xA6, 0xA6, 0x06, 0xE6, 0x06, 0xE6, 0xFD, 0x06, 0xFE,
    0xFE, 0x00, 0xFC, 0x9D, 0x00, 0x01, 0x07, 0x07, 0xFD, 0xFF, 0x07, 0xF0, 0xF0, 0x70, 0x73, 0x72,
    0x71, 0x70, 0x73, 0xFE, 0x70, 0x06, 0x73, 0x72, 0x73, 0x70, 0x70, 0xF0, 0xF0, 0xFD, 0xFF, 0xF9,
    0x00, 0x07, 0x80, 0xC0, 0xE0, 0xF0, 0xF8, 0xFC, 0xFC, 0xF8, 0xF8, 0xE0, 0x00, 0xC0, 0xFA, 0x00,
    0x00, 0xE0, 0xF9, 0xF0, 0x02, 0x00, 0xF8, 0xFC, 0xED, 0xFE, 0x02, 0xFC, 0xF8, 0x00, 0xFD, 0xF0,
    0xEF, 0xC0, 0xF5, 0x00, 0x00, 0x7F, 0xFC, 0xFF, 0xF3, 0xFB, 0xFC, 0xFF, 0x00, 0x7F, 0xFA, 0x00,
    0x08, 0x01, 0x03, 0x07, 0x0F, 0x1F, 0x3F, 0x7F, 0x7F, 0x3F, 0xF8, 0x0F, 0x00, 0x07, 0xFA, 0x00,
    0x00, 0x0F, 0xF9, 0x1F, 0x02, 0x00, 0x3F, 0x7F, 0xED, 0xFF, 0x02, 0x7F, 0x3F, 0x00, 0xFD, 0x1F,
    0xEF, 0x07, 0xF5, 0x00, 0x00, 0xE0, 0x9B, 0x10, 0x00, 0xE0, 0xE9, 0x00, 0x00, 0x01, 0x9B, 0x02,
    0x00, 0x01, 0x81, 0x00, 0x81, 0x00, 0xF5, 0x00,
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

static u32 read_bl_rom_meta(u8 date_out[8]) {
    for (u32 i = 0; i < 8; i++) date_out[i] = 0;
    const u8* rom = (const u8*)0x08001F00u;
    const u32 rom_len = 0x0800u;
    u32 vid_pid = 0x36B70001u;
    u8 i_serial = 0;
    u32 i = 0;

    while (i + 18u <= rom_len) {
        if (rom[i] == 0x12 && rom[i + 1] == 0x01 &&
            rom[i + 2] == 0x00 && rom[i + 3] == 0x02 && rom[i + 7] == 0x40) {
            vid_pid = (((u32)rom[i + 9] << 24) | ((u32)rom[i + 8] << 16) |
                       ((u32)rom[i + 11] << 8) | rom[i + 10]);
            i_serial = rom[i + 16];
            i += 18u;
            break;
        }
        i++;
    }
    if (i_serial == 0) return vid_pid;

    while (i + 4u <= rom_len) {
        if (rom[i] == 0x04 && rom[i + 1] == 0x03 && rom[i + 2] == 0x09 && rom[i + 3] == 0x04) {
            u32 pos = i + 4u;
            u8 idx = 1;
            while (pos + 2u <= rom_len) {
                u32 len = rom[pos];
                if (len < 2u || (len & 1u) != 0 || rom[pos + 1] != 0x03 || pos + len > rom_len) break;
                if (idx == i_serial && len >= 18u) {
                    u8 ok = 1;
                    for (u32 k = 0; k < 8u; k++) {
                        u8 lo = rom[pos + 2u + k * 2u];
                        if (rom[pos + 3u + k * 2u] != 0 || lo < '0' || lo > '9') { ok = 0; break; }
                        date_out[k] = lo;
                    }
                    if (!ok || date_out[0] != '2') for (u32 k = 0; k < 8u; k++) date_out[k] = 0;
                    break;
                }
                pos += len;
                idx++;
            }
            break;
        }
        i++;
    }
    return vid_pid;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf1_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    krx_wake_backlight(api);
    *(volatile u32*)0x50001418u = 1u << 8;

    st->keys_held = ((*(const volatile u32*)0x50000410u) & 0xC000u) != 0xC000u;
    char bl_ver[8];
    dfu_read_bl_version(bl_ver);
    u8 bl_date[8];
    st->vid_pid = read_bl_rom_meta(bl_date);

    u8* fb = krx_get_framebuffer(api);
    decode_rle_1024(fb, DFU_SPLASH_RLE, sizeof(DFU_SPLASH_RLE));

    char line[32];
    line[0] = 'v';
    dfu_copy_bytes((u8*)(line + 1), (const u8*)bl_ver, 7);
    u32 hdr_len = 8u;
    if (bl_date[0] != 0) {
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
    if ((i16)krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u) > 84) tr.len = 8u;
    tr.flags = 0u;
    krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u);
    return 0;
}
