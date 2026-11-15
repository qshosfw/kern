#ifndef SYSINFO_SHARED_H
#define SYSINFO_SHARED_H

#include "krx.h"

typedef struct {
    const KrxApiTable* api;
    u8   has_qs1;
    u8   _pad[1];
    u16  flash_kb;
    char ver_short[10]; // "v0.1.0"
    char git_hash[8];   // "701d5a4"
    char model[13];     // "UV-K1" / "UV-K5 V3" / custom
    char mcu[16];       // "PY32F071 128K"
    char boot[8];       // "7.02.02"
    char build[12];     // "2026-11-15"
    char sn[15];        // Crockford 14-char
    char mac[18];       // 02:XX:XX:XX:XX:XX
    char hex_id64[19];  // "0x0123456789ABCDEF"
    char hex_uid_hi[17];// High 64 bits of 128-bit UID (16 hex)
    char hex_uid_lo[17];// Low 64 bits of 128-bit UID (16 hex)
    char qs1[17];       // 16-char QS-1 or "Present"
} SysInfoState;

static inline void si_zero(u8* p, u16 n) {
    for (u16 i = 0; i < n; i++) p[i] = 0;
}

static inline void si_copy(u8* dst, const u8* src, u16 n) {
    for (u16 i = 0; i < n; i++) dst[i] = src[i];
}

static inline u8 si_len(const char* s) {
    u8 n = 0;
    while (s[n]) n++;
    return n;
}

static inline char si_hex_nib(u8 v) {
    return (char)(v < 10 ? ('0' + v) : ('A' + (v - 10)));
}

static inline u32 si_udivmod(const KrxApiTable* api, u32 n, u32 d, u32* rem_out) {
    if (api && api->math && api->math->uidivmod) {
        u64 res = api->math->uidivmod(n, d);
        if (rem_out) *rem_out = (u32)(res >> 32);
        return (u32)res;
    }
    u32 q = 0;
    if (d != 0) {
        while (n >= d) { n -= d; q++; }
    }
    if (rem_out) *rem_out = n;
    return q;
}

static inline u8 si_put_uint(const KrxApiTable* api, char* out, u32 v) {
    char tmp[10];
    u8 n = 0;
    do {
        u32 r = 0;
        v = si_udivmod(api, v, 10u, &r);
        tmp[n++] = (char)('0' + r);
    } while (v > 0 && n < 10);
    for (u8 i = 0; i < n; i++) out[i] = tmp[n - 1 - i];
    return n;
}

static inline void si_put_2d(const KrxApiTable* api, char* out, u32 v) {
    u32 r = 0;
    u32 d = si_udivmod(api, v, 10u, &r);
    out[0] = (char)('0' + d);
    out[1] = (char)('0' + r);
}

#endif // SYSINFO_SHARED_H
