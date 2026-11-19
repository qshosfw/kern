/*
 * KERN DFU Hook Bare-Metal Drivers & Rendering Primitives
 *
 * Position-independent Thumb-1 routines executed from upper SRAM (0x2000_3700+):
 *   - dfu_reboot_tools:  Writes backup-domain RTC_ALR tag (0xA500_4900) and falls into dfu_reboot
 *   - dfu_reboot:        Quiesces LED/SysTick/NVIC/USB/backlight and triggers AIRCR SYSRESETREQ
 *   - bk4829_set_led:    Bitbangs BK4829 REG_33 status LED (Off/Red/Green/Orange)
 *   - udiv32:            Restoring 32-bit unsigned division (Cortex-M0+ has no UDIV)
 *   - stamp_dec / glyph: 3x5 proportional digit & unit stamper
 *   - st7565_*:          Direct SPI1 strip streaming to ST7565 LCD
 */

dfu_reboot_tools:
    cpsid i
    // Enable PWR (1<<28) + RTCAPB (1<<10) clocks in RCC_APB1ENR (0x4002_103C)
    ldr r0, =RCC_APB1ENR
    ldr r1, [r0, #0]
    ldr r2, =0x10000400
    orrs r1, r1, r2
    str r1, [r0, #0]
    // Unlock backup domain: PWR_CR1.DBP (1<<8)
    ldr r3, =PWR_CR1
    ldr r1, [r3, #0]
    movs r2, #1
    lsls r2, r2, #8
    orrs r1, r1, r2
    str r1, [r3, #0]
    // Ensure LSI oscillator is running: RCC_CSR (+0x24 from APB1ENR)
    ldr r1, [r0, #0x24]
    movs r2, #1
    orrs r1, r1, r2
    str r1, [r0, #0x24]
    ldr r3, =50000
1001:
    ldr r1, [r0, #0x24]
    lsrs r1, r1, #2             // bit 1 = LSIRDY
    bcs 1002f
    subs r3, r3, #1
    bne 1001b
1002:
    // Ensure RTC clocked by LSI and enabled: RCC_BDCR (+0x20 from APB1ENR) |= 0x8200
    ldr r1, [r0, #0x20]
    ldr r2, =0x8200
    orrs r1, r1, r2
    str r1, [r0, #0x20]
    // Write 0xA500_4900 to RTC_ALRH/ALRL with RTOFF/CNF handshake
    ldr r0, =RTC_BASE
    ldr r3, =50000
1003:
    ldr r1, [r0, #0x04]
    lsrs r1, r1, #6             // bit 5 = RTOFF
    bcs 1004f
    subs r3, r3, #1
    bne 1003b
1004:
    ldr r1, [r0, #0x04]
    movs r2, #0x10              // CRL_CNF
    orrs r1, r1, r2
    str r1, [r0, #0x04]
    ldr r2, =KBHP_RTC_TOOLS_HI
    str r2, [r0, #0x20]         // RTC_ALRH = 0xA500
    ldr r2, =KBHP_RTC_TOOLS_LO
    str r2, [r0, #0x24]         // RTC_ALRL = 0x4900
    movs r2, #0x10
    bics r1, r1, r2
    str r1, [r0, #0x04]         // clear CNF to commit write
    ldr r3, =50000
1005:
    ldr r1, [r0, #0x04]
    lsrs r1, r1, #6             // wait RTOFF
    bcs 1006f
    subs r3, r3, #1
    bne 1005b
1006:
    movs r4, #1                 // keep PF8 backlight ON across warm reset into DFU Tools
    b dfu_reboot_common

dfu_reboot:
    movs r4, #0                 // normal EXIT reboot: turn PF8 backlight OFF
dfu_reboot_common:
    movs r0, #0
    bl bk4829_set_led
    cpsid i
    ldr r3, =SCS_SYSTICK_CSR
    movs r1, #0
    str r1, [r3]
    movs r2, #0x17
    lsls r2, r2, #4             // 0xE000_E180 (NVIC->ICER[0])
    mvns r1, r1
    str r1, [r3, r2]
    ldr r0, =RCC_APB1ENR
    subs r0, #0x10              // 0x4002_102C (RCC->APB1RSTR)
    movs r1, #1
    lsls r1, r1, #23
    str r1, [r0]
    movs r1, #0
    str r1, [r0]
    cmp r4, #0
    bne 1007f
    ldr r0, =0x50001428         // GPIOF->BRR
    movs r1, #1
    lsls r1, r1, #8             // PF8 backlight OFF
    str r1, [r0]
1007:
    dsb
    ldr r0, =SCB_AIRCR
    ldr r1, =AIRCR_SYSRESETREQ
    str r1, [r0]
    dsb
0:  b 0b
    .ltorg

// Bitbang BK4829 REG_33 status LED: r0 = led (0=Off, 1=Red, 2=Green, 3=Orange); clobbers r0..r3
bk4829_set_led:
    push {r4, r5, r6, lr}
    ldr r2, =GPIOB_BASE
    movs r1, #16
    lsls r1, r1, #8             // +0x1000
    adds r1, r2, r1             // GPIOF_BASE = 0x5000_1400
    // Ensure PB8 (SCL) & PB9 (SDA) and PF9 (CSN) are push-pull outputs
    movs r3, #0xF
    lsls r3, r3, #16
    ldr r4, [r2, #0]
    bics r4, r3
    movs r5, #5
    lsls r5, r5, #16
    orrs r4, r5
    str r4, [r2, #0]
    movs r3, #3
    lsls r3, r3, #18
    ldr r4, [r1, #0]
    bics r4, r3
    movs r5, #1
    lsls r5, r5, #18
    orrs r4, r5
    str r4, [r1, #0]

    movs r4, #1
    lsls r4, r4, #8             // PB8 = BK_SCL
    str r4, [r2, #0x28]         // SCL = 0
    lsls r5, r4, #1             // PB9 = BK_SDA / PF9 = BK_CSN
    str r5, [r1, #0x28]         // CSN = 0
    movs r3, #8
201: subs r3, r3, #1
    bne 201b

    ldr r3, =0x33900000         // (REG 0x33 << 24) | (0x9000 << 8)
    lsls r0, r0, #8
    orrs r0, r0, r3
    movs r6, #24
202:
    lsls r0, r0, #1
    bcs 203f
    str r5, [r2, #0x28]         // SDA = 0
    b 204f
203:
    str r5, [r2, #0x18]         // SDA = 1
204:
    movs r3, #4
205: subs r3, r3, #1
    bne 205b
    str r4, [r2, #0x18]         // SCL = 1
    movs r3, #8
206: subs r3, r3, #1
    bne 206b
    str r4, [r2, #0x28]         // SCL = 0
    movs r3, #4
207: subs r3, r3, #1
    bne 207b
    subs r6, r6, #1
    bne 202b
    str r5, [r1, #0x18]         // CSN = 1
    pop {r4, r5, r6, pc}

// Unsigned 32-bit restoring division: r0 = r0 / r1 (requires 0 < r1 < 0x8000_0000); clobbers r1..r3
udiv32:
    movs r2, #0
    movs r3, #0
    push {r4}
    movs r4, #32
210:
    lsls r0, r0, #1
    adcs r3, r3, r3
    lsls r2, r2, #1
    cmp r3, r1
    blo 211f
    subs r3, r3, r1
    adds r2, r2, #1
211:
    subs r4, r4, #1
    bne 210b
    movs r0, r2
    pop {r4}
    bx lr

// Stamp unsigned integer r0 (0..=999_999) into cursor r4 (advancing r4 += 4 per digit)
stamp_dec:
    push {r5, r6, r7, lr}
    movs r6, r0
    movs r7, #0
    adr r5, stamp_divs
    movs r2, #0
85: ldr r3, [r5, r2]
    movs r0, #0
86: cmp r6, r3
    blo 87f
    subs r6, r6, r3
    adds r0, r0, #1
    b 86b
87: cmp r0, #9
    bls 84f
    movs r0, #9
84: movs r1, r0
    orrs r1, r7
    bne 88f
    cmp r3, #1
    bne 89f
88: movs r7, #1
    bl stamp_glyph_3x5
89: adds r2, r2, #4
    cmp r2, #24
    blo 85b
    pop {r5, r6, r7, pc}

// Leaf stamper: glyph index r0 (0..=14) at cursor r4, advancing r4 += 4; preserves r2, clobbers r0/r1/r3
stamp_glyph_3x5:
    adr r1, stamp_glyphs
    lsls r3, r0, #1
    adds r0, r0, r3
    adds r1, r1, r0
    ldrb r3, [r1, #0]
    strb r3, [r4, #0]
    ldrb r3, [r1, #1]
    strb r3, [r4, #1]
    ldrb r3, [r1, #2]
    strb r3, [r4, #2]
    adds r4, r4, #4
    bx lr

// r0=page_cmd, r1=hw_col, r2=ptr(or 0 for zeros), r3=len
st7565_write_strip:
    push {r4, r5, lr}
    movs r4, r2
    movs r5, r3
    bl st7565_begin_page
    movs r2, #0
90:
    movs r0, #0
    cmp r4, #0
    beq 95f
    ldrb r0, [r4, r2]
95:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, r5
    blo 90b
    bl st7565_end
    pop {r4, r5, pc}

// r0=page_cmd, r1=hw_col; sets r3=SPI1_BASE, CS=0, A0=1 (spi_tx preserves r2, r4..r7)
st7565_begin_page:
    push {r4, r5, r6, lr}
    movs r4, r0
    lsrs r5, r1, #4
    movs r0, #0x10
    orrs r5, r5, r0             // col_hi
    movs r0, #0x0F
    ands r1, r1, r0
    movs r6, r1                 // col_lo
    ldr r3, =SPI1_BASE
    ldr r1, =GPIOB_BASE
    movs r0, #4
    lsls r2, r0, #8             // 0x400
    subs r2, r1, r2             // GPIOA_BASE = 0x5000_0000
    str r0, [r1, #0x28]         // CS = 0 (PB2)
    movs r1, #64
    str r1, [r2, #0x28]         // A0 = 0 (PA6)
    movs r0, r4
    bl spi_tx
    movs r0, r5
    bl spi_tx
    movs r0, r6
    bl spi_tx
80: ldr r1, [r3, #0x08]
    lsls r1, r1, #24
    bmi 80b
    movs r1, #64
    str r1, [r2, #0x18]         // A0 = 1
    pop {r4, r5, r6, pc}

st7565_end:
    ldr r0, [r3, #0x08]
    lsls r0, r0, #24
    bmi st7565_end
    ldr r0, =GPIOB_BASE
    movs r1, #4
    str r1, [r0, #0x18]         // CS = 1
    bx lr

// Sends byte r0 on SPI1 (r3=SPI1_BASE); preserves r2..r7
spi_tx:
    ldr r1, [r3, #0x08]
    lsrs r1, r1, #2
    bcc spi_tx
    strb r0, [r3, #0x0C]
    bx lr

    .balign 4
stamp_divs:
    .word 100000, 10000, 1000, 100, 10, 1
    .balign 4
stamp_glyphs:
    .byte 0x78, 0x44, 0x3C      // 0
    .byte 0x08, 0x7C, 0x00      // 1
    .byte 0x64, 0x54, 0x48      // 2
    .byte 0x44, 0x54, 0x28      // 3
    .byte 0x1C, 0x10, 0x7C      // 4
    .byte 0x5C, 0x54, 0x24      // 5
    .byte 0x78, 0x54, 0x74      // 6
    .byte 0x64, 0x14, 0x0C      // 7
    .byte 0x7C, 0x54, 0x7C      // 8
    .byte 0x5C, 0x54, 0x3C      // 9
    .byte 0x60, 0x10, 0x0C      // /
    .byte 0x24, 0x10, 0x48      // %
    .byte 0x7C, 0x48, 0x30      // b
    .byte 0xF8, 0x48, 0x30      // p
    .byte 0x50, 0x78, 0x28      // s

    .ltorg
