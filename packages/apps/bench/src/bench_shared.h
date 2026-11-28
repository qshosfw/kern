#ifndef BENCH_SHARED_H
#define BENCH_SHARED_H

#include <krx.h>

#define BENCH_TAB_COUNT 6u

typedef struct {
    u8  tab;           // 0=UI, 1=Menu, 2=RF, 3=Audio, 4=Sys, 5=Key
    u8  prev_tab;
    u8  anim_prog;
    i8  slide_dir;
    u8  menu_sel;
    u8  progress;
    u8  aud_idx;
    u8  aud_vol;
    u8  rf_swept;
    u8  fs_status;     // 0=idle, 1=pass, 2=fail
    u8  div_status;    // 0=idle, 1=pass, 2=fail
    u8  last_key;
    u16 key_count;
    u16 _pad;
    u32 rf_freq_hz;
    i16 rf_bins[64];
} BenchState;

static inline u8 bench_slen(const char* s) {
    u8 n = 0;
    while (s[n]) n++;
    return n;
}

static inline u8 bench_u32_to_str(const KrxApiTable* api, u32 val, u8* out) {
    u8 tmp[10];
    u8 n = 0;
    if (val == 0u) {
        out[0] = '0';
        return 1u;
    }
    while (val > 0u && n < 10u) {
        u32 rem = 0;
        val = krx_udivmod(api, val, 10u, &rem);
        tmp[n++] = (u8)('0' + rem);
    }
    for (u8 i = 0; i < n; i++) out[i] = tmp[n - 1u - i];
    return n;
}

#endif
