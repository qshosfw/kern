/*
 * Multiboot Leaf 8: 100% Bare-Metal SRAM Page-0-Last Slot Switcher
 */
#include "mb_shared.h"

#define FLASH_A 0x40022000u

typedef struct {
    const MbState*        st;
    const KrxFileExtents* ext;
    u8  buf[256];
    u16 cur_lp;
    u32 pos;
    u32 end;
    u8  rle_rem;
    u8  rle_rep;
    u8  rle_val;
} SpiStream;

__attribute__((noinline))
static u8 stream_raw(SpiStream* s) {
    if (s->pos >= s->end) return 0xFFu;
    u16 lp = (u16)(s->pos >> 8);
    u8  in = (u8)(s->pos & 0xFFu);
    s->pos++;
    if (lp != s->cur_lp) {
        s->cur_lp = lp;
        u16 phys = 0u;
        if (s->ext->depth == 1u) {
            if (lp < 8u) phys = s->ext->l1[lp];
        } else {
            u16 idx_p = ((lp >> 7) < 8u) ? s->ext->l1[lp >> 7] : 0u;
            if (idx_p) s->st->ui.py25_read(((u32)idx_p << 8) + (((u32)(lp & 0x7Fu)) << 1), (u8*)&phys, 2u);
        }
        if (phys) s->st->ui.py25_read((u32)phys << 8, s->buf, 256u);
        else for (u32 i = 0u; i < 256u; i++) s->buf[i] = 0u;
    }
    return s->buf[in];
}

__attribute__((noinline))
static void pull(SpiStream* s, u8* dst, u32 n, u8 rle) {
    for (u32 i = 0u; i < n; i++) {
        u8 b = 0xFFu;
        if (!rle) {
            b = stream_raw(s);
        } else {
            while (!s->rle_rem && s->pos < s->end) {
                u8 c = stream_raw(s);
                if (c <= 127u) { s->rle_rep = 0u; s->rle_rem = (u8)(c + 1u); }
                else if (c >= 129u) { s->rle_rep = 1u; s->rle_rem = (u8)(257u - c); s->rle_val = stream_raw(s); }
            }
            if (s->rle_rem) { s->rle_rem--; b = s->rle_rep ? s->rle_val : stream_raw(s); }
        }
        if (dst) dst[i] = b;
    }
}

__attribute__((noinline))
static void flash_page(u32 addr, const u32* src) {
    if (addr < MB_APP_FLASH_BASE || addr >= MB_FLASH_END || (addr & 0xFFu)) return;
    volatile u32* dst = (volatile u32*)addr;
    volatile u32* fl = (volatile u32*)FLASH_A;

    for (u8 tries = 0u; tries < 2u; tries++) {
        *(volatile u32*)0x40003000u = 0xAAAAu;
        u32 diff = 0u, blank = 0xFFFFFFFFu;
        for (u32 w = 0u; w < 64u; w++) {
            u32 cur = dst[w];
            diff |= cur ^ src[w];
            blank &= cur;
        }
        if (!diff) return;

        while (fl[4] & 1u) {}
        fl[4] = 0x32u;
        if (fl[5] & 0x80000000u) { fl[2] = 0x45670123u; fl[2] = 0xCDEF89ABu; }

        if (blank != 0xFFFFFFFFu) {
            fl[5] = (1u << 1) | (1u << 24);
            *dst = 0xFFFFFFFFu;
            while (fl[4] & 1u) {}
            fl[4] = 0x32u;
            fl[5] = 0u;
        }

        fl[5] = (1u << 0) | (1u << 24);
        for (u32 w = 0u; w < 64u; w++) {
            if (w == 63u) fl[5] |= (1u << 16);
            dst[w] = src[w];
        }
        while (fl[4] & 1u) {}
        fl[4] = 0x32u;
        fl[5] = 0x80000000u;
    }
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf8_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op; (void)api;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !ctx->arg) return 1u;
    const MbState* st = (const MbState*)ctx->arg;
    MbFlashJob* job = &st->sc->job;

    __asm__ volatile("cpsid i" ::: "memory");
    *(volatile u32*)0xE000E010u = 0u;

    SpiStream s;
    s.st = st; s.ext = &job->ext; s.cur_lp = 0xFFFFu;
    s.pos = job->data_off; s.end = job->data_off + job->stored_size;
    s.rle_rem = 0u; s.rle_rep = 0u; s.rle_val = 0u;

    u32 pg[64];
    if (job->fmt == FLASH_FMT_UF2) {
        u32 nblk = job->stored_size >> 9;
        for (u32 b = 0u; b < nblk; b++) {
            u32 hdr[8];
            pull(&s, (u8*)hdr, 32u, 0u);
            pull(&s, (u8*)pg, 256u, 0u);
            pull(&s, 0, 224u, 0u);
            if (hdr[0] == 0x0A324655u && hdr[3] != MB_APP_FLASH_BASE) flash_page(hdr[3], pg);
            st->ui.lcd_bar(b + 1u, nblk + 1u);
        }
    } else {
        u32 pages = (job->raw_size + 255u) >> 8;
        if (pages > MB_APP_MAX_PAGES) pages = MB_APP_MAX_PAGES;
        u8 is_rle = (job->fmt == FLASH_FMT_KFW_RLE);
        for (u32 p = 0u; p < pages; p++) {
            pull(&s, (u8*)pg, 256u, is_rle);
            if (p) flash_page(MB_APP_FLASH_BASE + (p << 8), pg);
            st->ui.lcd_bar(p, pages);
        }
    }

    flash_page(MB_APP_FLASH_BASE, (const u32*)job->page0);
    st->ui.lcd_bar(100u, 100u);

    *(volatile u32*)0x50001428u = 1u << 8; /* PF8 backlight OFF */
    __asm__ volatile("dsb" ::: "memory");
    *(volatile u32*)0xE000ED0Cu = 0x05FA0004u;
    for (;;) { __asm__ volatile("nop"); }
}
