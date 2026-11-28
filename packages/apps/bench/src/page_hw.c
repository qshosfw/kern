/*
 * P1 v2 Block 2: System/DMA/KernFS Self-Test (Tab 4) + Keypad Matrix (Tab 5).
 */

#include "bench_shared.h"

int block_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl block_main\n pop {pc}\n");
}

static void draw_kv(const KrxApiTable* api, i16 x, i16 y, const char* k, u32 val) {
    u8 kl = bench_slen(k);
    krx_draw_str_ex(api, x, y, (const u8*)k, kl, KRX_FONT_TOMTHUMB, COLOR_FILL);
    u8 buf[10];
    u8 n = bench_u32_to_str(api, val, buf);
    krx_draw_str_ex(api, (i16)(x + kl * 4 + 2), y, buf, n, KRX_FONT_TOMTHUMB, COLOR_FILL);
}

int block_main(u32 op, const KrxApiTable* api, const void* param) {
    (void)op;
    if (!api || !param) return 0;
    const u32* ctx = (const u32*)param;
    const BenchState* st = (const BenchState*)ctx[1];
    if (!st) return 0;

    if (st->tab == 4u) {
        u32 mv = krx_prop_get32(api, KRX_PROP_BATT_MV);
        u32 pct = krx_prop_get32(api, KRX_PROP_BATT_PCT);
        u32 mhz = krx_udivmod(api, krx_prop_get32(api, KRX_PROP_CPU_HZ), 1000000u, (u32*)0);
        u32 stk = krx_prop_get32(api, KRX_PROP_STACK_FREE);

        draw_kv(api, 4, 20, "BAT:", mv);
        draw_kv(api, 66, 20, "PCT:", pct);
        draw_kv(api, 4, 29, "CPU:", mhz);
        draw_kv(api, 66, 29, "STK:", stk);

        const char* fs_s = st->fs_status == 1u ? "FS:PASS" : (st->fs_status == 2u ? "FS:FAIL" : "FS:IDLE");
        const char* dv_s = st->div_status == 1u ? "DIV:PASS" : (st->div_status == 2u ? "DIV:FAIL" : "DIV:IDLE");
        krx_ui_draw_tag_badge(api, 4, 34, (const u8*)fs_s, bench_slen(fs_s), st->fs_status == 1u ? 1u : 0u);
        krx_ui_draw_tag_badge(api, 52, 34, (const u8*)dv_s, bench_slen(dv_s), st->div_status == 1u ? 1u : 0u);
        krx_draw_str_ex(api, 4, 50, (const u8*)"M:RUN FS+CRC+DIV TEST", 21u, KRX_FONT_TOMTHUMB, COLOR_FILL);
    } else {
        u32 mask = krx_key_matrix(api);
        krx_draw_str_ex(api, 4, 20, (const u8*)"KEY MATRIX", 10u, KRX_FONT_MUMATRIX, COLOR_FILL);
        draw_kv(api, 4, 31, "MASK:", mask);
        draw_kv(api, 4, 40, "LAST:", st->last_key);
        draw_kv(api, 64, 40, "CNT:", st->key_count);
        krx_draw_str_ex(api, 4, 50, (const u8*)"PRESS KEYS  M:TORCH", 19u, KRX_FONT_TOMTHUMB, COLOR_FILL);
    }
    return 0;
}
