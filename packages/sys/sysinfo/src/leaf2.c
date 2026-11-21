/*
 * P0 Leaf 2: Flash-resident Quansheng QS-1 AES-128-CBC Verifier (< 1 KB).
 * Invoked once at KRX_OP_START to verify 0x0800_2700..0x0800_2767 against
 * the 16-byte MCU UID at 0x1FFF_3000..0x1FFF_300F without bloating firmware Flash.
 */

#include "sysinfo_shared.h"

static const u8 QS_SEED[16] = {
    0x7d, 0x18, 0x20, 0x7f, 0x0e, 0xd1, 0x41, 0xee,
    0xa3, 0x4d, 0x29, 0x2e, 0x84, 0xf2, 0xa8, 0x71
};

static const u8 RCON[10] = {
    0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36
};

static u8 gf_xtime(u8 x) {
    return (u8)((x << 1) ^ ((x & 0x80u) ? 0x1bu : 0u));
}

static u8 gf_mul(u8 a, u8 b) {
    u8 r = 0u;
    for (u8 i = 0u; i < 8u; i++) {
        if (b & 1u) r ^= a;
        a = gf_xtime(a);
        b >>= 1u;
    }
    return r;
}

static u8 rotl8(u8 x, u8 n) {
    return (u8)((x << n) | (x >> (8u - n)));
}

static u8 aes_sbox(u8 x) {
    u8 inv = 1u, p = x;
    for (u8 i = 0u; i < 8u; i++) {
        if (i > 0u) inv = gf_mul(inv, p);
        p = gf_mul(p, p);
    }
    if (x == 0u) inv = 0u;
    return (u8)(inv ^ rotl8(inv, 1u) ^ rotl8(inv, 2u) ^ rotl8(inv, 3u) ^ rotl8(inv, 4u) ^ 0x63u);
}

static void rev4(u8* dst, const u8* src) {
    dst[0] = src[3];
    dst[1] = src[2];
    dst[2] = src[1];
    dst[3] = src[0];
}

static void aes128_cbc_80(const u8* key, const u8* iv, u8* blocks) {
    u8 rk[11][16];
    for (u8 i = 0u; i < 16u; i++) rk[0][i] = key[i];
    for (u8 r = 0u; r < 10u; r++) {
        for (u8 i = 0u; i < 16u; i++) rk[r + 1u][i] = rk[r][i];
        rk[r + 1u][0] ^= aes_sbox(rk[r][13]) ^ RCON[r];
        rk[r + 1u][1] ^= aes_sbox(rk[r][14]);
        rk[r + 1u][2] ^= aes_sbox(rk[r][15]);
        rk[r + 1u][3] ^= aes_sbox(rk[r][12]);
        for (u8 i = 4u; i < 16u; i++) rk[r + 1u][i] ^= rk[r + 1u][i - 4u];
    }

    u8 prev[16];
    for (u8 i = 0u; i < 16u; i++) prev[i] = iv[i];

    for (u8 b = 0u; b < 5u; b++) {
        u8* blk = blocks + (u32)b * 16u;
        u8 s[16];
        for (u8 i = 0u; i < 16u; i++) s[i] = (u8)(blk[i] ^ prev[i] ^ rk[0][i]);

        for (u8 rnd = 1u; rnd <= 10u; rnd++) {
            for (u8 i = 0u; i < 16u; i++) s[i] = aes_sbox(s[i]);

            u8 t = s[1]; s[1] = s[5]; s[5] = s[9]; s[9] = s[13]; s[13] = t;
            t = s[2]; s[2] = s[10]; s[10] = t;
            t = s[6]; s[6] = s[14]; s[14] = t;
            t = s[15]; s[15] = s[11]; s[11] = s[7]; s[7] = s[3]; s[3] = t;

            if (rnd < 10u) {
                for (u8 c = 0u; c < 4u; c++) {
                    u8 i = (u8)(c * 4u);
                    u8 a0 = s[i], a1 = s[i + 1u], a2 = s[i + 2u], a3 = s[i + 3u];
                    u8 all = (u8)(a0 ^ a1 ^ a2 ^ a3);
                    s[i]      = (u8)(a0 ^ all ^ gf_xtime((u8)(a0 ^ a1)));
                    s[i + 1u] = (u8)(a1 ^ all ^ gf_xtime((u8)(a1 ^ a2)));
                    s[i + 2u] = (u8)(a2 ^ all ^ gf_xtime((u8)(a2 ^ a3)));
                    s[i + 3u] = (u8)(a3 ^ all ^ gf_xtime((u8)(a3 ^ a0)));
                }
            }
            for (u8 i = 0u; i < 16u; i++) s[i] ^= rk[rnd][i];
        }
        for (u8 i = 0u; i < 16u; i++) {
            blk[i] = s[i];
            prev[i] = s[i];
        }
    }
}

static u32 le32(const u8* p) {
    return (u32)p[0] | ((u32)p[1] << 8) | ((u32)p[2] << 16) | ((u32)p[3] << 24);
}

static u32 be32(const u8* p) {
    return ((u32)p[0] << 24) | ((u32)p[1] << 16) | ((u32)p[2] << 8) | (u32)p[3];
}

__attribute__((section(".text.entry")))
u32 leaf2_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    (void)api;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx) return 0u;

    const volatile u8* flash_cert = (const volatile u8*)0x08002700u;
    const volatile u8* flash_uid  = (const volatile u8*)0x1FFF3000u;

    u8 cert[104], uid16[16];
    u8 erased = 1u;
    for (u8 i = 0u; i < 104u; i++) {
        cert[i] = flash_cert[i];
        if (cert[i] != 0xFFu) erased = 0u;
    }
    for (u8 i = 0u; i < 16u; i++) uid16[i] = flash_uid[i];

    u32 ok = 0u;
    if (!erased) {
        u8 buf[64];
        for (u8 i = 0u; i < 10u; i++) {
            for (u8 b = 0u; b < 4u; b++) buf[4u * i + b] = cert[8u * i + b];
        }
        for (u8 i = 0u; i < 24u; i++) buf[40u + i] = cert[80u + i];

        u8 key[16], iv[16], ct[80];
        rev4(key + 0u,  QS_SEED + 0u);
        rev4(key + 4u,  QS_SEED + 4u);
        rev4(key + 8u,  uid16 + 0u);
        rev4(key + 12u, uid16 + 8u);

        rev4(iv + 0u,   QS_SEED + 8u);
        rev4(iv + 4u,   QS_SEED + 12u);
        rev4(iv + 8u,   uid16 + 4u);
        rev4(iv + 12u,  uid16 + 12u);

        for (u8 c = 0u; c < 4u; c++) {
            for (u8 w = 0u; w < 4u; w++) {
                rev4(ct + 20u * c + 4u * w, buf + 16u * c + 4u * w);
            }
            rev4(ct + 20u * c + 16u, uid16 + 4u * c);
        }

        aes128_cbc_80(key, iv, ct);

        ok = 1u;
        for (u8 i = 0u; i < 10u; i++) {
            u32 odd = be32(cert + 8u * i + 4u);
            u32 a = le32(ct + 4u * i);
            u32 b = le32(ct + 4u * (19u - i));
            if (odd != (a ^ b)) {
                ok = 0u;
                break;
            }
        }
    }

    ctx->continuation.kind = KRX_LEAF_KIND_DONE;
    ctx->continuation.target_or_code = 0u;
    ctx->continuation.payload = ok;
    return ok;
}
