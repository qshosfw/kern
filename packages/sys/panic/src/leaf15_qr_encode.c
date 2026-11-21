/*
 * P0 Leaf 15: QR Payload Formatter, Alphanumeric Bitstream Packer & RS(44,34) ECC (<= 1 KB).
 */
#include "panic_shared.h"

static u8 gf_mul(u8 a, u8 b) {
    u8 r = 0u;
    for (u8 i = 0; i < 8u; i++) {
        if (b & 1u) r ^= a;
        u8 hi = a & 0x80u;
        a <<= 1;
        if (hi) a ^= 0x1Du;
        b >>= 1;
    }
    return r;
}

static u8 qr_alnum(char ch) {
    if (ch >= '0' && ch <= '9') return (u8)(ch - '0');
    if (ch >= 'A' && ch <= 'Z') return (u8)(ch - 'A' + 10);
    if (ch >= 'a' && ch <= 'z') return (u8)(ch - 'a' + 10);
    if (ch == '/') return 43u;
    if (ch == ':') return 44u;
    return 36u;
}

static void push_bits(u8* cw, u16* bpos, u32 val, u8 nb) {
    for (i8 k = (i8)(nb - 1u); k >= 0; k--) {
        if ((val >> (u8)k) & 1u) {
            u16 p = *bpos;
            cw[p >> 3] |= (u8)(0x80u >> (p & 7u));
        }
        (*bpos)++;
    }
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf15_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1;
    PanicState* st = (PanicState*)ctx->arg;
    const BootSlot* s = &st->cur_slot;

    char txt[48];
    const char* rsn = "PNC";
    if (s->reset_reason == REASON_HARD) rsn = "HRD";
    else if (s->reset_reason == REASON_WDOG) rsn = "WDG";
    else if (s->reset_reason == REASON_BROW) rsn = "BRW";
    else if (s->reset_reason == REASON_TAGS) rsn = "TAG";

    u8 len = p_app_s(txt, 0u, "K:");
    len = p_app_s(txt, len, rsn);
    txt[len++] = '/'; len += p_put_hex(txt + len, s->fault_pc, 8u);
    txt[len++] = '/'; len += p_put_hex(txt + len, s->fault_lr, 8u);
    txt[len++] = '/'; len += p_put_hex(txt + len, s->fault_sp, 8u);
    txt[len++] = '/'; len += p_put_hex(txt + len, st->ana.crash_sig, 8u);
    txt[len++] = '/'; len += p_put_hex(txt + len, s->panic_line, 4u);

    u8* cw = st->qr_cw;
    for (u8 i = 0; i < 44u; i++) cw[i] = 0u;
    u16 bpos = 0u;

    push_bits(cw, &bpos, 2u, 4u);
    push_bits(cw, &bpos, len, 9u);
    u8 idx = 0u;
    while ((u8)(idx + 1u) < len) {
        u16 pair = (u16)qr_alnum(txt[idx]) * 45u + (u16)qr_alnum(txt[idx + 1u]);
        push_bits(cw, &bpos, pair, 11u);
        idx += 2u;
    }
    if (idx < len) push_bits(cw, &bpos, qr_alnum(txt[idx]), 6u);
    if (bpos + 4u <= 272u) bpos += 4u;
    while (bpos & 7u) bpos++;
    u8 pad = 0u;
    while ((bpos >> 3) < 34u) {
        cw[bpos >> 3] = (pad & 1u) ? 0x11u : 0xECu;
        pad ^= 1u;
        bpos += 8u;
    }

    static const u8 GEN[10] = { 216, 194, 159, 111, 199, 94, 95, 113, 157, 193 };
    for (u8 i = 0; i < 34u; i++) {
        u8 fb = cw[i] ^ cw[34];
        for (u8 j = 0; j < 9u; j++) cw[34u + j] = cw[35u + j] ^ gf_mul(GEN[j], fb);
        cw[43] = gf_mul(GEN[9], fb);
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0;
    ctx->continuation.payload = 0;
    return 0;
}
