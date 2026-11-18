#ifndef SYSINFO_SHARED_H
#define SYSINFO_SHARED_H

#include "krx.h"

static inline u8 si_len(const char* s) {
    u8 n = 0;
    while (s[n]) n++;
    return n;
}

static inline const char* si_nth(const char* p, u8 idx) {
    while (idx > 0) {
        while (*p) p++;
        p++;
        idx--;
    }
    return p;
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
