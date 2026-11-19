/*
 * KERN DFU Hook Core: 30 Hz SysTick Telemetry, Hardware Guard & Anti-Brick ISR
 *
 * Pipeline per 30 Hz tick:
 *   1. Hardware Safety Lock: enforces PC13 flashlight LOW every tick.
 *   2. Input & Anti-Brick Guard:
 *        - Side1 short press: toggles PF8 backlight
 *        - Side1 / EXIT (>= 0.5 s hold): verified clean reboot to OS
 *        - MENU (tap, ~66 ms): verified reboot into `/apps/dfu.mod` DFU Tools menu
 *          via backup-domain `RTC_ALR` (`0xA500_4900`, zero main firmware overhead)
 *      All reboot/tools actions are gated on FLASH->SR/CR idle + hardened 4-point
 *      vector validation at 0x0800_2800.
 *   3. Link Detection (STOCK_K1 only): USB SOF activity + USART1 DMA RX ring.
 *   4. UI State Machine & KBHP v1.0 Mirror: updates live KbhpEnvelope telemetry.
 *   5. Differential LCD & BK4829 LED Choreography:
 *        - Progress bar (Page 4) appears ONLY while flashing, complete, or aborted
 *        - Arrow (Pages 2-3) represents live connection state (blink/solid/pulse/off)
 *        - Banners fit strictly inside Page 5 (84 B stride, zero Page 6 spill)
 */

    .thumb
    .thumb_func
dfu_systick_isr:
    push {r4, r5, r6, r7, lr}

    // 1. Keep PC13 flashlight explicitly LOW even if GPIOC->LCKR is absent on a stepping
    ldr r1, =GPIOB_BASE
    movs r0, #4
    lsls r0, r0, #8             // 0x400 -> GPIOC_BASE = GPIOB_BASE + 0x400
    lsls r2, r0, #3             // 1 << 13
    adds r0, r0, r1
    str r2, [r0, #0x28]         // GPIOC->BRR = PC13

    ldr r4, =DFU_SRAM_STATE
    ldr r5, [r4, #ST_FLAGS]

    // 2. Scan Side1 (PB15 direct), EXIT (PB3 Col 4 + PB15), and MENU (PB6 Col 1 + PB15)
    //    r7 = key kind: 0 = Side1, 1 = EXIT, 2 = MENU
    movs r7, #0
    ldr r0, [r1, #0x10]         // IDR
    lsls r0, r0, #16
    bpl 13f                     // PB15 low directly -> Side Key 1 pressed

    // Test EXIT: drive PB3 (Col 4) LOW briefly, sample PB15, restore MODER
    movs r2, #8                 // PB3
    str r2, [r1, #0x28]         // GPIOB->BRR = PB3
    ldr r3, [r1, #0x00]         // MODER
    movs r2, #0xC0              // PB3 bits [7:6]
    bics r3, r2
    movs r2, #0x40              // PB3 Output
    orrs r2, r3
    str r2, [r1, #0x00]
    movs r2, #16
19: subs r2, r2, #1
    bne 19b
    ldr r0, [r1, #0x10]
    str r3, [r1, #0x00]
    lsls r0, r0, #16
    bpl 120f                    // EXIT pressed -> r7 = 1

    // Test MENU: drive PB6 (Col 1) LOW briefly, sample PB15, restore MODER
    movs r2, #0x40              // PB6
    str r2, [r1, #0x28]         // GPIOB->BRR = PB6
    ldr r3, [r1, #0x00]         // MODER
    movs r2, #0x30
    lsls r2, r2, #8             // 0x3000: PB6 bits [13:12]
    bics r3, r2
    movs r2, #0x10
    lsls r2, r2, #8             // 0x1000: PB6 Output
    orrs r2, r3
    str r2, [r1, #0x00]
    movs r2, #16
190: subs r2, r2, #1
    bne 190b
    ldr r0, [r1, #0x10]
    str r3, [r1, #0x00]
    lsls r0, r0, #16
    bmi 1f                      // Released high
    movs r7, #2                 // MENU pressed
    b 121f
120:
    movs r7, #1                 // EXIT pressed
121:
    movs r0, #FLAG_NO_TAP       // EXIT/MENU suppress short-press backlight toggle
    orrs r5, r5, r0
    str r5, [r4, #ST_FLAGS]
13:
    movs r0, #FLAG_KEY_SEEN_HI
    tst r5, r0
    beq 3f

    // Block reboot/tools when Flash busy (SR_BSY==1), unlocked (CR_LOCK==0), flashing (state==2), or aborted
    ldr r6, =FLASH_REGS_BASE
    ldr r0, [r6, #0]            // FLASH->SR
    lsls r0, r0, #31
    bmi 3f
    ldr r0, [r6, #4]            // FLASH->CR
    cmp r0, #0
    bge 3f
    movs r0, #FLAG_STOCK_K1
    tst r5, r0
    beq 2f
    ldr r0, =STOCK_BL_STATE
    ldrb r0, [r0, #4]
    cmp r0, #2
    beq 3f
    movs r0, #FLAG_WAS_FLASH
    tst r5, r0
    bne 3f
2:
    ldr r0, [r4, #ST_KEY_HOLD]
    adds r0, r0, #1
    str r0, [r4, #ST_KEY_HOLD]
    movs r1, #15                // 0.5 s hold for EXIT / Side1 reboot
    cmp r7, #2
    bne 20f
    movs r1, #2                 // ~66 ms tap debounce for MENU -> Tools
20:
    cmp r0, r1
    blo 4f
    movs r0, #FLAG_NO_TAP
    orrs r5, r5, r0
    str r5, [r4, #ST_FLAGS]

    // Shared hardened anti-brick app vector check at 0x0800_2800 (preserves r7):
    // - MSP 4-byte aligned in SRAM 0x2000_0400..=0x2000_4000
    // - Reset, NMI, and HardFault vectors all odd Thumb pointers inside 0x0800_2840..0x0802_0000
    // - First word at *(Reset & !3) is neither erased (0xFFFF_FFFF) nor 0x0000_0000
    ldr r0, =APP_FLASH_BASE
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
    lsls r6, r1, #31
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

    // Valid firmware verified: MENU opens DFU Tools via RTC_ALR, EXIT/Side1 reboots to OS
    cmp r7, #2
    bne 21f
    b dfu_reboot_tools
21:
    b dfu_reboot

1:
    // Released high: short Side Key 1 press (1 <= hold < 15, KEY_SEEN_HI set, !NO_TAP) toggles PF8 backlight
    movs r0, #(FLAG_NO_TAP | FLAG_KEY_SEEN_HI)
    ands r0, r0, r5
    cmp r0, #FLAG_KEY_SEEN_HI
    bne 30f
    ldr r0, [r4, #ST_KEY_HOLD]
    cmp r0, #1
    blo 30f
    cmp r0, #15
    bhs 30f
    movs r0, #FLAG_BL_OFF
    eors r5, r5, r0             // toggle BL_OFF
    ldr r2, =GPIOF_BASE
    movs r1, #1
    lsls r1, r1, #8             // PF8
    tst r5, r0
    beq 32f
    str r1, [r2, #0x28]         // GPIOF->BRR (backlight OFF)
    b 30f
32:
    str r1, [r2, #0x18]         // GPIOF->BSRR (backlight ON)
30:
    movs r0, #FLAG_KEY_SEEN_HI
    orrs r5, r5, r0
    movs r0, #FLAG_NO_TAP
    bics r5, r5, r0
    str r5, [r4, #ST_FLAGS]
3:
    movs r0, #0
    str r0, [r4, #ST_KEY_HOLD]
4:
    // 3. Require verified stock K1 DFU state struct before touching USB/UART/LCD state
    movs r0, #FLAG_STOCK_K1
    tst r5, r0
    bne 11f
    pop {r4, r5, r6, r7, pc}
11:
    ldr r6, [r4, #ST_TICK]
    adds r0, r6, #1
    str r0, [r4, #ST_TICK]

    // Track mid-flash abort antibrick latch: set FLAG_WAS_FLASH when state==2, clear when state==3
    ldr r1, =STOCK_BL_STATE
    ldrb r0, [r1, #4]           // 0x2000_0020 (state)
    cmp r0, #2
    bne 14f
    movs r0, #FLAG_WAS_FLASH
    orrs r5, r5, r0
    b 15f
14:
    cmp r0, #3
    bne 15f
    movs r0, #FLAG_WAS_FLASH
    bics r5, r5, r0
15:

    // 4. Detect live USB link (RCC_APB1ENR.USBDEN + USBD_ADDR + advancing USBD_FRAME)
    movs r7, #0
    ldr r0, =RCC_APB1ENR
    ldr r0, [r0]
    lsrs r0, r0, #24
    bcc 42f
    ldr r1, =USBD_BASE
    ldrb r0, [r1, #0]
    lsls r0, r0, #25
    beq 42f
    ldrh r0, [r1, #0x0C]
    ldrh r2, [r4, #ST_USB_FRAME]
    strh r0, [r4, #ST_USB_FRAME]
    cmp r0, r2
    beq 40f
    movs r0, #0
    strb r0, [r4, #ST_USB_MISS]
    movs r7, #1
    b 43f
40:
    ldrb r0, [r4, #ST_USB_MISS]
    cmp r0, #4
    bhs 42f
    adds r0, r0, #1
    strb r0, [r4, #ST_USB_MISS]
    lsrs r7, r5, #2
    movs r0, #1
    ands r7, r7, r0
    b 43f
42:
    movs r0, #4
    strb r0, [r4, #ST_USB_MISS]
43:
    movs r0, #FLAG_USB_ACTIVE
    bics r5, r5, r0
    lsls r0, r7, #2
    orrs r5, r5, r0
    str r5, [r4, #ST_FLAGS]

    // 4b. Smart UART connection timer (ST_UART_TIMER, 0..=150 = 5 s idle hold)
    ldrb r1, [r4, #ST_UART_TIMER]
    cmp r7, #0
    beq 440f
    movs r1, #0
    b 443f
440:
    cmp r1, #0
    beq 441f
    subs r1, r1, #1
441:
    ldr r0, =RCC_APB2ENR
    ldr r0, [r0]
    lsrs r0, r0, #15            // bit 14 = USART1EN
    bcc 443f
    ldr r0, =USART1_BASE
    ldr r0, [r0, #0x0C]         // USART1->CR1
    lsrs r0, r0, #14            // bit 13 = UE
    bcc 443f

    ldr r0, =DMA1_CH2_CNDTR     // 2048 circular RX ring
    ldrh r0, [r0, #0]
    ldrh r2, [r4, #ST_PREV_CNDTR]
    subs r3, r2, r0
    lsls r3, r3, #21
    lsrs r3, r3, #21            // (prev_cndtr - cur_cndtr) & 0x7FF
    cmp r3, #2
    blo 442f
    strh r0, [r4, #ST_PREV_CNDTR]
    b 444f
442:
    ldr r2, =STOCK_BL_STATE
    ldrb r2, [r2, #4]           // state
    cmp r2, #2
    bne 443f
444:
    movs r1, #150               // 5 s @ 30 Hz
443:
    strb r1, [r4, #ST_UART_TIMER]

    // Track serial/command activity pulse across USB + UART while USART1 is armed
    ldr r0, =USART1_BASE
    ldr r0, [r0, #0x0C]
    lsrs r0, r0, #14
    bcc 46f
    ldr r2, =STOCK_BL_STATE
    ldr r0, =STOCK_USB_RING
    ldrh r0, [r0, #0]
    ldr r1, =STOCK_UART_RING
    ldrh r1, [r1, #0]
    lsls r1, r1, #16
    eors r0, r0, r1
    ldr r1, [r2, #0]
    eors r0, r0, r1
    ldr r1, =DMA1_CH2_CNDTR
    ldrh r1, [r1, #0]
    eors r0, r0, r1
    ldr r1, [r4, #ST_SERIAL_HASH]
    cmp r0, r1
    beq 45f
    str r0, [r4, #ST_SERIAL_HASH]
    movs r0, #12
    strb r0, [r4, #ST_SERIAL_PULSE]
    b 46f
45:
    ldrb r0, [r4, #ST_SERIAL_PULSE]
    cmp r0, #0
    beq 46f
    subs r0, r0, #1
    strb r0, [r4, #ST_SERIAL_PULSE]
46:

    // 5. Compute UI state (r7 = 0..=7) and Page 4 progress bar ([r5..r6), or r5=0xFF hidden)
    // Stack temp words: [sp, #0]=cur_page_1based, [sp, #4]=tot_pages, [sp, #8]=cutout_val, [sp, #12]=usb_active
    sub sp, #16
    str r7, [sp, #12]
    movs r0, #0
    str r0, [sp, #0]
    mvns r0, r0
    str r0, [sp, #8]

    ldr r2, =STOCK_BL_STATE     // [+4]=state, [+5]=hs, [+6]=cur, [+8]=tot
    ldrb r0, [r2, #4]
    cmp r0, #3
    beq 5f
    cmp r0, #2
    bne 6f

    // Flashing active -> reset elapsed cache, latch flash_start_tick, detect >= 10s mid-flash stall
    movs r0, #0
    mvns r0, r0
    strh r0, [r4, #ST_FLASH_ELAPSED]
    ldr r0, [r4, #ST_FLASH_START]
    cmp r0, #0
    bne 53f
    ldr r0, [r4, #ST_TICK]
    str r0, [r4, #ST_FLASH_START]
53:
    ldrh r0, [r2, #6]           // cur
    ldrh r1, [r4, #ST_FLASH_CUR]
    cmp r0, r1
    beq 54f
    strh r0, [r4, #ST_FLASH_CUR]
    movs r1, #0
    strh r1, [r4, #ST_STALL_TICKS]
    b 55f
54:
    cmp r0, #0
    beq 55f
    ldrh r1, [r4, #ST_STALL_TICKS]
    movs r3, #150
    lsls r3, r3, #1             // 300 ticks = 10 s @ 30 Hz
    cmp r1, r3
    bhs 55f
    adds r1, r1, #1
    strh r1, [r4, #ST_STALL_TICKS]
55:
    movs r7, #4                 // banner 4: "Flashing page X/Y"
    ldrh r1, [r4, #ST_STALL_TICKS]
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
    ldrh r0, [r4, #ST_FLASH_ELAPSED]
    adds r1, r0, #1
    lsls r1, r1, #16
    bne 58f
    ldr r1, [r4, #ST_FLASH_START]
    movs r0, #0
    cmp r1, #0
    beq 57f
    ldr r0, [r4, #ST_TICK]
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
    strh r0, [r4, #ST_FLASH_ELAPSED]
    movs r1, #0
    str r1, [r4, #ST_FLASH_START]
    strh r1, [r4, #ST_STALL_TICKS]
58:
    movs r2, #100
    movs r1, #32
    tst r6, r1
    beq 59f
    ldrh r2, [r4, #ST_FLASH_ELAPSED]
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
    // Check mid-flash abort (FLAG_WAS_FLASH latched while state != 2 && state != 3)
    ldr r0, [r4, #ST_FLAGS]
    movs r1, #FLAG_WAS_FLASH
    tst r0, r1
    beq 66f
    movs r0, #0
    strh r0, [r4, #ST_STALL_TICKS]
    ldrh r0, [r2, #6]
    ldrh r1, [r2, #8]
    movs r7, #7                 // banner 7: "Aborted - reflash"
    b 50b

66:
    // Idle / connected / ready states: progress bar is HIDDEN (r5 = r6 = 0xFF);
    // the connection arrow carries live link state.
    movs r5, #0xFF
    movs r6, #0xFF
    ldrb r2, [r2, #5]           // hs (0/1)
    cmp r2, #0
    beq 62f
    movs r7, #2                 // banner 2: "Ready to flash"
    b 7f
62:
    ldrb r3, [r4, #ST_SERIAL_PULSE]
    cmp r3, #0
    beq 63f
    movs r7, #3                 // banner 3: "Link active"
    b 7f
63:
    cmp r7, #0
    bne 7f                      // banner 1: "USB connected" (r7 already 1)
    ldrb r0, [r4, #ST_UART_TIMER]
    cmp r0, #0
    beq 64f
    movs r7, #6                 // banner 6: "UART connected"
    b 7f
64:
    movs r7, #0                 // banner 0: "Plug in cable"

7:
    // 5b. Mirror live status, pages, total_pages, and elapsed_sec into KBHP v1.0 [0x30..0x3F]
    ldr r3, [r4, #ST_KBHP_ADDR]
    cmp r3, #0
    beq 700f
    adds r3, #KBHP_LIVE_BASE
    strb r7, [r3, #KBHP_LIVE_STATUS]
    ldr r0, [sp, #0]
    cmp r0, #0
    beq 701f
    strh r0, [r3, #KBHP_LIVE_PAGES]
    ldr r0, [sp, #4]
    strh r0, [r3, #KBHP_LIVE_TOTAL]
701:
    ldrh r0, [r4, #ST_FLASH_ELAPSED]
    strh r0, [r3, #KBHP_LIVE_ELAPSED]
700:

    // 6a. Page 4 progress bar (y=34..39, 100% inside Page 4): shown ONLY while flashing/complete/aborted
    lsls r2, r5, #8
    orrs r2, r2, r6
    lsls r0, r7, #24
    orrs r2, r2, r0
    ldr r3, [r4, #ST_LAST_CAPSULE]
    cmp r3, r2
    beq 78f
    str r2, [r4, #ST_LAST_CAPSULE]

    cmp r5, #0xFF
    bne 705f
    movs r0, #0xB4
    movs r1, #16                // HW col 12 + 4 = 16
    movs r2, #0                 // zero-fill -> completely hidden when idle
    movs r3, #104
    bl st7565_write_strip
    b 78f
705:
    adds r5, r5, #2
    adds r6, r6, #2
    movs r0, #0xB4
    movs r1, #16                // HW col 12 + 4 = 16
    bl st7565_begin_page
    movs r0, #0x78              // left rounded cap (y=35..38 -> P4 bits 3..6)
    bl spi_tx
    movs r2, #1
71:
    movs r0, #0x84              // empty track (top y=34 bit 2 | bottom y=39 bit 7)
    cmp r2, r5
    blo 74f
    cmp r2, r6
    bhs 74f
    movs r0, #0xB4              // 2 px filled interior (y=36..37 bits 4..5 | 0x84 borders)
    cmp r7, #7
    bne 74f
    movs r1, #2
    tst r2, r1                  // Aborted: 2 px dashed bar
    bne 74f
    movs r0, #0x84
74:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, #103
    blo 71b
    movs r0, #0x78              // right rounded cap (col 115)
    bl spi_tx
    bl st7565_end

78:
    // 6b. Page 5 ONLY status banner redraw on (banner << 12) | cur_page_lsb change
    lsls r0, r7, #12
    ldr r1, [sp, #0]
    lsls r1, r1, #20
    lsrs r1, r1, #20
    orrs r0, r0, r1
    ldrh r1, [r4, #ST_LAST_BANNER]
    cmp r0, r1
    beq 79f
    strh r0, [r4, #ST_LAST_BANNER]

    ldr r5, =DFU_SRAM_BANNERS   // 8 x 84 B (Page 5 only, cols 22..105)
    movs r0, #84
    muls r0, r0, r7
    adds r5, r5, r0

    // When flashing (banner == 4), stamp dynamic "<cur>/<tot>" at X=76 (+54 inside slice) on Page 5
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
    movs r2, #0
75:
    mov r0, sp
    ldrb r3, [r0, r2]           // raw 3x5 glyph byte (bits 2..6)
    lsls r3, r3, #1             // -> y=43..47 on Page 5 (bits 3..7, zero Page 6 spill)
    strb r3, [r5, r2]
    adds r2, r2, #1
    cmp r2, #28
    blo 75b
    subs r5, #54
    add sp, #32
    ldr r4, =DFU_SRAM_STATE

76:
    movs r0, #0xB5
    movs r1, #26                // HW col 22 + 4 = 26
    movs r2, r5
    movs r3, #84
    bl st7565_write_strip

79:
    // 6c. Page 1+2 radio LCD cutout ("0%".."100%" or "<N>s", Y=13..17, X=19..=34, HW col 23)
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
    ldrh r0, [r4, #ST_LAST_CUTOUT]
    cmp r5, r0
    beq 96f
    strh r5, [r4, #ST_LAST_CUTOUT]

    movs r0, #0
    movs r1, #0
    movs r2, #0
    movs r3, #0
    push {r0, r1, r2, r3}
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

    // Stream Page 1 (<< 3 + 0x0C bezel / 0x0F antenna cols 0..2) and Page 2 (>> 5 + 0x70 bezel)
    movs r4, #0
101:
    movs r0, #0xB1
    adds r0, r0, r4
    movs r1, #23                // HW col 19 + 4 = 23
    bl st7565_begin_page
    mov r5, sp
    movs r6, #0
102:
    ldrb r0, [r5, r6]
    cmp r4, #0
    bne 103f
    lsls r0, r0, #3
    movs r1, #0x0C
    cmp r6, #3
    bhs 105f
    movs r1, #0x0F
    b 105f
103:
    lsrs r0, r0, #5
    movs r1, #0x70
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
    ldr r4, =DFU_SRAM_STATE

96:
    // 6c-top. Page 1 link badge (`y=13, X=76..=115`, HW col 80):
    // r5 = 0 (blank), 1 (VID:PID when USB connected), >= 1200 (<baud>bps when UART connected)
    movs r5, #0
    ldr r0, [sp, #12]           // usb_active
    cmp r0, #0
    beq 808f
    movs r5, #1
    b 810f
808:
    ldrb r0, [r4, #ST_UART_TIMER]
    cmp r0, #0
    beq 810f
    ldr r0, =RCC_APB2ENR
    ldr r0, [r0]
    lsrs r0, r0, #15            // bit 14 = USART1EN
    bcc 810f
    ldr r0, =USART1_BASE
    ldr r1, [r0, #0x0C]         // CR1
    lsrs r1, r1, #14            // bit 13 = UE
    bcc 810f
    ldrh r1, [r0, #0x08]        // BRR
    cmp r1, #49
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
    ldr r0, [r4, #ST_LAST_TOP]
    cmp r5, r0
    beq 819f
    str r5, [r4, #ST_LAST_TOP]
    cmp r5, #1
    bne 811f
    movs r0, #0xB1
    movs r1, #80                // HW col 76 + 4 = 80
    ldr r2, =DFU_SRAM_VIDPID    // 40 B
    movs r3, #40
    bl st7565_write_strip
    b 819f
811:
    movs r0, #0
    movs r1, #0
    movs r2, #0
    push {r0, r1, r2}
    push {r0, r1, r2}
    push {r0, r1, r2}
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
    lsrs r5, r5, #1             // centered at X=96
    adds r6, r5, r1
    b 815f
814:
    movs r5, #40
    movs r6, #0
815:
    movs r0, #0xB1
    movs r1, #80                // HW col 76 + 4 = 80
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
    lsrs r0, r0, #1
813:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, #40
    blo 812b
    bl st7565_end
    add sp, #36
    ldr r4, =DFU_SRAM_STATE

819:
    // 6d. Connection arrow (P2/P3 cols 49..=67, HW col 53) represents connection state:
    //     r7 == 0 (Standby): slow ~1 Hz blink
    //     r7 == 3 (Link active): brisk ~3.7 Hz pulse
    //     r7 == 7 (Aborted): hidden (off)
    //     r7 == 1,2,4,5,6 (USB/UART connected, Ready, Flashing, Complete): solid steady
    ldr r5, [r4, #ST_FLAGS]
    movs r6, #1
    cmp r7, #7
    bne 970f
    movs r6, #0
    b 97f
970:
    cmp r7, #0
    bne 971f
    ldr r0, [r4, #ST_TICK]
    lsrs r0, r0, #4             // ~1 Hz waiting blink
    eors r6, r6, r0
    b 97f
971:
    cmp r7, #3
    bne 97f
    ldr r0, [r4, #ST_TICK]
    lsrs r0, r0, #2             // brisk activity pulse
    eors r6, r6, r0
97:
    movs r1, #1
    ands r6, r6, r1
    lsrs r0, r5, #4
    ands r0, r0, r1             // previous arrow_vis (FLAG_ARROW_VIS)
    cmp r0, r6
    beq 900f
    movs r0, #FLAG_ARROW_VIS
    eors r5, r5, r0
    str r5, [r4, #ST_FLAGS]

    movs r5, #0
    cmp r6, #0
    beq 98f
    ldr r5, =DFU_SRAM_ARROW     // P2 +0, P3 +19
98:
    movs r6, #0xB2
94:
    movs r0, r6
    movs r1, #53                // HW col 49 + 4 = 53
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
    ldr r1, [r4, #ST_TICK]      // Aborted / stalled -> fast Red blink (~3.7 Hz)
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
    ldr r1, [r4, #ST_TICK]      // Link active -> Green/Orange flutter
    lsrs r1, r1, #1
    movs r0, #1
    ands r0, r0, r1
    adds r0, r0, #2
    b 905f
903:
    cmp r7, #0
    bne 905f
    ldr r1, [r4, #ST_TICK]      // Standby -> subtle Green heartbeat blip every ~2.1 s
    movs r2, #0x3F
    ands r1, r1, r2
    movs r0, #0
    cmp r1, #3
    bhs 905f
    movs r0, #2
905:
    ldrb r1, [r4, #ST_LAST_LED]
    cmp r0, r1
    beq 99f
    strb r0, [r4, #ST_LAST_LED]
    bl bk4829_set_led

99:
    add sp, #16
    pop {r4, r5, r6, r7, pc}
    .ltorg
