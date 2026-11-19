/*
 * KERN DFU Bare-Metal SRAM SysTick Hook Assembly Unit
 *
 * Modularized into:
 *   - hook_defs.inc:    Single-source SRAM/MMIO/struct offsets (verified against dfu_abi.h)
 *   - hook_main.S:      30 Hz SysTick ISR, anti-brick guard, link detection & KBHP v1.0 mirror
 *   - hook_drivers.S:   ST7565 SPI1, BK4829 LED, udiv32, 3x5 glyph stamper & clean reboot
 */
.syntax unified
.cpu cortex-m0plus
.thumb

.include "hook_defs.inc"

    .section .rodata.dfu_hook,"a"
    .balign 4
    .global __dfu_hook_start
    .global __dfu_hook_end
__dfu_hook_start:

.include "hook_main.S"
.include "hook_drivers.S"

__dfu_hook_end:
