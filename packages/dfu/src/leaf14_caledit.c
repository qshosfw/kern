/*
 * DFU Leaf 14: Calibration Parameter Editor
 *
 * Adjusts selected calibration field (st->tool_sub = 0..=5) inside st->stage_buf
 * with live bounds clamping; MENU applies, EXIT cancels and restores original value.
 */
#include "dfu_shared.h"

#define CAL_EDIT_COUNT 6u

typedef struct {
    u8  off_rel;
    u8  step;
    i16 min_v;
    i16 max_v;
} CalField;

static const char* nth_s(const char* p, u8 i) {
    while (i--) { while (*p) p++; p++; }
    return p;
}

static void fmt_i32(const DfuSharedState* st, char* dst, i32 v) {
    u32 n = 0u;
    if (v < 0) { dst[n++] = '-'; v = -v; }
    n += st->ui.u32_dec(dst + n, (u32)v);
    dst[n] = 0;
}

__attribute__((visibility("default"), section(".text.entry")))
u32 leaf14_entry(u32 op, const KrxApiTable* api, void* param) {
    (void)op;
    KrxLeafContext* ctx = (KrxLeafContext*)param;
    if (!ctx || !api || !ctx->arg) return 1u;
    DfuSharedState* st = (DfuSharedState*)ctx->arg;

    static const char NAMES[] =
        "XTAL Trim\0Vol Gain\0DAC Gain\0LNA Calib\0Mix Calib\0Batt 7.6V\0";
    static const CalField FIELDS[CAL_EDIT_COUNT] = {
        { 0x48u, 10u, -1000, 1000 },
        { 0x4Eu,  1u,     0,   63 },
        { 0x4Fu,  1u,     0,   15 },
        { 0x4Au,  4u,     0,  511 },
        { 0x4Cu,  4u,     0,  511 },
        { 0x06u,  5u,  1800, 2600 },
    };

    u8 idx = (st->tool_sub < CAL_EDIT_COUNT) ? st->tool_sub : 0u;
    const CalField* f = &FIELDS[idx];
    u8* buf = st->stage_buf;
    u32 o = 0x140u + f->off_rel;
    i32 val = (o >= 0x18Eu) ? (i32)buf[o]
                            : (i32)((u16)buf[o] | ((u16)buf[o + 1u] << 8));
    if (f->min_v < 0) val = (i32)(i16)val;
    i32 orig = val;

    char rng[16], cur[10];
    fmt_i32(st, rng, f->min_v);
    u32 rn = krx_strlen(rng);
    rng[rn++] = '.'; rng[rn++] = '.';
    fmt_i32(st, rng + rn, f->max_v);

    for (;;) {
        fmt_i32(st, cur, val);
        krx_clear(api);
        dfu_draw_header(api, nth_s(NAMES, idx));
        st->ui.kv(api, 20, "Value", cur);
        st->ui.kv(api, 30, "Range", rng);
        krx_draw_str_align(api, 64, 42, (const u8*)"UP / DOWN to adjust", 19u,
                           KRX_FONT_TOMTHUMB, 1u, KRX_ALIGN_CENTER);
        st->ui.softbar(api, "MENU", "Apply", "EXIT", "Cancel");
        krx_commit_frame(api);

        u8 k = st->ui.wait_key(api);
        if (k == KEY_UP) {
            val += f->step;
            if (val > f->max_v) val = f->max_v;
        } else if (k == KEY_DOWN) {
            val -= f->step;
            if (val < f->min_v) val = f->min_v;
        } else if (k == KEY_MENU || k == KEY_EXIT) {
            i32 out = (k == KEY_MENU) ? val : orig;
            buf[o] = (u8)out;
            if (o < 0x18Eu) buf[o + 1u] = (u8)(out >> 8);
            st->tool_action = (u8)(10u + idx); /* restore cursor on leaf7_calib */
            st->next_leaf = DFU_LEAF_CALIB;
            return 0u;
        }
    }
}
