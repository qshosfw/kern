/*
 * KERN DFU Bootloader Root Module (/apps/dfu.mod)
 *
 * P0 Structured Paged KRX Module (`KrxExecType::Module`, `FLAG_SYSTEM_ACTION`).
 * Resident root holds `dfu_hook.bin` (2.1 KB) + `quiesce_hw_for_dfu` +
 * `handoff_to_factory_bootloader`; overlays handle splash (`leaf1_splash`),
 * Settings hold-confirm modal (`leaf2_modal`), and SRAM staging (`leaf3_stage`).
 */

#include "dfu_shared.h"

int krx_main(u32 op, const KrxApiTable* api, const void* param);

__attribute__((visibility("default"), naked, section(".text.entry")))
void _start(void) {
    __asm__ volatile("push {lr}\n bl krx_main\n pop {pc}\n");
}

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

#define DFU_SRAM_VTOR  0x20003600u
#define DFU_SRAM_STATE 0x200036C0u
#define DFU_SRAM_CODE  0x20003700u

static DfuSharedState g_state;

static u8 resolve_stock_bl_trampoline(u32 out[3]) {
    const u8* base = (const u8*)0x08000000u;
    const u32 rom_len = 0x2800u;
    u32 msp = *(const volatile u32*)(base + 0);
    u32 reset = *(const volatile u32*)(base + 4);
    u32 pc_off = (reset - 0x08000000u) & ~1u;
    if ((msp - 0x20000400u) > 0x2A00u || (reset & 1u) == 0 || pc_off < 8u || pc_off + 8u > rom_len) {
        return 0;
    }
    out[0] = msp;
    for (u32 step = 0; step < 2u; step++) {
        u32 off = pc_off + step * 4u;
        u16 expect = (step == 0u) ? 0x4780u : 0x4700u;
        if (base[off + 1] != 0x48u || *(const volatile u16*)(base + off + 2) != expect) {
            return 0;
        }
        u32 lit = ((off + 4u) & ~3u) + ((u32)base[off] << 2);
        if (lit + 4u > rom_len) return 0;
        u32 target = *(const volatile u32*)(base + lit);
        if ((target & 1u) == 0 || (target - 0x08000010u) >= 0x27F0u) {
            return 0;
        }
        out[step + 1] = target;
    }
    return 1;
}

static void gpio_apply_and_lock(u32 base, u32 clear_2b, u32 pupdr_set, u32 moder_set, u32 lock_pins) {
    volatile u32* port = (volatile u32*)base;
    u32 pupdr = port[0x0C / 4];
    port[0x0C / 4] = (pupdr & ~clear_2b) | pupdr_set;
    u32 moder = port[0];
    port[0] = (moder & ~clear_2b) | moder_set;
    volatile u32* lckr = &port[0x1C / 4];
    u32 lckk = (1u << 16) | lock_pins;
    *lckr = lckk;
    *lckr = lock_pins;
    *lckr = lckk;
    (void)*lckr;
}

static void quiesce_hw_for_dfu(u32 apb2_keep) {
    volatile u32* gpiob = (volatile u32*)0x50000400u;
    u32 ptt_pin = 1u << 10;
    gpiob[0x28 / 4] = ptt_pin;
    gpiob[0x04 / 4] |= ptt_pin;
    gpio_apply_and_lock(0x50000400u, 0xF0303FC0u, 0x50001540u, 0b01u << 20, 0xC470u);

    volatile u32* rcc = (volatile u32*)0x40021000u;
    rcc[0x34 / 4] |= (1u << 2) | 0x1u;
    volatile u32* gpioc = (volatile u32*)0x50000800u;
    gpioc[0x28 / 4] = 1u << 13;
    gpio_apply_and_lock(0x50000800u, 0b11u << 26, 0b10u << 26, 0u, 1u << 13);

    rcc[0x08 / 4] = 0u;
    while ((rcc[0x08 / 4] & 0x38u) != 0u) {}
    rcc[0] &= ~(1u << 24);

    if ((rcc[0x3C / 4] & (1u << 23)) != 0u) {
        *(volatile u8*)0x40005C01u = 0u;
    }

    rcc[0x28 / 4] = 1u;
    rcc[0x28 / 4] = 0u;
    rcc[0x2C / 4] = 0xFFFFFFFFu;
    rcc[0x2C / 4] = 0u;
    rcc[0x30 / 4] = ~apb2_keep;
    rcc[0x30 / 4] = 0u;
    rcc[0x3C / 4] = 0u;
    rcc[0x40 / 4] = apb2_keep;

    volatile u32* gpioa = (volatile u32*)0x50000000u;
    gpioa[0x28 / 4] = 1u << 12;
    gpioa[0x0C / 4] &= ~(0b11u << 24);
    u32 pa_moder = gpioa[0];
    gpioa[0] = (pa_moder & ~(0b11u << 24)) | (0b01u << 24);
    for (volatile u32 d = 0; d < 600000u; d++) { __asm__ volatile("nop"); }
    gpioa[0] = pa_moder & ~(0b11u << 24);

    volatile u32* scs = (volatile u32*)0xE000E000u;
    scs[0x010 / 4] = 0u;
    scs[0xD04 / 4] = 1u << 25;
    scs[0x180 / 4] = 0xFFFFFFFFu;
    scs[0x280 / 4] = 0xFFFFFFFFu;
}

__attribute__((noreturn, noinline))
static void handoff_to_factory_bootloader(u32 vid_pid) {
    u32 tramp[3] = {0, 0, 0};
    u32 hook_len = (u32)(dfu_hook_end - dfu_hook_start);
    u8 use_hook = resolve_stock_bl_trampoline(tramp) && hook_len <= 0x900u;
    u8 is_stock_k1 = (vid_pid == 0x36B70001u) ? 1u : 0u;

    if (use_hook && is_stock_k1 && g_state.has_stage) {
        dfu_copy_bytes((u8*)DFU_SRAM_BANNERS, g_state.sram_banners, sizeof(g_state.sram_banners));
        dfu_copy_bytes((u8*)DFU_SRAM_ARROW, g_state.sram_arrow, sizeof(g_state.sram_arrow));
        dfu_copy_bytes((u8*)DFU_SRAM_VIDPID, g_state.sram_vidpid, sizeof(g_state.sram_vidpid));
    }

    quiesce_hw_for_dfu(use_hook ? (1u << 12) : 0u);

    volatile u32* scs = (volatile u32*)0xE000E000u;
    u32 final_msp, final_entry;
    if (use_hook) {
        volatile u32* dst_vec = (volatile u32*)DFU_SRAM_VTOR;
        dfu_copy_bytes((u8*)dst_vec, (const u8*)0x08000000u, 192u);
        dst_vec[15] = DFU_SRAM_CODE | 1u;

        volatile u32* sp = (volatile u32*)DFU_SRAM_STATE;
        sp[0]  = (u32)is_stock_k1 | 0x10u;
        sp[1]  = 0u;
        sp[2]  = 0u;
        sp[3]  = 0xFFFFFFFFu;
        sp[4]  = 0x00040000u;
        sp[5]  = 0xFFFFFFFFu;
        sp[6]  = 0u;
        sp[7]  = 0x00FFFFFFu;
        sp[8]  = 0x00000800u;
        sp[9]  = 0xFFFFFFFFu;
        sp[10] = 0u;
        sp[11] = 0u;
        sp[12] = 2048u;

        dfu_copy_bytes((u8*)DFU_SRAM_CODE, dfu_hook_start, hook_len);

        void (*sys_init_fn)(void) = (void (*)(void))(tramp[1]);
        sys_init_fn();

        scs[0xD08 / 4] = DFU_SRAM_VTOR;
        scs[0x014 / 4] = 1599999u;
        scs[0x018 / 4] = 0u;
        scs[0x010 / 4] = 0b111u;
        final_msp = tramp[0];
        final_entry = tramp[2];
    } else {
        scs[0xD08 / 4] = 0x08000000u;
        final_msp = *(const volatile u32*)0x08000000u;
        final_entry = *(const volatile u32*)0x08000004u;
    }

    __asm__ volatile(
        "dsb\n"
        "isb\n"
        "msr msp, %0\n"
        "cpsie i\n"
        "bx %1\n"
        :
        : "r"(final_msp), "r"(final_entry)
        : "memory"
    );
    __builtin_unreachable();
}

__attribute__((noreturn))
static void enter_dfu_with_hooks(const KrxApiTable* api) {
    if (api->service && api->service->leaf_call) {
        api->service->leaf_call(1, (u32)&g_state);
        api->service->leaf_call(3, (u32)&g_state);
    }
    register u32 r0_vp __asm__("r0") = g_state.vid_pid;
    register u32 r1_sp __asm__("r1") = 0x20002FC0u;
    register u32 r2_fn __asm__("r2") = (u32)handoff_to_factory_bootloader;
    __asm__ volatile(
        "cpsid i\n"
        "msr msp, %1\n"
        "isb\n"
        "bx %2\n"
        :
        : "r"(r0_vp), "r"(r1_sp), "r"(r2_fn)
        : "memory"
    );
    __builtin_unreachable();
}

int krx_main(u32 op, const KrxApiTable* api, const void* param) {
    if (!api) return 0;

    if (op == KRX_OP_START) {
        if (param != 0) {
            enter_dfu_with_hooks(api);
        }
        g_state.op = op;
        g_state.param = param;
        if (api->service && api->service->leaf_call) {
            api->service->leaf_call(2, (u32)&g_state);
        }
        return 0;
    }

    if (op == KRX_OP_EVENT || op == KRX_OP_RENDER) {
        g_state.op = op;
        g_state.param = param;
        if (api->service && api->service->leaf_call) {
            return (int)api->service->leaf_call(2, (u32)&g_state);
        }
        return 0;
    }

    return 0;
}
