/*
 * KERN Interactive Hardware, OS & UI Test Bench (P1 v2 Demand-Paged).
 *
 * 6 interactive tabs across 3 on-demand P1 v2 flash blocks:
 *   0: UI    - UI Kit widgets, badges, capsules, progress bar, toast
 *   1: Menu  - List rows, scrollbar, live OS confirm modal
 *   2: RF    - Live tuner (+/-12.5kHz), RSSI S-meter, 64-bin spectrum sweep
 *   3: Audio - Live volume control, non-blocking tone & DTMF playback
 *   4: Sys   - Battery/CPU/Stack/KernFS telemetry + live FS & DIV self-test
 *   5: Key   - Live keypad matrix bitmask, key counter & torch test
 */

#include "bench_shared.h"

static BenchState g_st;

static const char TAB_TITLES[] = "UI\0Menu\0RF\0Aud\0Sys\0Key";
static const u16 TEST_TONES[4] = { 440u, 880u, 1000u, 1750u };

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl krx_main\n pop {pc}\n");
}

static void select_tab(const KrxApiTable* api, u8 next) {
    if (next < BENCH_TAB_COUNT && next != g_st.tab) {
        g_st.prev_tab = g_st.tab;
        g_st.slide_dir = (next > g_st.tab) ? 1 : -1;
        g_st.tab = next;
        g_st.anim_prog = krx_prop_get(api, KRX_PROP_UI_ANIMATIONS) ? 0u : 16u;
    }
}

static void run_rf_sweep(const KrxApiTable* api) {
    u32 center = g_st.rf_freq_hz;
    u32 start = center > 800000u ? (center - 800000u) : 18000000u;
    struct { u32 start_hz; u32 step_hz; i16* out_rssi; u16 count; u16 _pad; } req = {
        start, 25000u, g_st.rf_bins, 64u, 0u
    };
    krx_call(api, OP_RF_BULK, SUB_RF_BULK_SWEEP, (u32)&req, 0u);
    g_st.rf_swept = 1u;
}

static void run_sys_selftest(const KrxApiTable* api) {
    // 1. Hardware 32/32 DIV check
    u32 rem = 0;
    u32 q = krx_udivmod(api, 1234567u, 997u, &rem);
    g_st.div_status = (q == 1238u && rem == 281u) ? 1u : 2u;

    // 2. KernFS write + read + CRC32 + remove roundtrip
    static const u8 PATH[] = "/user/_bench.tmp";
    static const u8 PAYLOAD[16] = "KERN_BENCH_DMA1!";
    u8 rbuf[16];
    for (u8 i = 0; i < 16u; i++) rbuf[i] = 0u;
    u8 w_rc = krx_fs_write(api, PATH, 16u, PAYLOAD, 16u);
    u32 r_n = krx_fs_read(api, PATH, 16u, rbuf, 16u);
    u32 c1 = krx_crc32(api, PAYLOAD, 16u);
    u32 c2 = krx_crc32(api, rbuf, 16u);
    (void)krx_fs_remove(api, PATH, 16u);
    g_st.fs_status = (w_rc == 0u && r_n == 16u && c1 == c2) ? 1u : 2u;
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        u8* p = (u8*)&g_st;
        for (u32 i = 0; i < sizeof(BenchState); i++) p[i] = 0u;
        g_st.anim_prog = 16u;
        g_st.progress = 65u;
        g_st.rf_freq_hz = krx_rf_get_freq(api);
        if (g_st.rf_freq_hz < 18000000u) g_st.rf_freq_hz = 145500000u;
        g_st.aud_vol = krx_prop_get(api, KRX_PROP_VOLUME);

        KrxStatusBar sb;
        for (u8 i = 0; i < sizeof(KrxStatusBar); i++) ((u8*)&sb)[i] = 0u;
        sb.flags = KRX_STATUS_VISIBLE | KRX_STATUS_CUSTOM_TITLE | KRX_STATUS_PARTIAL;
        krx_ui_set_status_bar(api, &sb);
        return 0;
    }

    if (op == KRX_OP_EVENT && param) {
        const u8* ev = (const u8*)param;
        u8 kind = ev[0];

        if (kind == KRX_EV_TICK) {
            if (g_st.anim_prog < 16u) {
                g_st.anim_prog += 4u;
                if (g_st.anim_prog >= 16u) {
                    g_st.anim_prog = 16u;
                    g_st.slide_dir = 0;
                }
                return 1;
            }
            return (g_st.tab == 2u || g_st.tab == 4u || g_st.tab == 5u) ? 1 : 0;
        }

        if (kind == KRX_EV_KEY) {
            u8 key = ev[1];
            u32 state = *(const u32*)(ev + 4);
            if (state != 0u) return 1;

            g_st.last_key = key;
            g_st.key_count++;

            if (key == KEY_EXIT) return 0;
            if (key == KEY_4 || key == KEY_STAR) {
                select_tab(api, (g_st.tab == 0u) ? (BENCH_TAB_COUNT - 1u) : (u8)(g_st.tab - 1u));
                return 1;
            }
            if (key == KEY_6 || key == KEY_HASH) {
                select_tab(api, (g_st.tab + 1u >= BENCH_TAB_COUNT) ? 0u : (u8)(g_st.tab + 1u));
                return 1;
            }
            if (key >= KEY_1 && key <= KEY_6) {
                select_tab(api, (u8)(key - KEY_1));
                return 1;
            }

            if (key == KEY_UP) {
                if (g_st.tab == 0u) {
                    g_st.progress = (g_st.progress <= 90u) ? (u8)(g_st.progress + 10u) : 100u;
                } else if (g_st.tab == 1u) {
                    g_st.menu_sel = (g_st.menu_sel == 0u) ? 5u : (u8)(g_st.menu_sel - 1u);
                } else if (g_st.tab == 2u) {
                    g_st.rf_freq_hz += 12500u;
                    krx_rf_tune(api, g_st.rf_freq_hz, KRX_RF_WIDTH_12K5, 12500u);
                } else if (g_st.tab == 3u) {
                    if (g_st.aud_vol < 10u) g_st.aud_vol++;
                    krx_prop_set(api, KRX_PROP_VOLUME, g_st.aud_vol);
                }
                return 1;
            }
            if (key == KEY_DOWN) {
                if (g_st.tab == 0u) {
                    g_st.progress = (g_st.progress >= 10u) ? (u8)(g_st.progress - 10u) : 0u;
                } else if (g_st.tab == 1u) {
                    g_st.menu_sel = (g_st.menu_sel >= 5u) ? 0u : (u8)(g_st.menu_sel + 1u);
                } else if (g_st.tab == 2u && g_st.rf_freq_hz > 18012500u) {
                    g_st.rf_freq_hz -= 12500u;
                    krx_rf_tune(api, g_st.rf_freq_hz, KRX_RF_WIDTH_12K5, 12500u);
                } else if (g_st.tab == 3u) {
                    if (g_st.aud_vol > 0u) g_st.aud_vol--;
                    krx_prop_set(api, KRX_PROP_VOLUME, g_st.aud_vol);
                }
                return 1;
            }
            if (key == KEY_MENU) {
                if (g_st.tab == 0u) {
                    krx_ui_show_toast(api, (const u8*)"UI Kit OK", 9u, 1000u);
                } else if (g_st.tab == 1u) {
                    krx_ui_show_confirm_labels(
                        api, 1u,
                        (const u8*)"Bench Modal", 11u,
                        (const u8*)"Dialog & Keys OK?", 17u,
                        (const u8*)"YES", 3u,
                        (const u8*)"NO", 2u
                    );
                } else if (g_st.tab == 2u) {
                    run_rf_sweep(api);
                } else if (g_st.tab == 3u) {
                    krx_call(api, OP_AUD, SUB_AUD_TONE, (u32)TEST_TONES[g_st.aud_idx], 160u);
                    g_st.aud_idx = (g_st.aud_idx >= 3u) ? 0u : (u8)(g_st.aud_idx + 1u);
                } else if (g_st.tab == 4u) {
                    run_sys_selftest(api);
                } else if (g_st.tab == 5u) {
                    krx_prop_set(api, KRX_PROP_FLASHLIGHT, 2u);
                }
                return 1;
            }
            return 1;
        }
        return 1;
    }

    if (op == KRX_OP_RENDER) {
        krx_clear(api);
        u8 block = (u8)(g_st.tab >> 1);
        (void)krx_leaf_call(api, block, (u32)&g_st);

        krx_draw_tabbed_sheet_anim(
            api, 0u, 0u, 128u, 53u, g_st.tab, g_st.prev_tab, g_st.anim_prog, BENCH_TAB_COUNT,
            KRX_TAB_FLAG_INTEGRATED_86 | KRX_TAB_FLAG_BOTTOM_BORDER,
            TAB_TITLES, (u16)(sizeof(TAB_TITLES) - 1u)
        );

        static const char* const ACT_LBL[6] = { "Toast", "Modal", "Sweep", "Tone", "Test", "Torch" };
        const char* act = ACT_LBL[g_st.tab];
        krx_ui_draw_key_badge(api, 2, 56, (const u8*)"*#", 2u, (const u8*)"Tab", 3u);
        krx_ui_draw_key_badge(api, 40, 56, (const u8*)"M", 1u, (const u8*)act, bench_slen(act));
        krx_ui_draw_key_badge(api, 86, 56, (const u8*)"EXIT", 4u, (const u8*)"Back", 4u);
        return 0;
    }

    if (op == KRX_OP_EXIT) {
        krx_ui_set_status_bar(api, 0);
        krx_ui_set_tray_icon(api, KRX_ICON_NONE);
        krx_ui_close_modal(api);
        return 0;
    }

    return 0;
}
