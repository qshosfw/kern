/*
 * KERN DFU Bootloader Root Module (/apps/dfu.mod)
 *
 * P0 Structured Paged KRX Module (`KrxExecType::Module`, `FLAG_SYSTEM_ACTION`).
 * Resident root hosts shared UI helpers once and orchestrates 11 P0 overlay leaves:
 *   - Leaf 1  (`leaf1_splash`):  Splash RLE decode & bootloader-aware header
 *   - Leaf 2  (`leaf2_modal`):   Settings hold-confirm modal + low-battery warning
 *   - Leaf 3  (`leaf3_stage`):   Key-release guard, soft bar & SRAM strip pre-renderer
 *   - Leaf 4  (`leaf4_probe`):   ROM fingerprint (Stock K1/Safe/Moto/Custom) & KBHP prep
 *   - Leaf 5  (`leaf5_tools`):   RTC tools gate & DFU Tools menu
 *   - Leaf 6  (`leaf6_info`):    Bootloader & hardware Information screen
 *   - Leaf 7  (`leaf7_calib`):   Calibration inspect & /boot/calib.bkp backup
 *   - Leaf 8  (`leaf8_rescue`):  Buddy Rescue 2-pin UART readiness screen
 *   - Leaf 9  (`leaf9_scan`):    Flat firmware/applet filesystem scanner
 *   - Leaf 10 (`leaf10_files`):  Flat firmware/applet list browser
 *   - Leaf 11 (`leaf11_inspect`): Pre-flight image/applet format & fit inspector
 */
#include "dfu_shared.h"
#include "dfu_quiesce.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((visibility("default"), naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile(
        ".hidden dfu_root_base\n"
        "dfu_root_base:\n"
        "push {lr}\n bl krx_main\n pop {pc}\n"
    );
}

__attribute__((visibility("hidden"))) extern const u8 dfu_root_base[];

__asm__(
    ".section .rodata.dfu_hook,\"a\",%progbits\n"
    ".balign 4\n"
    ".hidden dfu_hook_start\n"
    "dfu_hook_start:\n"
    ".incbin \"dfu_hook.bin\"\n"
    ".hidden dfu_hook_end\n"
    "dfu_hook_end:\n"
    ".text\n"
);

__attribute__((visibility("hidden"))) extern const u8 dfu_hook_start[];
__attribute__((visibility("hidden"))) extern const u8 dfu_hook_end[];

static DfuSharedState g_state;

/* ── Compact Resident UI Helpers (shared by leaves via g_state.ui) ──────────── */

__attribute__((noinline))
static void root_text(const KrxApiTable* api, i16 x, i16 y, const char* s, u8 col, u8 align) {
    krx_draw_str_align(api, x, y, (const u8*)s, krx_strlen(s), KRX_FONT_TOMTHUMB, col, align);
}

static void root_draw_header(const KrxApiTable* api, const char* title) {
    KrxWidget d;
    dfu_zero_bytes((u8*)&d, sizeof(d));
    d.p0 = (const u8*)title;
    d.len0 = krx_strlen(title);
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_NEGATED_HEADER, (u32)&d, 0u);
}

__attribute__((noinline))
static void root_draw_badge(const KrxApiTable* api, i16 x, const char* key, const char* lbl) {
    i16 bw = (i16)(krx_strlen(key) * 4u + 3u);
    krx_fill_rect(api, x, 54, bw, 7, 1u);
    root_text(api, x + 2, 59, key, 0u, KRX_ALIGN_LEFT);
    if (lbl[0]) root_text(api, x + bw + 2, 59, lbl, 1u, KRX_ALIGN_LEFT);
}

static void root_draw_softbar(
    const KrxApiTable* api,
    const char* lk, const char* ll,
    const char* rk, const char* rl
) {
    if (lk[0]) root_draw_badge(api, 2, lk, ll);
    if (rk[0]) {
        u16 kl = krx_strlen(rk), ll2 = krx_strlen(rl);
        i16 tw = (i16)(kl * 4u + 3u + (ll2 ? (ll2 * 4u + 1u) : 0u));
        root_draw_badge(api, 126 - tw, rk, rl);
    }
}

static void root_draw_kv(const KrxApiTable* api, i16 y, const char* k, const char* v) {
    root_text(api, 3, y, k, 1u, KRX_ALIGN_LEFT);
    root_text(api, 125, y, v, 1u, KRX_ALIGN_RIGHT);
}

static u8 root_wait_key(const KrxApiTable* api) {
    while (krx_poll_key(api) != 0u) krx_delay_ms(api, 12u);
    for (;;) {
        u8 k = krx_poll_key(api);
        if (k != 0u) {
            krx_delay_ms(api, 18u);
            if (krx_poll_key(api) == k) return k;
        }
        krx_delay_ms(api, 12u);
    }
}

static u32 root_u32_dec(char* dst, u32 val) {
    char tmp[10];
    u32 n = 0u;
    do {
        u32 q = 0u;
        while (val >= 10u) { val -= 10u; q++; }
        tmp[n++] = (char)('0' + val);
        val = q;
    } while (val > 0u);
    for (u32 i = 0u; i < n; i++) dst[i] = tmp[n - 1u - i];
    return n;
}

static void dfu_move_bytes(u8* dst, const u8* src, u32 len) {
    if ((u32)dst > (u32)src) {
        while (len > 0u) { len--; dst[len] = src[len]; }
    } else {
        dfu_copy_bytes(dst, src, len);
    }
}

__attribute__((noreturn, noinline))
static void handoff_to_bootloader(void) {
    u32 hook_len = (u32)(dfu_hook_end - dfu_hook_start);
    u8 use_hook = g_state.use_hook && (hook_len <= DFU_SRAM_CODE_MAX) &&
                  dfu_vectors_sane(g_state.tramp[0], g_state.tramp[1]) &&
                  dfu_vectors_sane(g_state.tramp[0], g_state.tramp[2]);
    u8 is_stock_k1 = (g_state.kbhp.bl_kind == KBHP_BL_STOCK_K1) ? 1u : 0u;

    dfu_move_bytes((u8*)KBHP_SRAM_ADDR, (const u8*)&g_state.kbhp, sizeof(KbhpEnvelope));

    if (use_hook && is_stock_k1 && g_state.has_stage) {
        dfu_move_bytes((u8*)DFU_SRAM_VIDPID, g_state.sram_vidpid, sizeof(g_state.sram_vidpid));
        dfu_move_bytes((u8*)DFU_SRAM_ARROW, g_state.sram_arrow, sizeof(g_state.sram_arrow));
        dfu_move_bytes((u8*)DFU_SRAM_BANNERS, g_state.buf.sram_banners, sizeof(g_state.buf.sram_banners));
    }

    dfu_quiesce_hw(use_hook ? (1u << 12) : 0u);

    volatile u32* scs = (volatile u32*)0xE000E000u;
    u32 final_msp, final_entry;

    if (use_hook) {
        volatile u32* dst_vec = (volatile u32*)DFU_SRAM_VTOR;
        dfu_copy_bytes((u8*)dst_vec, (const u8*)DFU_BL_FLASH_BASE, 192u);
        dst_vec[15] = DFU_SRAM_CODE | 1u;

        DfuHookState* hs = (DfuHookState*)DFU_SRAM_STATE;
        dfu_zero_bytes((u8*)hs, sizeof(DfuHookState));
        hs->flags            = (u32)is_stock_k1 | HOOK_FLAG_ARROW_VIS | HOOK_FLAG_KBHP_VALID;
        hs->last_capsule     = 0xFFFFFFFFu;
        hs->usb_miss_cnt     = 4u;
        hs->kbhp_addr        = KBHP_SRAM_ADDR;
        hs->last_banner      = 0xFFFFu;
        hs->last_led         = 0xFFu;
        hs->last_serial_hash = 0x00000800u;
        hs->flash_elapsed_s  = 0xFFFFu;
        hs->last_cutout      = 0xFFFFu;
        hs->prev_cndtr       = 2048u;

        dfu_copy_bytes((u8*)DFU_SRAM_CODE, dfu_hook_start, hook_len);

        void (*sys_init_fn)(void) = (void (*)(void))(g_state.tramp[1]);
        sys_init_fn();

        scs[0xD08 / 4] = DFU_SRAM_VTOR;
        scs[0x014 / 4] = 1599999u;
        scs[0x018 / 4] = 0u;
        scs[0x010 / 4] = 0b111u;
        final_msp   = g_state.tramp[0];
        final_entry = g_state.tramp[2];
    } else {
        scs[0xD08 / 4] = DFU_BL_FLASH_BASE;
        final_msp   = *(const volatile u32*)(DFU_BL_FLASH_BASE + 0u);
        final_entry = *(const volatile u32*)(DFU_BL_FLASH_BASE + 4u);
        if (!dfu_vectors_sane(final_msp, final_entry)) {
            scs[0xD0C / 4] = 0x05FA0004u;
            for (;;) { __asm__ volatile("nop"); }
        }
    }

    __asm__ volatile(
        "dsb\n isb\n msr msp, %0\n cpsie i\n bx %1\n"
        : : "r"(final_msp), "r"(final_entry) : "memory"
    );
    __builtin_unreachable();
}

__attribute__((noreturn))
static void enter_dfu_session(const KrxApiTable* api) {
    krx_leaf_call(api, DFU_LEAF_PROBE, (u32)&g_state);

    g_state.ui.header   = root_draw_header;
    g_state.ui.softbar  = root_draw_softbar;
    g_state.ui.kv       = root_draw_kv;
    g_state.ui.wait_key = root_wait_key;
    g_state.ui.u32_dec  = root_u32_dec;

    /* Leaf 5 checks RTC_ALR TOOLS flag; immediately returns DFU_TOOL_ACT_DFU on normal boot */
    for (;;) {
        g_state.tool_action = DFU_TOOL_ACT_DFU;
        krx_leaf_call(api, DFU_LEAF_TOOLS, (u32)&g_state);
        if (g_state.tool_action != DFU_TOOL_ACT_OPEN) break;
        u8 sel = g_state.tool_sel;
        if (sel <= DFU_TOOL_ITEM_APPLETS) {
            g_state.tool_sub = sel;
            krx_leaf_call(api, DFU_LEAF_SCAN, (u32)&g_state);
            for (;;) {
                g_state.tool_action = DFU_TOOL_ACT_DFU;
                krx_leaf_call(api, DFU_LEAF_FILES, (u32)&g_state);
                if (g_state.tool_action != DFU_TOOL_ACT_INSPECT) break;
                krx_leaf_call(api, DFU_LEAF_INSPECT, (u32)&g_state);
            }
        } else {
            static const u8 SUB_LEAF[3] = { DFU_LEAF_RESCUE, DFU_LEAF_CALIB, DFU_LEAF_INFO };
            krx_leaf_call(api, SUB_LEAF[sel - 2u], (u32)&g_state);
        }
    }

    krx_leaf_call(api, DFU_LEAF_SPLASH, (u32)&g_state);
    krx_leaf_call(api, DFU_LEAF_STAGE, (u32)&g_state);

    u32 base_addr = ((u32)dfu_root_base) & ~7u;
    u32 evac_sp = (base_addr >= 0x20000400u) ? base_addr : 0x20002FC0u;
    register u32 r1_sp __asm__("r1") = evac_sp;
    register u32 r2_fn __asm__("r2") = (u32)handoff_to_bootloader;
    __asm__ volatile(
        "cpsid i\n msr msp, %0\n isb\n bx %1\n"
        : : "r"(r1_sp), "r"(r2_fn) : "memory"
    );
    __builtin_unreachable();
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        if (param != 0) {
            enter_dfu_session(api);
        }
        g_state.op = op;
        g_state.param = param;
        krx_leaf_call(api, DFU_LEAF_MODAL, (u32)&g_state);
        return 0;
    }

    if (op == KRX_OP_EVENT || op == KRX_OP_RENDER) {
        g_state.op = op;
        g_state.param = param;
        return (int)krx_leaf_call(api, DFU_LEAF_MODAL, (u32)&g_state);
    }

    return 0;
}
