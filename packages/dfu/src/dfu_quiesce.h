/*
 * KERN DFU Hardware Quiescing & GPIO Lock Hardening
 *
 * Prepares the PY32F071xB silicon for deterministic entry into any primary
 * bootloader at 0x0800_0000 (Stock Quansheng 7.0x, Moto UF2, or custom):
 *   - PB10 (PTT): locked open-drain LOW -> triggers Stock BL & Moto DFU entry.
 *   - PB14/PB15 (Side keys): locked input pull-up HIGH -> required by Moto
 *     (`!PB10 && PB14 && PB15`) and cleanly releases chord keys.
 *   - PC13 (Flashlight): locked pull-down LOW -> prevents strobe latch-on.
 *   - PA13/PA14 (SWDIO/SWCLK): NEVER modified or locked (permanent SWD safety).
 *   - Restores HSI clock, stops PLL, asserts USB SE0 detach pulse on PA12,
 *     resets APB/DMA peripherals, and clears NVIC/SysTick pending state.
 */
#pragma once
#include "dfu_abi.h"

static void dfu_gpio_apply_and_lock(
    u32 base,
    u32 clear_2b,
    u32 pupdr_set,
    u32 moder_set,
    u32 lock_pins
) {
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

static void dfu_quiesce_hw(u32 apb2_keep) {
    /* 1. GPIOB: PB10 (PTT) LOW open-drain; PB4..6, PB14..15 pull-up inputs; lock via LCKR
     *    PB6 (MENU Col 1) is intentionally left unlocked (0xC430) so hook_main.S can scan MENU. */
    volatile u32* gpiob = (volatile u32*)0x50000400u;
    u32 ptt_pin = 1u << 10;
    gpiob[0x28 / 4] = ptt_pin;
    gpiob[0x04 / 4] |= ptt_pin;
    dfu_gpio_apply_and_lock(0x50000400u, 0xF0303FC0u, 0x50001540u, 0b01u << 20, 0xC430u);

    /* 2. GPIOC: PC13 (Flashlight) LOW pull-down input; lock via LCKR */
    volatile u32* rcc = (volatile u32*)0x40021000u;
    rcc[0x34 / 4] |= (1u << 2) | 0x1u;
    volatile u32* gpioc = (volatile u32*)0x50000800u;
    gpioc[0x28 / 4] = 1u << 13;
    dfu_gpio_apply_and_lock(0x50000800u, 0b11u << 26, 0b10u << 26, 0u, 1u << 13);

    /* 3. Switch SYSCLK back to HSI and stop PLL before resetting peripherals */
    rcc[0x08 / 4] = 0u;
    while ((rcc[0x08 / 4] & 0x38u) != 0u) {}
    rcc[0] &= ~(1u << 24);

    /* 4. Soft-disconnect USB peripheral if clocked */
    if ((rcc[0x3C / 4] & (1u << 23)) != 0u) {
        *(volatile u8*)0x40005C01u = 0u;
    }

    /* 5. Reset DMA + APB1 + APB2 (keeping SPI1 when SysTick LCD hook attaches) */
    rcc[0x28 / 4] = 1u;
    rcc[0x28 / 4] = 0u;
    rcc[0x2C / 4] = 0xFFFFFFFFu;
    rcc[0x2C / 4] = 0u;
    rcc[0x30 / 4] = ~apb2_keep;
    rcc[0x30 / 4] = 0u;
    rcc[0x3C / 4] = 0u;
    rcc[0x40 / 4] = apb2_keep;

    /* 6. Pull PA12 (USB D+) LOW briefly (SE0 detach) without touching PA13/PA14 (SWD) */
    volatile u32* gpioa = (volatile u32*)0x50000000u;
    gpioa[0x28 / 4] = 1u << 12;
    gpioa[0x0C / 4] &= ~(0b11u << 24);
    u32 pa_moder = gpioa[0];
    gpioa[0] = (pa_moder & ~(0b11u << 24)) | (0b01u << 24);
    for (volatile u32 d = 0; d < 600000u; d++) { __asm__ volatile("nop"); }
    gpioa[0] = pa_moder & ~(0b11u << 24);

    /* 7. Quiesce SysTick and clear all NVIC enables & pending bits */
    volatile u32* scs = (volatile u32*)0xE000E000u;
    scs[0x010 / 4] = 0u;
    scs[0xD04 / 4] = 1u << 25;
    scs[0x180 / 4] = 0xFFFFFFFFu;
    scs[0x280 / 4] = 0xFFFFFFFFu;
}

static inline u8 dfu_vectors_sane(u32 msp, u32 entry) {
    if ((msp & 3u) != 0u) return 0u;
    if ((msp - 0x20000400u) > 0x3C00u) return 0u;
    if ((entry & 1u) == 0u) return 0u;
    u32 addr = entry & ~1u;
    if (addr < (DFU_BL_FLASH_BASE + 8u) || addr >= DFU_BL_CODE_LIMIT) return 0u;
    return 1u;
}
