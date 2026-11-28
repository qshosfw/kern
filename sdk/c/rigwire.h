/* SPDX-License-Identifier: GPL-3.0-or-later OR MIT
 *
 * rigwire.h — Universal Zero-Dependency Single-Header ANSI C89 Reference SDK
 *             for RigWire v2, Ascon-128 AEAD, COBS, CRC-16/32 & Kern Tables.
 *
 * Portability Guarantees:
 *   - Strict ISO C89 / ANSI C90, C99, C11, C23, and C++98..C++23 compatible.
 *   - Zero dynamic memory allocation (no malloc/free).
 *   - Zero OS or POSIX headers (only <stddef.h> and <limits.h>).
 *   - Zero 64-bit integer requirement: Ascon-128 AEAD is implemented using
 *     pure 32-bit bit-sliced halves, compiling cleanly on 16/32/64-bit CPUs
 *     (Symbian EPOC32, Nokia N900 Maemo, Win32, XNU, Linux, BSD, Android NDK,
 *     WebAssembly, AVR, MSP430, ARM Cortex-M0+).
 *   - Endian-neutral: works identically on Little-Endian and Big-Endian hosts.
 */

#ifndef KERN_RIGWIRE_H
#define KERN_RIGWIRE_H

#include <stddef.h>
#include <limits.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Portable Exact-Width Integer Types (C89 & C99+) ─────────────────────── */
#if defined(__STDC_VERSION__) && (__STDC_VERSION__ >= 199901L)
#include <stdint.h>
typedef uint8_t  rw_u8;
typedef uint16_t rw_u16;
typedef uint32_t rw_u32;
typedef int16_t  rw_i16;
typedef int32_t  rw_i32;
#else
typedef unsigned char  rw_u8;
typedef unsigned short rw_u16;
typedef short          rw_i16;
#if UINT_MAX == 0xFFFFFFFFUL
typedef unsigned int   rw_u32;
typedef int            rw_i32;
#else
typedef unsigned long  rw_u32;
typedef long           rw_i32;
#endif
#endif

typedef int rw_status;

/* Portable C89/C99/C++ single-header linkage (zero -Wunused-function warnings) */
#ifndef RW_API
#if defined(__cplusplus) || (defined(__STDC_VERSION__) && (__STDC_VERSION__ >= 199901L))
#define RW_API static inline
#elif defined(__GNUC__) || defined(__clang__)
#define RW_API __attribute__((unused)) static
#else
#define RW_API static
#endif
#endif

/* Compile-time geometry assertions valid in C89 */
typedef char rw_assert_u8_size [(sizeof(rw_u8)  == 1) ? 1 : -1];
typedef char rw_assert_u16_size[(sizeof(rw_u16) == 2) ? 1 : -1];
typedef char rw_assert_u32_size[(sizeof(rw_u32) == 4) ? 1 : -1];

/* ── Status Codes ────────────────────────────────────────────────────────── */
#define RW_OK                 0
#define RW_ERR_BUF_TOO_SMALL -1
#define RW_ERR_COBS          -2
#define RW_ERR_TOO_SHORT     -3
#define RW_ERR_BAD_IDENT     -4
#define RW_ERR_BAD_LEN       -5
#define RW_ERR_BAD_CRC       -6
#define RW_ERR_AUTH          -7
#define RW_ERR_OVERFLOW      -8

/* ── RigWire v2 Protocol Constants ───────────────────────────────────────── */
#define RW_WIRE_IDENT        0x52u
#define RW_COBS_DELIM        0x00u
#define RW_HEADER_LEN        8u
#define RW_CRC16_LEN         2u
#define RW_ASCON_KEY_LEN     16u
#define RW_ASCON_NONCE_LEN   16u
#define RW_ASCON_TAG_LEN     16u
#define RW_DEFAULT_MTU       256u
#define RW_MAX_RAW_FRAME     (RW_HEADER_LEN + 512u + RW_ASCON_TAG_LEN + RW_CRC16_LEN)
#define RW_MAX_WIRE_FRAME    (RW_MAX_RAW_FRAME + (RW_MAX_RAW_FRAME / 254u) + 2u)

/* Header Flags */
#define RW_FLAG_QOS_CRIT     0x00u
#define RW_FLAG_QOS_HIGH     0x01u
#define RW_FLAG_QOS_NORM     0x02u
#define RW_FLAG_QOS_BULK     0x03u
#define RW_FLAG_ENC          0x04u
#define RW_FLAG_AUTH         0x08u
#define RW_FLAG_NO_SEQ       0x10u
#define RW_FLAG_ACK_REQ      0x20u
#define RW_FLAG_ERR          0x40u
#define RW_FLAG_RESP         0x80u

/* Core Channels */
#define RW_CH_CORE           0x00u
#define RW_CH_OBJECT         0x01u
#define RW_CH_EVENT          0x02u
#define RW_CH_STREAM         0x03u
#define RW_CH_STORAGE        0x04u
#define RW_CH_LINK           0x05u
#define RW_CH_TIMED          0x06u
#define RW_CH_HWBUS          0x07u
#define RW_CH_DEBUG          0xFFu

/* CORE Channel Opcodes */
#define RW_OP_HELLO          0x01u
#define RW_OP_PING           0x02u
#define RW_OP_TIME_SYNC      0x03u
#define RW_OP_AUTH           0x04u
#define RW_OP_LEASE          0x05u
#define RW_OP_PORT_CTRL      0x06u
#define RW_OP_RUNLEVEL       0x07u
#define RW_OP_RESOLVE        0x08u
#define RW_OP_CAPS           0x09u
#define RW_OP_TABLES         0x0Au
#define RW_OP_BAUD           0x0Bu

/* HELLO Build/Safety Flags */
#define RW_HELLO_FLAG_DRY_TX 0x01u

/* STREAM Channel Opcodes */
#define RW_STREAM_CREDIT     0x01u
#define RW_STREAM_TILE       0x02u
#define RW_STREAM_SPEC       0x03u
#define RW_STREAM_AUDI       0x04u
#define RW_STREAM_BASE       0x05u
#define RW_STREAM_LOG        0x06u
#define RW_STREAM_TUNNEL     0x07u
#define RW_STREAM_UI_TEXT    0x08u
#define RW_STREAM_CONSOLE    0x09u

/* STREAM::AUDI Header & Flags */
#define RW_AUDIO_HDR_LEN     14u
#define RW_AUDIO_SQ_OPEN     0x01u
#define RW_AUDIO_CLIPPED     0x02u
#define RW_AUDIO_TX_DIR      0x04u
#define RW_AUDIO_SILENCE     0x08u

/* FourCC Helper (Big-Endian 32-bit Tag) */
#define RW_FOURCC(a, b, c, d) \
    ((((rw_u32)(rw_u8)(a)) << 24) | (((rw_u32)(rw_u8)(b)) << 16) | \
     (((rw_u32)(rw_u8)(c)) << 8)  |  ((rw_u32)(rw_u8)(d)))

#define RW_TABLE_CHAN        RW_FOURCC('C','H','A','N')
#define RW_TABLE_CONT        RW_FOURCC('C','O','N','T')
#define RW_TABLE_BNDP        RW_FOURCC('B','N','D','P')
#define RW_TABLE_ZONE        RW_FOURCC('Z','O','N','E')
#define RW_TABLE_HITS        RW_FOURCC('H','I','T','S')
#define RW_TABLE_FMBC        RW_FOURCC('F','M','B','C')

/* ── Endian-Neutral Byte Access Helpers ──────────────────────────────────── */
RW_API rw_u16 rw_read_u16_le(const rw_u8 *p) {
    return (rw_u16)(((rw_u16)p[0]) | (((rw_u16)p[1]) << 8));
}

RW_API void rw_write_u16_le(rw_u8 *p, rw_u16 v) {
    p[0] = (rw_u8)(v & 0xFFu);
    p[1] = (rw_u8)((v >> 8) & 0xFFu);
}

RW_API rw_u32 rw_read_u32_le(const rw_u8 *p) {
    return ((rw_u32)p[0]) | (((rw_u32)p[1]) << 8) |
           (((rw_u32)p[2]) << 16) | (((rw_u32)p[3]) << 24);
}

RW_API void rw_write_u32_le(rw_u8 *p, rw_u32 v) {
    p[0] = (rw_u8)(v & 0xFFu);
    p[1] = (rw_u8)((v >> 8) & 0xFFu);
    p[2] = (rw_u8)((v >> 16) & 0xFFu);
    p[3] = (rw_u8)((v >> 24) & 0xFFu);
}

RW_API rw_u32 rw_read_u32_be(const rw_u8 *p) {
    return (((rw_u32)p[0]) << 24) | (((rw_u32)p[1]) << 16) |
           (((rw_u32)p[2]) << 8)  | ((rw_u32)p[3]);
}

RW_API void rw_write_u32_be(rw_u8 *p, rw_u32 v) {
    p[0] = (rw_u8)((v >> 24) & 0xFFu);
    p[1] = (rw_u8)((v >> 16) & 0xFFu);
    p[2] = (rw_u8)((v >> 8) & 0xFFu);
    p[3] = (rw_u8)(v & 0xFFu);
}

RW_API void rw_zeroize(rw_u8 *buf, size_t len) {
    volatile rw_u8 *vp = (volatile rw_u8 *)buf;
    size_t i;
    for (i = 0; i < len; ++i) {
        vp[i] = 0u;
    }
}

/* ── Checksums: CRC-16/CCITT-FALSE & IEEE 802.3 CRC-32 ───────────────────── */
RW_API rw_u16 rw_crc16_ccitt(const rw_u8 *data, size_t len) {
    rw_u16 crc = 0xFFFFu;
    size_t i;
    int b;
    for (i = 0; i < len; ++i) {
        crc ^= (rw_u16)(((rw_u16)data[i]) << 8);
        for (b = 0; b < 8; ++b) {
            if ((crc & 0x8000u) != 0u) {
                crc = (rw_u16)(((crc << 1) ^ 0x1021u) & 0xFFFFu);
            } else {
                crc = (rw_u16)((crc << 1) & 0xFFFFu);
            }
        }
    }
    return crc;
}

RW_API rw_u32 rw_crc32_update(rw_u32 crc, const rw_u8 *data, size_t len) {
    static const rw_u32 lut[16] = {
        0x00000000UL, 0x1DB71064UL, 0x3B6E20C8UL, 0x26D930ACUL,
        0x76DC4190UL, 0x6B6B51F4UL, 0x4DB26158UL, 0x5005713CUL,
        0xEDB88320UL, 0xF00F9344UL, 0xD6D6A3E8UL, 0xCB61B38CUL,
        0x9B64C2B0UL, 0x86D3D2D4UL, 0xA00AE278UL, 0xBDBDF21CUL
    };
    size_t i;
    for (i = 0; i < len; ++i) {
        crc ^= (rw_u32)data[i];
        crc = (crc >> 4) ^ lut[crc & 0x0Fu];
        crc = (crc >> 4) ^ lut[crc & 0x0Fu];
    }
    return crc;
}

RW_API rw_u32 rw_crc32(const rw_u8 *data, size_t len) {
    return rw_crc32_update(0xFFFFFFFFUL, data, len) ^ 0xFFFFFFFFUL;
}

/* ── COBS Framing (Consistent Overhead Byte Stuffing) ────────────────────── */
RW_API size_t rw_cobs_max_len(size_t raw_len) {
    return raw_len + (raw_len / 254u) + 1u;
}

RW_API rw_status rw_cobs_encode(
    const rw_u8 *src, size_t src_len,
    rw_u8 *dst, size_t dst_cap, size_t *out_len
) {
    size_t read_idx = 0u;
    size_t write_idx = 1u;
    size_t code_idx = 0u;
    rw_u8 code = 1u;

    if (dst_cap < rw_cobs_max_len(src_len)) {
        return RW_ERR_BUF_TOO_SMALL;
    }

    while (read_idx < src_len) {
        rw_u8 byte = src[read_idx++];
        if (byte == 0u) {
            dst[code_idx] = code;
            code_idx = write_idx++;
            code = 1u;
        } else {
            dst[write_idx++] = byte;
            code = (rw_u8)(code + 1u);
            if (code == 0xFFu) {
                dst[code_idx] = code;
                code_idx = write_idx++;
                code = 1u;
            }
        }
    }
    dst[code_idx] = code;
    *out_len = write_idx;
    return RW_OK;
}

RW_API rw_status rw_cobs_decode_inplace(rw_u8 *buf, size_t len, size_t *out_len) {
    size_t read_idx = 0u;
    size_t write_idx = 0u;

    if (len == 0u) {
        *out_len = 0u;
        return RW_OK;
    }

    while (read_idx < len) {
        rw_u8 code = buf[read_idx++];
        rw_u8 i;
        if (code == 0u) {
            return RW_ERR_COBS;
        }
        if (read_idx + (size_t)(code - 1u) > len) {
            return RW_ERR_COBS;
        }
        for (i = 1u; i < code; ++i) {
            rw_u8 b = buf[read_idx++];
            if (b == 0u) {
                return RW_ERR_COBS;
            }
            buf[write_idx++] = b;
        }
        if (code < 0xFFu && read_idx < len) {
            buf[write_idx++] = 0u;
        }
    }
    *out_len = write_idx;
    return RW_OK;
}

/* ── Pure 32-Bit Ascon-128 AEAD (Bit-Exact with Kern src/crypto/src/ascon.rs) */
typedef struct {
    rw_u32 h[5];
    rw_u32 l[5];
} rw_ascon_state;

static void rw_ascon_sbox32(rw_u32 w[5]) {
    rw_u32 t0, t1, t2, t3, t4;
    w[0] ^= w[4];
    w[4] ^= w[3];
    w[2] ^= w[1];
    t0 = w[0] ^ ((~w[1]) & w[2]);
    t1 = w[1] ^ ((~w[2]) & w[3]);
    t2 = w[2] ^ ((~w[3]) & w[4]);
    t3 = w[3] ^ ((~w[4]) & w[0]);
    t4 = w[4] ^ ((~w[0]) & w[1]);
    w[1] = t1 ^ t0;
    w[0] = t0 ^ t4;
    w[3] = t3 ^ t2;
    w[2] = ~t2;
    w[4] = t4;
}

static void rw_ascon_rotr_pair(
    rw_u32 hi, rw_u32 lo, unsigned int r,
    rw_u32 *out_hi, rw_u32 *out_lo
) {
    rw_u32 a = (r >= 32u) ? lo : hi;
    rw_u32 b = (r >= 32u) ? hi : lo;
    unsigned int s = (r >= 32u) ? (r - 32u) : r;
    if (s == 0u) {
        *out_hi = a;
        *out_lo = b;
    } else {
        unsigned int inv = 32u - s;
        *out_hi = ((a >> s) | (b << inv)) & 0xFFFFFFFFUL;
        *out_lo = ((b >> s) | (a << inv)) & 0xFFFFFFFFUL;
    }
}

static void rw_ascon_permute(rw_ascon_state *s, unsigned int rounds) {
    static const unsigned int rot0[5] = { 19u, 61u,  1u, 10u,  7u };
    static const unsigned int rot1[5] = { 28u, 39u,  6u, 17u, 41u };
    unsigned int start = (rounds >= 12u) ? 0u : (12u - rounds);
    unsigned int r, w;

    for (r = start; r < 12u; ++r) {
        s->l[2] ^= (rw_u32)(0xF0u - r * 0x0Fu);
        rw_ascon_sbox32(s->h);
        rw_ascon_sbox32(s->l);
        for (w = 0u; w < 5u; ++w) {
            rw_u32 r0h, r0l, r1h, r1l;
            rw_ascon_rotr_pair(s->h[w], s->l[w], rot0[w], &r0h, &r0l);
            rw_ascon_rotr_pair(s->h[w], s->l[w], rot1[w], &r1h, &r1l);
            s->h[w] ^= r0h ^ r1h;
            s->l[w] ^= r0l ^ r1l;
        }
    }
}

static void rw_ascon_absorb_ad(rw_ascon_state *s, const rw_u8 *ad, size_t ad_len) {
    size_t off = 0u;
    rw_u8 xb[8];
    size_t j;

    while (off + 8u <= ad_len) {
        s->h[0] ^= rw_read_u32_be(ad + off);
        s->l[0] ^= rw_read_u32_be(ad + off + 4u);
        rw_ascon_permute(s, 6u);
        off += 8u;
    }
    rw_write_u32_be(xb, s->h[0]);
    rw_write_u32_be(xb + 4u, s->l[0]);
    for (j = 0u; j < (ad_len - off); ++j) {
        xb[j] ^= ad[off + j];
    }
    xb[ad_len - off] ^= 0x80u;
    s->h[0] = rw_read_u32_be(xb);
    s->l[0] = rw_read_u32_be(xb + 4u);
    rw_ascon_permute(s, 6u);
}

static void rw_ascon128_sponge(
    const rw_u8 key[16],
    const rw_u8 nonce[16],
    const rw_u8 *ad, size_t ad_len,
    rw_u8 *buf, size_t buf_len,
    int do_crypt, int decrypt,
    rw_u8 out_tag[16]
) {
    rw_ascon_state s;
    rw_u32 k0h = rw_read_u32_be(key);
    rw_u32 k0l = rw_read_u32_be(key + 4u);
    rw_u32 k1h = rw_read_u32_be(key + 8u);
    rw_u32 k1l = rw_read_u32_be(key + 12u);
    size_t off = 0u;

    /* ASCON128_IV = 0x80400c0600000000 */
    s.h[0] = 0x80400C06UL; s.l[0] = 0x00000000UL;
    s.h[1] = k0h;          s.l[1] = k0l;
    s.h[2] = k1h;          s.l[2] = k1l;
    s.h[3] = rw_read_u32_be(nonce);      s.l[3] = rw_read_u32_be(nonce + 4u);
    s.h[4] = rw_read_u32_be(nonce + 8u); s.l[4] = rw_read_u32_be(nonce + 12u);

    rw_ascon_permute(&s, 12u);
    s.h[3] ^= k0h; s.l[3] ^= k0l;
    s.h[4] ^= k1h; s.l[4] ^= k1l;

    if (ad_len > 0u) {
        rw_ascon_absorb_ad(&s, ad, ad_len);
    }
    s.l[4] ^= 1UL;

    for (;;) {
        size_t rem = buf_len - off;
        size_t n = (rem < 8u) ? rem : 8u;
        rw_u8 xb[8];
        size_t j;

        rw_write_u32_be(xb, s.h[0]);
        rw_write_u32_be(xb + 4u, s.l[0]);

        for (j = 0u; j < n; ++j) {
            rw_u8 b = buf[off + j];
            if (do_crypt) {
                if (decrypt) {
                    buf[off + j] = (rw_u8)(xb[j] ^ b);
                    xb[j] = b;
                } else {
                    xb[j] = (rw_u8)(xb[j] ^ b);
                    buf[off + j] = xb[j];
                }
            } else {
                xb[j] = (rw_u8)(xb[j] ^ b);
            }
        }
        if (rem >= 8u) {
            s.h[0] = rw_read_u32_be(xb);
            s.l[0] = rw_read_u32_be(xb + 4u);
            rw_ascon_permute(&s, 6u);
            off += 8u;
        } else {
            xb[n] ^= 0x80u;
            s.h[0] = rw_read_u32_be(xb);
            s.l[0] = rw_read_u32_be(xb + 4u);
            break;
        }
    }

    s.h[1] ^= k0h; s.l[1] ^= k0l;
    s.h[2] ^= k1h; s.l[2] ^= k1l;
    rw_ascon_permute(&s, 12u);
    s.h[3] ^= k0h; s.l[3] ^= k0l;
    s.h[4] ^= k1h; s.l[4] ^= k1l;

    rw_write_u32_be(out_tag,       s.h[3]);
    rw_write_u32_be(out_tag + 4u,  s.l[3]);
    rw_write_u32_be(out_tag + 8u,  s.h[4]);
    rw_write_u32_be(out_tag + 12u, s.l[4]);
}

RW_API void rw_ascon128_encrypt_inplace(
    const rw_u8 key[16], const rw_u8 nonce[16],
    const rw_u8 *ad, size_t ad_len,
    rw_u8 *buf, size_t buf_len,
    rw_u8 out_tag[16]
) {
    rw_ascon128_sponge(key, nonce, ad, ad_len, buf, buf_len, 1, 0, out_tag);
}

RW_API rw_status rw_ascon128_decrypt_inplace(
    const rw_u8 key[16], const rw_u8 nonce[16],
    const rw_u8 *ad, size_t ad_len,
    rw_u8 *buf, size_t buf_len,
    const rw_u8 expected_tag[16]
) {
    rw_u8 calc_tag[16];
    rw_u8 diff = 0u;
    size_t i;

    rw_ascon128_sponge(key, nonce, ad, ad_len, buf, buf_len, 1, 1, calc_tag);
    for (i = 0u; i < 16u; ++i) {
        diff |= (rw_u8)(calc_tag[i] ^ expected_tag[i]);
    }
    if (diff != 0u) {
        rw_zeroize(buf, buf_len);
        return RW_ERR_AUTH;
    }
    return RW_OK;
}

RW_API void rw_build_implicit_nonce(
    const rw_u8 session_salt[8],
    rw_u8 direction,
    rw_u32 key_epoch_24,
    rw_u32 seq_counter,
    rw_u8 out_nonce[16]
) {
    size_t i;
    for (i = 0u; i < 8u; ++i) {
        out_nonce[i] = session_salt[i];
    }
    out_nonce[8]  = direction;
    out_nonce[9]  = (rw_u8)((key_epoch_24 >> 16) & 0xFFu);
    out_nonce[10] = (rw_u8)((key_epoch_24 >> 8) & 0xFFu);
    out_nonce[11] = (rw_u8)(key_epoch_24 & 0xFFu);
    rw_write_u32_be(out_nonce + 12u, seq_counter);
}

/* ── RigWire v2 Header & Packet Framing ──────────────────────────────────── */
typedef struct {
    rw_u8  ident_ver;
    rw_u8  flags;
    rw_u8  channel;
    rw_u8  op;
    rw_u16 seq;
    rw_u16 len;
} rw_header;

RW_API void rw_header_pack(const rw_header *h, rw_u8 out[8]) {
    out[0] = h->ident_ver;
    out[1] = h->flags;
    out[2] = h->channel;
    out[3] = h->op;
    rw_write_u16_le(out + 4u, h->seq);
    rw_write_u16_le(out + 6u, h->len);
}

RW_API rw_status rw_header_unpack(const rw_u8 raw[8], rw_header *out) {
    if (raw[0] != RW_WIRE_IDENT) {
        return RW_ERR_BAD_IDENT;
    }
    out->ident_ver = raw[0];
    out->flags     = raw[1];
    out->channel   = raw[2];
    out->op        = raw[3];
    out->seq       = rw_read_u16_le(raw + 4u);
    out->len       = rw_read_u16_le(raw + 6u);
    return RW_OK;
}

/* Encode a complete RigWire v2 COBS + 0x00 frame (optional Ascon-128 AEAD). */
RW_API rw_status rw_encode_frame(
    rw_u8 flags, rw_u8 channel, rw_u8 op, rw_u16 seq,
    const rw_u8 *payload, rw_u16 payload_len,
    const rw_u8 *aead_key, const rw_u8 *aead_nonce,
    rw_u8 *raw_scratch, size_t raw_scratch_cap,
    rw_u8 *wire_out, size_t wire_cap, size_t *out_wire_len
) {
    rw_header hdr;
    int has_sec = ((flags & (RW_FLAG_AUTH | RW_FLAG_ENC)) != 0u) ? 1 : 0;
    size_t sec_len = has_sec ? RW_ASCON_TAG_LEN : 0u;
    size_t raw_len = RW_HEADER_LEN + (size_t)payload_len + sec_len + RW_CRC16_LEN;
    size_t cobs_len = 0u;
    rw_u16 crc;
    size_t i;
    rw_status st;

    if (raw_scratch_cap < raw_len || wire_cap < rw_cobs_max_len(raw_len) + 1u) {
        return RW_ERR_BUF_TOO_SMALL;
    }
    if (has_sec && (aead_key == NULL || aead_nonce == NULL)) {
        return RW_ERR_AUTH;
    }

    hdr.ident_ver = RW_WIRE_IDENT;
    hdr.flags     = flags;
    hdr.channel   = channel;
    hdr.op        = op;
    hdr.seq       = seq;
    hdr.len       = payload_len;
    rw_header_pack(&hdr, raw_scratch);

    for (i = 0u; i < (size_t)payload_len; ++i) {
        raw_scratch[RW_HEADER_LEN + i] = payload[i];
    }

    if ((flags & RW_FLAG_ENC) != 0u) {
        rw_ascon128_encrypt_inplace(
            aead_key, aead_nonce,
            raw_scratch, RW_HEADER_LEN,
            raw_scratch + RW_HEADER_LEN, (size_t)payload_len,
            raw_scratch + RW_HEADER_LEN + (size_t)payload_len
        );
    } else if ((flags & RW_FLAG_AUTH) != 0u) {
        rw_ascon128_sponge(
            aead_key, aead_nonce,
            raw_scratch, RW_HEADER_LEN,
            raw_scratch + RW_HEADER_LEN, (size_t)payload_len,
            0, 0,
            raw_scratch + RW_HEADER_LEN + (size_t)payload_len
        );
    }

    crc = rw_crc16_ccitt(raw_scratch, raw_len - RW_CRC16_LEN);
    rw_write_u16_le(raw_scratch + raw_len - RW_CRC16_LEN, crc);

    st = rw_cobs_encode(raw_scratch, raw_len, wire_out, wire_cap - 1u, &cobs_len);
    if (st != RW_OK) {
        return st;
    }
    wire_out[cobs_len] = RW_COBS_DELIM;
    *out_wire_len = cobs_len + 1u;
    return RW_OK;
}

/* Decode and verify an unframed COBS body in-place (excluding trailing 0x00). */
RW_API rw_status rw_decode_cobs_record_inplace(
    rw_u8 *buf, size_t cobs_len,
    const rw_u8 *aead_key, const rw_u8 *aead_nonce,
    rw_header *out_hdr, const rw_u8 **out_payload
) {
    size_t raw_len = 0u;
    int has_sec;
    size_t sec_len, expected_len;
    rw_u16 wire_crc, calc_crc;
    rw_status st = rw_cobs_decode_inplace(buf, cobs_len, &raw_len);
    if (st != RW_OK) {
        return st;
    }
    if (raw_len < RW_HEADER_LEN + RW_CRC16_LEN) {
        return RW_ERR_TOO_SHORT;
    }
    st = rw_header_unpack(buf, out_hdr);
    if (st != RW_OK) {
        return st;
    }
    has_sec = ((out_hdr->flags & (RW_FLAG_AUTH | RW_FLAG_ENC)) != 0u) ? 1 : 0;
    sec_len = has_sec ? RW_ASCON_TAG_LEN : 0u;
    expected_len = RW_HEADER_LEN + (size_t)out_hdr->len + sec_len + RW_CRC16_LEN;
    if (raw_len != expected_len) {
        return RW_ERR_BAD_LEN;
    }

    wire_crc = rw_read_u16_le(buf + raw_len - RW_CRC16_LEN);
    calc_crc = rw_crc16_ccitt(buf, raw_len - RW_CRC16_LEN);
    if (wire_crc != calc_crc) {
        return RW_ERR_BAD_CRC;
    }

    if (has_sec) {
        const rw_u8 *tag_ptr = buf + RW_HEADER_LEN + (size_t)out_hdr->len;
        if (aead_key == NULL || aead_nonce == NULL) {
            return RW_ERR_AUTH;
        }
        if ((out_hdr->flags & RW_FLAG_ENC) != 0u) {
            st = rw_ascon128_decrypt_inplace(
                aead_key, aead_nonce,
                buf, RW_HEADER_LEN,
                buf + RW_HEADER_LEN, (size_t)out_hdr->len,
                tag_ptr
            );
            if (st != RW_OK) {
                return st;
            }
        } else {
            rw_u8 calc_tag[16];
            rw_u8 diff = 0u;
            size_t i;
            rw_ascon128_sponge(
                aead_key, aead_nonce,
                buf, RW_HEADER_LEN,
                buf + RW_HEADER_LEN, (size_t)out_hdr->len,
                0, 0, calc_tag
            );
            for (i = 0u; i < 16u; ++i) {
                diff |= (rw_u8)(calc_tag[i] ^ tag_ptr[i]);
            }
            if (diff != 0u) {
                return RW_ERR_AUTH;
            }
        }
    }

    *out_payload = buf + RW_HEADER_LEN;
    return RW_OK;
}

/* ── Terminator-Only Streaming RX Collector ──────────────────────────────── */
typedef struct {
    rw_u8  buf[RW_MAX_WIRE_FRAME];
    size_t len;
    int    overflow;
} rw_rx_collector;

RW_API void rw_rx_init(rw_rx_collector *rx) {
    rx->len = 0u;
    rx->overflow = 0;
}

/* Feed 1 byte. Returns 1 when a non-empty frame is ready in rx->buf[0..*frame_len]. */
RW_API int rw_rx_push_byte(rw_rx_collector *rx, rw_u8 byte, size_t *frame_len) {
    if (byte == RW_COBS_DELIM) {
        size_t n = rx->len;
        int ovf = rx->overflow;
        rx->len = 0u;
        rx->overflow = 0;
        if (!ovf && n > 0u) {
            *frame_len = n;
            return 1;
        }
        return 0;
    }
    if (rx->overflow) {
        return 0;
    }
    if (rx->len >= RW_MAX_WIRE_FRAME) {
        rx->overflow = 1;
        rx->len = 0u;
        return 0;
    }
    rx->buf[rx->len++] = byte;
    return 0;
}

/* ── Kern Open Table Codecs: TableFileHeader (16B) & ChannelRecord (64B) ─── */
typedef struct {
    rw_u32 ident;
    rw_u16 version;
    rw_u16 record_count;
    rw_u16 max_records;
    rw_u16 record_stride;
    rw_u8  active_bank;
    rw_u8  flags;
    rw_u16 crc16;
} rw_table_header;

RW_API void rw_table_header_pack(rw_table_header *h, rw_u8 out[16]) {
    rw_write_u32_be(out, h->ident);
    rw_write_u16_le(out + 4u, h->version);
    rw_write_u16_le(out + 6u, h->record_count);
    rw_write_u16_le(out + 8u, h->max_records);
    rw_write_u16_le(out + 10u, h->record_stride);
    out[12] = h->active_bank;
    out[13] = h->flags;
    h->crc16 = rw_crc16_ccitt(out, 14u);
    rw_write_u16_le(out + 14u, h->crc16);
}

RW_API rw_status rw_table_header_unpack(const rw_u8 raw[16], rw_table_header *out) {
    rw_u16 expected = rw_crc16_ccitt(raw, 14u);
    rw_u16 stored   = rw_read_u16_le(raw + 14u);
    if (expected != stored) {
        return RW_ERR_BAD_CRC;
    }
    out->ident         = rw_read_u32_be(raw);
    out->version       = rw_read_u16_le(raw + 4u);
    out->record_count  = rw_read_u16_le(raw + 6u);
    out->max_records   = rw_read_u16_le(raw + 8u);
    out->record_stride = rw_read_u16_le(raw + 10u);
    out->active_bank   = raw[12];
    out->flags         = raw[13];
    out->crc16         = stored;
    return RW_OK;
}

/* O(1) division-free record offset: 16 + (index << stride_shift) */
RW_API rw_u32 rw_record_offset(rw_u32 index, unsigned int stride_shift) {
    return 16UL + (index << stride_shift);
}

typedef struct {
    rw_u32 rx_frequency_hz;
    rw_u32 tx_frequency_hz;
    rw_u16 step_hz_x10;
    rw_u16 bandwidth_50hz;
    rw_u8  modulation;
    rw_u8  squelch_level;
    rw_u16 rx_subaudible;
    rw_u16 tx_subaudible;
    rw_u16 zone_mask;
    rw_u16 flags;
    rw_u16 power_mw;
    rw_u16 proto_ref;
    rw_u16 uid;
    rw_u8  proto;
    rw_u8  audio_block[9];
    rw_u8  name[16];
    rw_u8  user_tag[8];
    rw_u16 record_crc;
} rw_channel_record;

RW_API void rw_channel_pack(rw_channel_record *ch, rw_u8 out[64]) {
    size_t i;
    rw_write_u32_le(out + 0x00u, ch->rx_frequency_hz);
    rw_write_u32_le(out + 0x04u, ch->tx_frequency_hz);
    rw_write_u16_le(out + 0x08u, ch->step_hz_x10);
    rw_write_u16_le(out + 0x0Au, ch->bandwidth_50hz);
    out[0x0Cu] = ch->modulation;
    out[0x0Du] = ch->squelch_level;
    rw_write_u16_le(out + 0x0Eu, ch->rx_subaudible);
    rw_write_u16_le(out + 0x10u, ch->tx_subaudible);
    rw_write_u16_le(out + 0x12u, ch->zone_mask);
    rw_write_u16_le(out + 0x14u, ch->flags);
    rw_write_u16_le(out + 0x16u, ch->power_mw);
    rw_write_u16_le(out + 0x18u, ch->proto_ref);
    rw_write_u16_le(out + 0x1Au, ch->uid);
    out[0x1Cu] = ch->proto;
    for (i = 0u; i < 9u; ++i)  { out[0x1Du + i] = ch->audio_block[i]; }
    for (i = 0u; i < 16u; ++i) { out[0x26u + i] = ch->name[i]; }
    for (i = 0u; i < 8u; ++i)  { out[0x36u + i] = ch->user_tag[i]; }
    ch->record_crc = rw_crc16_ccitt(out, 0x3Eu);
    rw_write_u16_le(out + 0x3Eu, ch->record_crc);
}

RW_API rw_status rw_channel_unpack(const rw_u8 raw[64], rw_channel_record *out) {
    size_t i;
    rw_u16 expected = rw_crc16_ccitt(raw, 0x3Eu);
    rw_u16 stored   = rw_read_u16_le(raw + 0x3Eu);
    if (expected != stored) {
        return RW_ERR_BAD_CRC;
    }
    out->rx_frequency_hz = rw_read_u32_le(raw + 0x00u);
    out->tx_frequency_hz = rw_read_u32_le(raw + 0x04u);
    out->step_hz_x10     = rw_read_u16_le(raw + 0x08u);
    out->bandwidth_50hz  = rw_read_u16_le(raw + 0x0Au);
    out->modulation      = raw[0x0Cu];
    out->squelch_level   = raw[0x0Du];
    out->rx_subaudible   = rw_read_u16_le(raw + 0x0Eu);
    out->tx_subaudible   = rw_read_u16_le(raw + 0x10u);
    out->zone_mask       = rw_read_u16_le(raw + 0x12u);
    out->flags           = rw_read_u16_le(raw + 0x14u);
    out->power_mw        = rw_read_u16_le(raw + 0x16u);
    out->proto_ref       = rw_read_u16_le(raw + 0x18u);
    out->uid             = rw_read_u16_le(raw + 0x1Au);
    out->proto           = raw[0x1Cu];
    for (i = 0u; i < 9u; ++i)  { out->audio_block[i] = raw[0x1Du + i]; }
    for (i = 0u; i < 16u; ++i) { out->name[i]        = raw[0x26u + i]; }
    for (i = 0u; i < 8u; ++i)  { out->user_tag[i]    = raw[0x36u + i]; }
    out->record_crc      = stored;
    return RW_OK;
}

/* ── Universal STREAM::AUDI Codecs (Zero-Table G.711 u-Law & 12-Bit PCM) ─── */
typedef struct {
    rw_u32 codec;      /* FourCC BE: "ULAW", "P12L", "ADPC", "PC16" */
    rw_u16 rate_hz;    /* e.g. 8000, 12000, 16000, 24000 */
    rw_u16 seq;        /* Rolling 16-bit frame counter (wire drop detect) */
    rw_u32 sample_idx; /* Monotonic 32-bit sample index (MCU overrun detect) */
    rw_u8  slot;       /* 0=VFO A, 1=VFO B, 2=Aux/Si4732, 3=Mic */
    rw_u8  flags;      /* RW_AUDIO_SQ_OPEN | CLIPPED | TX_DIR | SILENCE */
} rw_audio_header;

RW_API void rw_audio_header_pack(const rw_audio_header *h, rw_u8 out[14]) {
    rw_write_u32_be(out + 0u, h->codec);
    rw_write_u16_le(out + 4u, h->rate_hz);
    rw_write_u16_le(out + 6u, h->seq);
    rw_write_u32_le(out + 8u, h->sample_idx);
    out[12] = h->slot;
    out[13] = h->flags;
}

RW_API rw_status rw_audio_header_unpack(const rw_u8 *raw, size_t len, rw_audio_header *out) {
    if (len < RW_AUDIO_HDR_LEN) {
        return RW_ERR_TOO_SHORT;
    }
    out->codec      = rw_read_u32_be(raw + 0u);
    out->rate_hz    = rw_read_u16_le(raw + 4u);
    out->seq        = rw_read_u16_le(raw + 6u);
    out->sample_idx = rw_read_u32_le(raw + 8u);
    out->slot       = raw[12];
    out->flags      = raw[13];
    return RW_OK;
}

/* Zero-table ITU-T G.711 u-law encoder (bias=132, clamp=32635, 38 dB SNR) */
RW_API rw_u8 rw_ulaw_encode(rw_i16 pcm) {
    rw_u32 sign = 0u;
    rw_u32 mag;
    rw_u32 exp = 7u;
    rw_u32 mask = 0x4000u;
    rw_u32 mant;

    if (pcm < 0) {
        sign = 0x80u;
        mag = (rw_u32)(-(rw_i32)pcm);
    } else {
        mag = (rw_u32)pcm;
    }
    if (mag > 32635u) {
        mag = 32635u;
    }
    mag += 132u;
    while (exp > 0u && (mag & mask) == 0u) {
        exp--;
        mask >>= 1;
    }
    mant = (mag >> (exp + 3u)) & 0x0Fu;
    return (rw_u8)(~(sign | (exp << 4) | mant));
}

RW_API rw_i16 rw_ulaw_decode(rw_u8 ulaw) {
    rw_u32 u = (rw_u32)(~ulaw) & 0xFFu;
    rw_u32 sign = u & 0x80u;
    rw_u32 exp  = (u >> 4) & 0x07u;
    rw_u32 mant = u & 0x0Fu;
    rw_i32 t = (rw_i32)((((mant << 3) + 0x84u) << exp) - 0x84u);
    return (rw_i16)(sign ? -t : t);
}

/* Packed 12-bit linear PCM (P12L): 2 x 12-bit samples <-> 3 bytes (lossless SAR ADC) */
RW_API size_t rw_pack12(const rw_u16 *samples, size_t count, rw_u8 *out, size_t out_cap) {
    size_t pairs = count >> 1;
    size_t need = pairs * 3u;
    size_t i, o = 0u;
    if (out_cap < need) {
        return 0u;
    }
    for (i = 0u; i < pairs * 2u; i += 2u) {
        rw_u16 s0 = (rw_u16)(samples[i] & 0x0FFFu);
        rw_u16 s1 = (rw_u16)(samples[i + 1u] & 0x0FFFu);
        out[o++] = (rw_u8)(s0 & 0xFFu);
        out[o++] = (rw_u8)(((s0 >> 8) & 0x0Fu) | ((s1 & 0x0Fu) << 4));
        out[o++] = (rw_u8)((s1 >> 4) & 0xFFu);
    }
    return o;
}

RW_API size_t rw_unpack12(const rw_u8 *in, size_t in_len, rw_u16 *out, size_t out_cap) {
    size_t triples = in_len / 3u;
    size_t need = triples * 2u;
    size_t i, o = 0u;
    if (out_cap < need) {
        return 0u;
    }
    for (i = 0u; i < triples * 3u; i += 3u) {
        rw_u8 b0 = in[i];
        rw_u8 b1 = in[i + 1u];
        rw_u8 b2 = in[i + 2u];
        out[o++] = (rw_u16)((rw_u16)b0 | (((rw_u16)b1 & 0x0Fu) << 8));
        out[o++] = (rw_u16)(((rw_u16)b1 >> 4) | ((rw_u16)b2 << 4));
    }
    return o;
}

#ifdef __cplusplus
}
#endif

#endif /* KERN_RIGWIRE_H */
