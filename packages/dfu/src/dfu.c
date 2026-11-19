/*
 * KERN DFU System Module (`/apps/dfu.mod`, P0 Structured Paged — 4 Leaves)
 *
 * Pure, lean USB-C / UART DFU bootloader handoff module:
 *   - Leaf 1 (`leaf1_splash`): Minimalist UV-K1 PackBits RLE splash & BL header
 *   - Leaf 2 (`leaf2_modal`):  Settings hold-confirm bottom sheet + battery guard
 *   - Leaf 3 (`leaf3_stage`):  Presence-aware soft bar & SRAM UI strip pre-renderer
 *   - Leaf 4 (`leaf4_probe`):  Multi-BL ROM fingerprint, QS-1 check & KBHP v1.0 prep
 *
 * Cooperates seamlessly with `/apps/recovery.mod`: shows [MENU] Tools and enables
 * the zero-flash EXC_RETURN bridge only when `/apps/recovery.mod` is installed.
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

static void root_emit_badge(const KrxApiTable* api, i16 x, const char* key, const char* lbl) {
    KrxWidget d;
    d.p0 = (const u8*)key; d.len0 = krx_strlen(key);
    d.p1 = (const u8*)lbl; d.len1 = krx_strlen(lbl);
    d.x = x; d.y = 54; d.a = 0u;
    krx_call(api, OP_UI_WIDGET, KRX_WIDGET_KEY_BADGE, (u32)&d, 0u);
}

static void root_draw_softbar(
    const KrxApiTable* api, const char* lk, const char* ll, const char* rk, const char* rl
) {
    if (lk && lk[0]) root_emit_badge(api, 2, lk, ll);
    if (rk && rk[0]) {
        KrxTextReq tr;
        tr.text = (const u8*)rk; tr.x = 0; tr.y = 0; tr.len = krx_strlen(rk);
        tr.arg = 0u; tr.font = KRX_FONT_TOMTHUMB; tr.color = 1u; tr.flags = 1u; tr._pad = 0u;
        i16 tw = (i16)krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u) + 4;
        if (rl && rl[0]) {
            tr.text = (const u8*)rl; tr.len = krx_strlen(rl);
            tw += 3 + (i16)krx_call(api, OP_GFX_TEXT, (u32)&tr, 0u, 0u);
        }
        root_emit_badge(api, 126 - tw, rk, rl);
    }
}

__attribute__((noinline))
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
        dfu_move_bytes((u8*)DFU_SRAM_BANNERS, g_state.stage_buf, DFU_BANNERS_BYTES + DFU_TITLES_BYTES);
    }

    dfu_quiesce_hw(use_hook ? (1u << 12) : 0u);

    volatile u32* scs = (volatile u32*)0xE000E000u;
    u32 final_msp, final_entry;

    if (use_hook) {
        volatile u32* dst_vec = (volatile u32*)DFU_SRAM_VTOR;
        dfu_move_bytes((u8*)dst_vec, (const u8*)DFU_BL_FLASH_BASE, 192u);
        dst_vec[15] = DFU_SRAM_CODE | 1u;

        DfuHookState* hs = (DfuHookState*)DFU_SRAM_STATE;
        dfu_zero_bytes((u8*)hs, sizeof(DfuHookState));
        hs->flags = (u32)is_stock_k1 | HOOK_FLAG_ARROW_VIS | HOOK_FLAG_KBHP_VALID |
                    (g_state.has_recovery ? HOOK_FLAG_HAS_RECOVERY : 0u);
        hs->last_capsule     = 0xFFFFFFFFu;
        hs->usb_miss_cnt     = 4u;
        hs->kbhp_addr        = KBHP_SRAM_ADDR;
        *(u32*)&hs->last_banner     = 0x00FFFFFFu;
        hs->last_serial_hash = 0x00000800u;
        *(u32*)&hs->flash_elapsed_s = 0xFFFFFFFFu;
        hs->prev_cndtr       = 2048u;

        dfu_move_bytes((u8*)DFU_SRAM_CODE, dfu_hook_start, hook_len);

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
    static const char REC_PATH[] = "/apps/recovery.mod";
    u32 base_addr = (u32)dfu_root_base;
    g_state.ui.softbar   = root_draw_softbar;
    g_state.stage_buf    = (u8*)(base_addr + DFU_STAGE_OFFSET);
    g_state.has_recovery = krx_fs_exists(api, (const u8*)REC_PATH, sizeof(REC_PATH) - 1u) ? 1u : 0u;

    krx_leaf_call(api, DFU_LEAF_PROBE,  (u32)&g_state);
    krx_leaf_call(api, DFU_LEAF_SPLASH, (u32)&g_state);
    krx_leaf_call(api, DFU_LEAF_STAGE,  (u32)&g_state);

    u32 evac_sp = ((base_addr & ~7u) >= 0x20000400u) ? (base_addr & ~7u) : 0x20002FC0u;
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
    if (op == KRX_OP_START && param != 0) {
        enter_dfu_session(api);
    }
    if (op <= KRX_OP_RENDER) {
        g_state.op = op;
        g_state.param = param;
        return (int)krx_leaf_call(api, DFU_LEAF_MODAL, (u32)&g_state);
    }
    return 0;
}
