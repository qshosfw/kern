/*
 * P1 v2 Block 1: RF Tuner & Spectrum Sweep (Tab 2) + Audio/Tone Bench (Tab 3).
 */

#include "bench_shared.h"

int block_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl block_main\n pop {pc}\n");
}

int block_main(u32 op, const KrxApiTable* api, const void* param) {
    (void)op;
    if (!api || !param) return 0;
    const u32* ctx = (const u32*)param;
    const BenchState* st = (const BenchState*)ctx[1];
    if (!st) return 0;

    if (st->tab == 2u) {
        u32 rem = 0;
        u32 mhz = krx_udivmod(api, st->rf_freq_hz, 1000000u, &rem);
        u32 khz = krx_udivmod(api, rem, 1000u, (u32*)0);
        u8 buf[16];
        u8 n = bench_u32_to_str(api, mhz, buf);
        buf[n++] = '.';
        u32 d2 = krx_udivmod(api, khz, 100u, &rem);
        u32 d1 = krx_udivmod(api, rem, 10u, &rem);
        buf[n++] = (u8)('0' + d2);
        buf[n++] = (u8)('0' + d1);
        buf[n++] = (u8)('0' + rem);
        buf[n++] = 'M';

        krx_draw_str_ex(api, 4, 20, buf, n, KRX_FONT_MUMATRIX, COLOR_FILL);
        i16 rssi = (i16)krx_prop_get32(api, KRX_PROP_RF_RSSI);
        krx_ui_draw_s_meter(api, 64, 15, rssi, 0u);

        if (st->rf_swept) {
            for (u8 i = 0; i < 60u; i++) {
                int db = (int)st->rf_bins[i] + 130;
                int h = db > 24 ? 24 : (db < 1 ? 1 : db);
                krx_fill_rect(api, (i16)(4 + i * 2), (i16)(50 - h), 2u, (u8)h, COLOR_FILL);
            }
        } else {
            krx_draw_str_ex(api, 4, 34, (const u8*)"UP/DN:TUNE +/-12.5K", 19u, KRX_FONT_TOMTHUMB, COLOR_FILL);
            krx_draw_str_ex(api, 4, 44, (const u8*)"M:RUN 64-BIN SWEEP", 18u, KRX_FONT_TOMTHUMB, COLOR_FILL);
        }
    } else {
        static const char* const TONE_LBL[4] = { "440 HZ", "880 HZ", "1000 HZ", "1750 HZ" };
        const char* tl = TONE_LBL[st->aud_idx & 3u];
        krx_draw_str_ex(api, 4, 20, (const u8*)"AUDIO BENCH", 11u, KRX_FONT_MUMATRIX, COLOR_FILL);
        krx_ui_draw_tag_badge(api, 82, 13, (const u8*)tl, bench_slen(tl), 1u);

        krx_draw_str_ex(api, 4, 31, (const u8*)"VOL", 3u, KRX_FONT_TOMTHUMB, COLOR_FILL);
        krx_ui_draw_progress_bar(api, 22, 26, 98u, 6u, (u8)(st->aud_vol * 10u), 1u);
        krx_draw_str_ex(api, 4, 44, (const u8*)"UP/DN:VOL  M:PLAY TONE", 22u, KRX_FONT_TOMTHUMB, COLOR_FILL);
    }
    return 0;
}
