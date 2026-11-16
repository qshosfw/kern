.syntax unified
.cpu cortex-m0plus
.thumb


    .section .rodata.dfu_hook,"a"
    .balign 4
    .global __dfu_hook_start
    .global __dfu_hook_end
__dfu_hook_start:
    .thumb
    .thumb_func
dfu_systick_isr:
    push {r4, r5, r6, r7, lr}

    // 1. Keep PC13 flashlight explicitly LOW even if GPIOC->LCKR is absent on a stepping
    ldr r1, =0x50000400         // GPIOB_BASE
    movs r0, #4
    lsls r0, r0, #8             // 0x400 -> GPIOC_BASE = GPIOB_BASE + 0x400
    lsls r2, r0, #3             // 1 << 13
    adds r0, r0, r1
    str r2, [r0, #0x28]         // GPIOC->BRR = PC13

    ldr r4, =0x200036C0         // DFU_SRAM_STATE
    ldr r5, [r4, #0]            // flags: b0=STOCK_K1, b1=KEY_SEEN_HIGH, b2=USB, b3=WAS_FLASH, b4=ARROW_VIS, b5=BL_OFF, b6=NO_TAP_TOGGLE

    // 2. Side Key 1 (PB15 short press toggles backlight; >= 0.5 s hold reboots) & EXIT (PB3 Col 4 + PB15 Row 0 >= 0.5 s reboots)
    ldr r0, [r1, #0x10]         // IDR
    lsls r0, r0, #16
    bpl 13f                     // PB15 low directly -> Side Key 1 pressed
12:
    movs r2, #8                 // PB3 = Col 4
    str r2, [r1, #0x28]         // GPIOB->BRR = PB3
    ldr r3, [r1, #0x00]         // MODER
    movs r2, #0xC0              // PB3 bits [7:6]
    bics r3, r2
    movs r2, #0x40              // PB3 Output
    orrs r2, r3
    str r2, [r1, #0x00]
    movs r2, #16                // ~1 us keypad matrix trace settling @ 48 MHz
19:
    subs r2, r2, #1
    bne 19b
    ldr r0, [r1, #0x10]         // Sample PB15 with PB3 driven LOW
    str r3, [r1, #0x00]         // Immediately restore PB3 Input
    lsls r0, r0, #16
    bmi 1f                      // Released high
    movs r0, #0x40              // EXIT pressed -> NO_TAP_TOGGLE so release never toggles backlight
    orrs r5, r5, r0
    str r5, [r4, #0]
13:
    movs r0, #2
    tst r5, r0
    beq 3f

    // Block reboot (and any 0x0800_2800 Flash read) when Flash busy (SR_BSY==1), unlocked (CR_LOCK==0), flashing (state==2), or aborted (b3)
    ldr r6, =0x40022010         // FLASH_REGS (+0=SR, +4=CR)
    ldr r0, [r6, #0]            // FLASH->SR
    lsls r0, r0, #31
    bmi 3f
    ldr r0, [r6, #4]            // FLASH->CR
    cmp r0, #0
    bge 3f
    movs r0, #1
    tst r5, r0
    beq 2f
    ldr r0, =0x2000001C
    ldrb r0, [r0, #4]
    cmp r0, #2
    beq 3f
    movs r0, #8
    tst r5, r0
    bne 3f
2:
    ldr r0, [r4, #4]
    adds r0, r0, #1
    str r0, [r4, #4]
    cmp r0, #15
    blo 4f
    movs r0, #0x40              // >= 0.5 s hold: suppress short-press backlight toggle if antibrick blocks
    orrs r5, r5, r0
    str r5, [r4, #0]

    // Hardened antibrick app vector check at 0x0800_2800:
    // - MSP 4-byte aligned in SRAM 0x2000_0400..=0x2000_4000
    // - Reset, NMI, and HardFault vectors all odd Thumb pointers inside 0x0800_2840..0x0802_0000
    // - First word at *(Reset & !3) is neither erased (0xFFFF_FFFF) nor 0x0000_0000
    ldr r0, =0x08002800
    ldr r1, [r0, #0]
    lsls r2, r1, #30
    bne 3f
    ldr r2, =0x20000400
    subs r1, r1, r2
    movs r2, #0x3C
    lsls r2, r2, #8
    cmp r1, r2
    bhi 3f

    movs r3, #4
    ldr r2, =0x1D7C0            // 0x0802_0000 - 0x0800_2840
16:
    ldr r1, [r0, r3]
    lsls r7, r1, #31
    bpl 3f
    subs r1, r1, r0
    subs r1, #0x40
    cmp r1, r2
    bhs 3f
    adds r3, r3, #4
    cmp r3, #16
    blo 16b

    ldr r1, [r0, #4]
    movs r2, #3
    bics r1, r1, r2
    ldr r1, [r1, #0]
    adds r2, r1, #1
    beq 3f
    cmp r1, #0
    beq 3f

    // Valid firmware verified: reboot cleanly
    b dfu_reboot

1:
    // Released high: short Side Key 1 press (1 <= hold < 15, KEY_SEEN_HIGH set, !NO_TAP_TOGGLE) toggles PF8 backlight
    movs r0, #0x42
    ands r0, r0, r5
    cmp r0, #2
    bne 30f
    ldr r0, [r4, #4]
    cmp r0, #1
    blo 30f
    cmp r0, #15
    bhs 30f
    movs r0, #0x20
    eors r5, r5, r0             // toggle BL_OFF (flags bit 5)
    ldr r2, =0x50001400         // GPIOF_BASE
    movs r1, #1
    lsls r1, r1, #8             // PF8
    tst r5, r0
    beq 32f
    str r1, [r2, #0x28]         // GPIOF->BRR (backlight OFF)
    b 30f
32:
    str r1, [r2, #0x18]         // GPIOF->BSRR (backlight ON)
30:
    movs r0, #2                 // KEY_SEEN_HIGH = 1
    orrs r5, r5, r0
    movs r0, #0x40              // clear NO_TAP_TOGGLE
    bics r5, r5, r0
    str r5, [r4, #0]
3:
    movs r0, #0
    str r0, [r4, #4]            // key_hold_ticks = 0
4:
    // 3. Require known stock K1 DFU state struct before touching USB/UART/LCD state
    movs r0, #1
    tst r5, r0
    bne 11f
    pop {r4, r5, r6, r7, pc}
11:
    ldr r6, [r4, #8]            // r6 = tick
    adds r0, r6, #1
    str r0, [r4, #8]

    // Track mid-flash abort antibrick latch: set b3 when state==2, clear when state==3
    ldr r1, =0x2000001C
    ldrb r0, [r1, #4]           // 0x2000_0020 (state)
    cmp r0, #2
    bne 14f
    movs r0, #8
    orrs r5, r5, r0
    b 15f
14:
    cmp r0, #3
    bne 15f
    movs r0, #8
    bics r5, r5, r0
15:

    // 4. Detect live USB link (RCC_APB1ENR.USBDEN + USBD_ADDR + advancing USBD_FRAME)
    movs r7, #0
    ldr r0, =0x4002103C
    ldr r0, [r0]
    lsrs r0, r0, #24
    bcc 42f
    ldr r1, =0x40005C00
    ldrb r0, [r1, #0]
    lsls r0, r0, #25
    beq 42f
    ldrh r0, [r1, #0x0C]
    ldrh r2, [r4, #16]
    strh r0, [r4, #16]
    cmp r0, r2
    beq 40f
    movs r0, #0
    strb r0, [r4, #18]
    movs r7, #1
    b 43f
40:
    ldrb r0, [r4, #18]
    cmp r0, #4
    bhs 42f
    adds r0, r0, #1
    strb r0, [r4, #18]
    lsrs r7, r5, #2
    movs r0, #1
    ands r7, r7, r0
    b 43f
42:
    movs r0, #4
    strb r0, [r4, #18]
43:
    movs r0, #4
    bics r5, r5, r0
    lsls r0, r7, #2
    orrs r5, r5, r0
    str r5, [r4, #0]

    // 4b. Smart UART connection timer ([r4, #19], 0..=150 = 5 s idle hold):
    // USB active resets UART cleanly; otherwise gated on USART1EN+UE so pre-init SRAM never
    // false-triggers, and refreshes on >= 2 new DMA RX bytes or active UART flashing (state == 2).
    ldrb r1, [r4, #19]
    cmp r7, #0
    beq 440f
    movs r1, #0
    b 443f
440:
    cmp r1, #0
    beq 441f
    subs r1, r1, #1
441:
    ldr r0, =0x40021040
    ldr r0, [r0]
    lsrs r0, r0, #15            // bit 14 = USART1EN
    bcc 443f
    ldr r0, =0x40013800
    ldr r0, [r0, #0x0C]         // USART1->CR1
    lsrs r0, r0, #14            // bit 13 = UE
    bcc 443f

    ldr r0, =0x40020020         // DMA1_Channel2->CNDTR (2048 circular RX ring)
    ldrh r0, [r0, #0]
    ldrh r2, [r4, #48]          // prev_cndtr (starts at 2048)
    subs r3, r2, r0
    lsls r3, r3, #21
    lsrs r3, r3, #21            // (prev_cndtr - cur_cndtr) & 0x7FF
    cmp r3, #2
    blo 442f
    strh r0, [r4, #48]
    b 444f
442:
    ldr r2, =0x2000001C
    ldrb r2, [r2, #4]           // state
    cmp r2, #2
    bne 443f
444:
    movs r1, #150               // 5 s @ 30 Hz
443:
    strb r1, [r4, #19]

    // Track serial/command activity pulse across USB + UART only while USART1 is armed
    ldr r0, =0x40013800
    ldr r0, [r0, #0x0C]
    lsrs r0, r0, #14
    bcc 46f
    ldr r2, =0x2000001C
    ldr r0, =0x200009A0
    ldrh r0, [r0, #0]
    ldr r1, =0x200019E0
    ldrh r1, [r1, #0]
    lsls r1, r1, #16
    eors r0, r0, r1
    ldr r1, [r2, #0]
    eors r0, r0, r1
    ldr r1, =0x40020020
    ldrh r1, [r1, #0]
    eors r0, r0, r1
    ldr r1, [r4, #32]
    cmp r0, r1
    beq 45f
    str r0, [r4, #32]
    movs r0, #12
    strb r0, [r4, #31]
    b 46f
45:
    ldrb r0, [r4, #31]
    cmp r0, #0
    beq 46f
    subs r0, r0, #1
    strb r0, [r4, #31]
46:


    // 5. Compute Page 4 capsule [fill_start..fill_end), banner_idx (r7 = 0..=7), flash timer, and abort/stall state
    // Stack temp words: [sp, #0]=cur_page_1based, [sp, #4]=tot_pages, [sp, #8]=cutout_val, [sp, #12]=usb_active
    sub sp, #16
    str r7, [sp, #12]
    movs r0, #0
    str r0, [sp, #0]
    mvns r0, r0
    str r0, [sp, #8]

    ldr r2, =0x2000001C         // [+4]=state, [+5]=hs, [+6]=cur, [+8]=tot
    ldrb r0, [r2, #4]
    cmp r0, #3
    beq 5f
    cmp r0, #2
    bne 6f

    // Flashing active -> reset elapsed cache, latch flash_start_tick, detect >= 10s mid-flash stall
    movs r0, #0
    mvns r0, r0
    strh r0, [r4, #36]          // flash_elapsed_s = 0xFFFF
    ldr r0, [r4, #24]
    cmp r0, #0
    bne 53f
    ldr r0, [r4, #8]            // updated tick (>= 1)
    str r0, [r4, #24]
53:
    ldrh r0, [r2, #6]           // cur
    ldrh r1, [r4, #44]          // last_flash_cur
    cmp r0, r1
    beq 54f
    strh r0, [r4, #44]
    movs r1, #0
    strh r1, [r4, #46]          // stall_ticks = 0
    b 55f
54:
    cmp r0, #0
    beq 55f
    ldrh r1, [r4, #46]
    movs r3, #150
    lsls r3, r3, #1             // 300 ticks = 10 s @ 30 Hz
    cmp r1, r3
    bhs 55f
    adds r1, r1, #1
    strh r1, [r4, #46]
55:
    movs r7, #4                 // banner 4: "Flashing page X/Y"
    ldrh r1, [r4, #46]
    movs r3, #150
    lsls r3, r3, #1
    cmp r1, r3
    blo 56f
    movs r7, #7                 // banner 7: "Aborted - reflash" (stalled)
56:
    adds r0, r0, #1             // cur_page_1based
    str r0, [sp, #0]
    ldrh r1, [r2, #8]           // tot_pages
    str r1, [sp, #4]

    // Shared pct calculation: r0=num, r1=den -> r2=0..=100, then fall into 51:
50:
    movs r3, #100
    muls r0, r3, r0
    movs r2, #0
    cmp r1, #0
    beq 51f
501:
    cmp r2, #100
    bhs 51f
    cmp r0, r1
    blo 51f
    subs r0, r0, r1
    adds r2, r2, #1
    b 501b
51:
    str r2, [sp, #8]
    movs r5, #0
    movs r6, r2
    b 7f
    .ltorg

5:
    // Flash complete -> latch elapsed seconds once, alternate "100%" / "<N>s" every ~1.06 s
    ldrh r0, [r4, #36]
    adds r1, r0, #1
    lsls r1, r1, #16
    bne 58f
    ldr r1, [r4, #24]
    movs r0, #0
    cmp r1, #0
    beq 57f
    ldr r0, [r4, #8]
    subs r0, r0, r1
    adds r0, r0, #15
    movs r1, #30
    bl udiv32
    movs r1, #125
    lsls r1, r1, #3             // 1000
    cmp r0, r1
    blo 57f
    subs r0, r1, #1             // clamp to 999 s
57:
    strh r0, [r4, #36]
    movs r1, #0
    str r1, [r4, #24]
    strh r1, [r4, #46]
58:
    movs r2, #100
    movs r1, #32
    tst r6, r1
    beq 59f
    ldrh r2, [r4, #36]
    movs r1, #4
    lsls r1, r1, #8             // 0x400 marker -> "<N>s"
    orrs r2, r2, r1
59:
    str r2, [sp, #8]
    movs r5, #0
    movs r6, #100
    movs r7, #5                 // banner 5: "Flash complete"
    b 7f

6:
    // Check mid-flash abort (WAS_FLASH latched while state != 2 && state != 3)
    ldr r0, [r4, #0]
    movs r1, #8
    tst r0, r1
    beq 66f
    movs r0, #0
    strh r0, [r4, #46]
    ldrh r0, [r2, #6]
    ldrh r1, [r2, #8]
    movs r7, #7                 // banner 7: "Aborted - reflash"
    b 50b

66:
    // If Flash is already unlocked (CR_LOCK==0) or busy erasing before state==2 settles,
    // dock the capsule cleanly at 0% ([0..2)) so the comet/ping-pong pill never freezes mid-bar.
    ldr r3, =0x40022010
    ldr r0, [r3, #4]            // FLASH->CR
    cmp r0, #0
    bge 691f
    ldr r0, [r3, #0]            // FLASH->SR
    lsls r0, r0, #31
    bpl 660f
691:
    movs r5, #0
    movs r6, #2
    movs r7, #2
    b 7f
660:
    ldrb r2, [r2, #5]           // hs (0/1)
    ldrb r3, [r4, #31]          // serial_pulse (0..12)
    cmp r3, #0
    beq 68f
    // Serial activity burst -> fast directional comet pill (16 px)
    lsls r0, r6, #1
    adds r0, r0, r6
    movs r1, #127
    ands r0, r0, r1
    cmp r0, #84
    bls 67f
    subs r0, r0, #84
67: movs r5, r0
    movs r1, #16
    movs r7, #3                 // banner 3: "Link active"
    cmp r2, #0
    beq 65f
    movs r7, #2                 // keep "Ready to flash" once handshake completed
    b 65f

68:
    ldrb r0, [r4, #19]          // uart_latched
    movs r1, r7
    orrs r1, r1, r0
    orrs r1, r1, r2
    bne 60f
    lsrs r6, r6, #1             // idle standby: slow gentle ping-pong
60:
    movs r0, #63
    ands r0, r0, r6
    movs r1, #64
    tst r6, r1
    beq 61f
    movs r1, #63
    subs r0, r1, r0
61: lsrs r1, r0, #1
    adds r5, r0, r1             // 0..=94

    cmp r2, #0
    beq 62f
    // Handshake done -> banner 2 ("Ready to flash"), 28 px pill
    movs r1, #28
    movs r7, #2
    b 69f
62:
    cmp r7, #0
    beq 63f
    movs r1, #14                // USB connected -> banner 1, 14 px pill
    b 69f
63:
    ldrb r0, [r4, #19]
    cmp r0, #0
    beq 64f
    movs r1, #14                // UART connected -> banner 6, 14 px pill
    movs r7, #6
    b 69f
64:
    movs r1, #8                 // Standby -> banner 0, 8 px pill
    movs r7, #0
69:
    movs r0, #100
    subs r0, r0, r1
    cmp r5, r0
    bls 65f
    movs r5, r0
65:
    movs r6, r5
    adds r6, r6, r1

7:
    // 6a. Page 4 capsule progress bar redraw on diff (dashed pattern on abort/stall r7 == 7)
    lsls r2, r5, #8
    orrs r2, r2, r6
    lsls r0, r7, #24
    orrs r2, r2, r0
    ldr r3, [r4, #12]
    cmp r3, r2
    beq 78f
    str r2, [r4, #12]

    adds r5, r5, #2
    adds r6, r6, #2
    movs r0, #0xB4
    movs r1, #16                // HW col 12 + 4 = 16
    bl st7565_begin_page
    movs r0, #0xE0              // left capsule cap (col 12, Y=37..39 -> 0xE0 on P4)
    bl spi_tx
    movs r2, #1
71:
    movs r0, #0x10              // sleek 6 px empty capsule top border (Y=36 -> P4 bit 4)
    cmp r2, r5
    blo 74f
    cmp r2, r6
    bhs 74f
    movs r0, #0xD0              // 2 px inner pill fill (`Y=38..39`: 0x10 | 0xC0)
    cmp r7, #7
    bne 74f
    movs r1, #2
    tst r2, r1                  // Aborted: 2 px dashed pattern (`0xD0, 0xD0, 0x10, 0x10`)
    bne 74f
    movs r0, #0x10
74:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, #103
    blo 71b
    movs r0, #0xE0              // right capsule cap (col 115)
    bl spi_tx
    bl st7565_end

78:
    // 6b. Page 5 + Page 6 dynamic status banner redraw on (banner << 12) | cur_page_lsb change
    lsls r0, r7, #12
    ldr r1, [sp, #0]
    lsls r1, r1, #20
    lsrs r1, r1, #20
    orrs r0, r0, r1
    ldrh r1, [r4, #28]
    cmp r0, r1
    beq 79f
    strh r0, [r4, #28]

    ldr r5, =0x20003000         // DFU_SRAM_BANNERS: 8 x [84 B P5 + 84 B P6] (`cols 22..105`)
    movs r0, #168
    muls r0, r0, r7
    adds r5, r5, r0

    // When flashing (banner == 4), stamp dynamic "<cur>/<tot>" at X=76 (+54 inside slice) across P5 & P6
    cmp r7, #4
    bne 76f
    movs r0, #0
    movs r1, #0
    movs r2, #0
    movs r3, #0
    push {r0, r1, r2, r3}
    push {r0, r1, r2, r3}
    mov r4, sp
    ldr r0, [sp, #32]           // cur_page_1based
    bl stamp_dec
    movs r0, #10                // '/'
    bl stamp_glyph_3x5
    ldr r0, [sp, #36]           // tot_pages
    bl stamp_dec

    adds r5, #54
    mov r6, r5
    adds r6, #84
    movs r4, #7
    movs r2, #0
75:
    mov r0, sp
    ldrb r3, [r0, r2]           // raw 3x5 glyph byte (bits 2..6 at y=46)
    ldrb r1, [r5, r2]
    ands r1, r1, r4             // preserve capsule bottom border (P5 bits 0..1; bit 2 clear)
    lsls r0, r3, #3             // -> y=49 Page 5 (bits 5..7)
    orrs r1, r1, r0
    strb r1, [r5, r2]

    ldrb r1, [r6, r2]
    bics r1, r1, r4             // clear P6 bits 0..2, preserve EXIT (bits 5..7)
    lsrs r3, r3, #5             // -> y=49 Page 6 (bits 0..1)
    orrs r1, r1, r3
    strb r1, [r6, r2]

    adds r2, r2, #1
    cmp r2, #28
    blo 75b
    subs r5, #54
    add sp, #32
    ldr r4, =0x200036C0

76:
    movs r0, #0xB5
    movs r1, #26                // HW col 22 + 4 = 26
    movs r2, r5
    movs r3, #84
    bl st7565_write_strip
    adds r5, #84
    movs r0, #0xB6
    movs r1, #26
    movs r2, r5
    movs r3, #84
    bl st7565_write_strip

79:
    // 6c. Page 1+2 radio LCD cutout ("0%".."100%" or "<N>s", Y=13..17, X=17..=32) on change
    ldr r5, [sp, #8]
    lsrs r0, r5, #10
    cmp r0, #1
    beq 81f
    cmp r5, #100
    bhi 96f
    movs r6, r5
    b 800f
81:
    lsls r6, r5, #22
    lsrs r6, r6, #22
800:
    ldrh r0, [r4, #38]
    cmp r5, r0
    beq 96f
    strh r5, [r4, #38]

    movs r0, #0
    movs r1, #0
    movs r2, #0
    movs r3, #0
    push {r0, r1, r2, r3}     // 16 B raw y=46 glyph strip at [sp, #0..16]
    mov r4, sp
    movs r0, #0
    cmp r6, #100
    bhs 82f
    adds r0, r0, #2
    cmp r6, #10
    bhs 82f
    adds r0, r0, #2
82: adds r4, r4, r0
    movs r0, r6
    bl stamp_dec
    movs r0, #11                // '%'
    lsrs r1, r5, #10
    cmp r1, #1
    bne 83f
    movs r0, #14                // 's'
83: bl stamp_glyph_3x5

    // Stream Page 1 (pass 0: << 3 + 0x06 bezel / 0x07 antenna cols 0..2) and Page 2 (pass 1: >> 5 + 0x70 bezel / 0xF0 cols 0,15).
    movs r4, #0
101:
    movs r0, #0xB1
    adds r0, r0, r4
    movs r1, #21                // HW col 17 + 4 = 21
    bl st7565_begin_page        // sets r3=SPI1_BASE, CS=0, A0=1; spi_tx preserves r2, r4..r7
    mov r5, sp
    movs r6, #0
102:
    ldrb r0, [r5, r6]
    cmp r4, #0
    bne 103f
    lsls r0, r0, #3
    movs r1, #6
    cmp r6, #3
    bhs 105f
    adds r1, r1, #1
    b 105f
103:
    lsrs r0, r0, #5
    movs r1, #0x70
    cmp r6, #0
    beq 104f
    cmp r6, #15
    bne 105f
104:
    movs r1, #0xF0
105:
    orrs r0, r0, r1
    bl spi_tx
    adds r6, r6, #1
    cmp r6, #16
    blo 102b
    bl st7565_end
    adds r4, r4, #1
    cmp r4, #2
    blo 101b
    add sp, #16
    ldr r4, =0x200036C0

96:
    // 6c-top. Page 1 top label (`y=13, X=73..=112`):
    // r5 = 0 (blank when disconnected), 1 (VID:PID when USB connected), >= 1200 (<baud>bps when UART connected)
    movs r5, #0
    ldr r0, [sp, #12]           // usb_active
    cmp r0, #0
    beq 808f
    movs r5, #1
    b 810f
808:
    ldrb r0, [r4, #19]          // uart_timer
    cmp r0, #0
    beq 810f
    ldr r0, =0x40021040         // RCC_APB2ENR
    ldr r0, [r0]
    lsrs r0, r0, #15            // bit 14 = USART1EN
    bcc 810f
    ldr r0, =0x40013800         // USART1_BASE
    ldr r1, [r0, #0x0C]         // CR1
    lsrs r1, r1, #14            // bit 13 = UE
    bcc 810f
    ldrh r1, [r0, #0x08]        // BRR
    cmp r1, #49                 // 48e6/49 = 979_591 < 1_000_000 (fits "NNNNNNbps" in 40 cols)
    blo 810f
    ldr r0, =48000000
    bl udiv32
    movs r1, #150
    lsls r1, r1, #3             // 1200
    lsrs r2, r1, #1             // 600
    adds r0, r0, r2
    bl udiv32
    movs r1, #150
    lsls r1, r1, #3             // 1200
    muls r0, r0, r1
    cmp r0, r1
    blo 810f
    movs r5, r0
810:
    ldr r0, [r4, #40]           // last_top_key
    cmp r5, r0
    beq 819f
    str r5, [r4, #40]
    cmp r5, #1
    bne 811f
    movs r0, #0xB1
    movs r1, #77                // HW col 73 + 4 = 77
    ldr r2, =0x20003580         // DFU_SRAM_VIDPID (40 B)
    movs r3, #40
    bl st7565_write_strip
    b 819f
811:
    movs r0, #0
    movs r1, #0
    movs r2, #0
    push {r0, r1, r2}
    push {r0, r1, r2}
    push {r0, r1, r2}         // 36 B zeroed raw y=46 strip (also blank fill when r5 == 0)
    cmp r5, #0
    beq 814f
    mov r4, sp
    movs r0, r5
    bl stamp_dec
    movs r0, #12                // 'b'
    bl stamp_glyph_3x5
    movs r0, #13                // 'p'
    bl stamp_glyph_3x5
    movs r0, #14                // 's'
    bl stamp_glyph_3x5
    mov r0, sp
    subs r1, r4, r0             // len = 4 * n_chars
    movs r5, #42
    subs r5, r5, r1
    lsrs r5, r5, #1             // left_col = (42 - len) >> 1 (centered at X=93)
    adds r6, r5, r1             // end_col
    b 815f
814:
    movs r5, #40
    movs r6, #0
815:
    movs r0, #0xB1
    movs r1, #77                // HW col 73 + 4 = 77
    bl st7565_begin_page
    mov r4, sp
    movs r2, #0
812:
    movs r0, #0
    cmp r2, r5
    blo 813f
    cmp r2, r6
    bhs 813f
    subs r1, r2, r5
    ldrb r0, [r4, r1]
    lsrs r0, r0, #1             // shift y=46 -> y=13 Page 1 (bits 1..5, 'p' descender bit 6)
813:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, #40
    blo 812b
    bl st7565_end
    add sp, #36
    ldr r4, =0x200036C0

819:
    // 6d. Blink splash arrow (P2/P3 cols 45..=62, HW col 49) only in Standby (r7 == 0)
    ldr r5, [r4, #0]            // flags
    movs r6, #1                 // desired arrow_vis = 1 whenever USB/UART/flashing/complete/aborted
    cmp r7, #0
    bne 97f
    ldr r0, [r4, #8]            // tick
    lsrs r0, r0, #4
    eors r6, r6, r0             // invert ~1 Hz bit
97:
    movs r1, #1
    ands r6, r6, r1
    lsrs r0, r5, #4
    ands r0, r0, r1             // previous arrow_vis (flags bit 4)
    cmp r0, r6
    beq 900f
    movs r0, #16
    eors r5, r5, r0             // toggle flags bit 4 cleanly to match r6
    str r5, [r4, #0]

    movs r5, #0
    cmp r6, #0
    beq 98f
    ldr r5, =0x20003540         // DFU_SRAM_ARROW (P2 +0, P3 +19)
98:
    movs r6, #0xB2
94:
    movs r0, r6
    movs r1, #49                // HW col 45 + 4 = 49
    movs r2, r5
    movs r3, #19
    bl st7565_write_strip
    cmp r5, #0
    beq 93f
    adds r5, r5, #19
93:
    adds r6, r6, #1
    cmp r6, #0xB4
    blo 94b

900:
    // 6e. BK4829 REG_33 status LED choreography (updated only on change):
    // 0=Off, 1=Red, 2=Green, 3=Orange
    movs r0, #2                 // connected / ready / complete -> solid Green
    cmp r7, #7
    bne 901f
    ldr r1, [r4, #8]            // Aborted / stalled -> fast Red blink (~3.7 Hz)
    movs r0, #0
    movs r2, #4
    tst r1, r2
    beq 905f
    movs r0, #1
    b 905f
901:
    cmp r7, #4
    bne 902f
    ldr r1, [sp, #0]            // Flashing -> Green/Orange alternating every flash page
    movs r0, #1
    ands r0, r0, r1
    adds r0, r0, #2
    b 905f
902:
    cmp r7, #3
    bne 903f
    ldr r1, [r4, #8]            // Link active -> Green/Orange flutter
    lsrs r1, r1, #1
    movs r0, #1
    ands r0, r0, r1
    adds r0, r0, #2
    b 905f
903:
    cmp r7, #0
    bne 905f
    ldr r1, [r4, #8]            // Standby -> subtle Green heartbeat blip every ~2.1 s
    movs r2, #0x3F
    ands r1, r1, r2
    movs r0, #0
    cmp r1, #3
    bhs 905f
    movs r0, #2
905:
    ldrb r1, [r4, #30]
    cmp r0, r1
    beq 99f
    strb r0, [r4, #30]
    bl bk4829_set_led

99:
    add sp, #16
    pop {r4, r5, r6, r7, pc}
    .ltorg

dfu_reboot:
    movs r0, #0
    bl bk4829_set_led
    cpsid i
    ldr r3, =0xE000E010
    movs r1, #0
    str r1, [r3]
    movs r2, #0x17
    lsls r2, r2, #4             // 0xE000_E180 (NVIC->ICER[0])
    mvns r1, r1
    str r1, [r3, r2]
    ldr r0, =0x4002103C
    subs r0, #0x10              // 0x4002_102C (RCC->APB1RSTR)
    movs r1, #1
    lsls r1, r1, #23
    str r1, [r0]
    movs r1, #0
    str r1, [r0]
    ldr r0, =0x50001428
    movs r1, #1
    lsls r1, r1, #8
    str r1, [r0]
    dsb
    ldr r0, =0xE000ED0C
    ldr r1, =0x05FA0004
    str r1, [r0]
    dsb
0:  b 0b
    .ltorg

// Bitbang BK4829 REG_33 status LED: r0 = led (0=Off, 1=Red, 2=Green, 3=Orange); clobbers r0..r3
bk4829_set_led:
    push {r4, r5, r6, lr}
    ldr r2, =0x50000400         // GPIOB_BASE
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
    ldr r3, =0x40013000
    ldr r1, =0x50000400         // GPIOB_BASE
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
    ldr r0, =0x50000400
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
__dfu_hook_end:

