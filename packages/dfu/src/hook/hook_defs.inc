/*
 * KERN DFU Hook Symbolic Definitions (synced 1:1 with src/dfu_abi.h)
 *
 * Every ST_* offset and KBHP_LIVE_* offset below is compile-time asserted via
 * `_Static_assert(offsetof(...))` in `src/dfu_abi.h`.
 */

/* ── Upper-SRAM Arena ───────────────────────────────────────────────────────── */
.equ DFU_SRAM_BANNERS,  0x20003000  /* 8 x 80 B = 640 B (Page 5 subtitles) */
.equ DFU_SRAM_TITLES,   0x20003280  /* 5 x 64 B = 320 B (Page 4 idle titles) */
.equ DFU_SRAM_ARROW,    0x20003540  /* 2 x 19 B = 38 B (Pages 2-3) */
.equ DFU_SRAM_VIDPID,   0x20003580  /* 40 B (Page 1) */
.equ KBHP_SRAM_ADDR,    0x200035C0  /* 64 B (KBHP v1.0 Handoff Envelope) */
.equ DFU_SRAM_STATE,    0x200036C0  /* 64 B (DfuHookState) */

/* ── Stock Quansheng PY32F071 BL State Addresses (only read when STOCK_K1=1) ── */
.equ STOCK_BL_STATE,    0x2000001C  /* [+4]=state, [+5]=hs, [+6]=cur, [+8]=tot */
.equ STOCK_USB_RING,    0x200009A0
.equ STOCK_UART_RING,   0x200019E0

/* ── PY32F071xB Peripheral & Flash Addresses ────────────────────────────────── */
.equ APP_FLASH_BASE,    0x08002800
.equ GPIOB_BASE,        0x50000400
.equ GPIOF_BASE,        0x50001400
.equ FLASH_REGS_BASE,   0x40022010  /* +0=SR, +4=CR */
.equ RCC_APB1ENR,       0x4002103C  /* +0=APB1ENR, +4=APB2ENR, +0x20=BDCR, +0x24=CSR */
.equ RCC_APB2ENR,       0x40021040
.equ PWR_CR1,           0x40007000
.equ RTC_BASE,          0x40002800  /* +0x04=CRL, +0x20=ALRH, +0x24=ALRL */
.equ DMA1_CH2_CNDTR,    0x40020020
.equ USBD_BASE,         0x40005C00
.equ SPI1_BASE,         0x40013000
.equ USART1_BASE,       0x40013800
.equ SCS_SYSTICK_CSR,   0xE000E010
.equ SCB_AIRCR,         0xE000ED0C
.equ AIRCR_SYSRESETREQ, 0x05FA0004

/* ── Backup-Domain RTC_ALR Tag for Zero-FW-Cost MENU Tools Wakeup ───────────── */
.equ KBHP_RTC_TOOLS_HI, 0xA500      /* IDENT=0xA5, strikes=0 */
.equ KBHP_RTC_TOOLS_LO, 0x4900      /* flags=CLEAN(0x01)|BL_HOLD(0x08)|TOOLS(0x40) */

/* ── DfuHookState Offsets (base = DFU_SRAM_STATE, 64 B) ─────────────────────── */
.equ ST_FLAGS,          0
.equ ST_KEY_HOLD,       4
.equ ST_TICK,           8
.equ ST_LAST_CAPSULE,   12
.equ ST_USB_FRAME,      16
.equ ST_USB_MISS,       18
.equ ST_UART_TIMER,     19
.equ ST_KBHP_ADDR,      20
.equ ST_FLASH_START,    24
.equ ST_LAST_BANNER,    28
.equ ST_LAST_LED,       30
.equ ST_SERIAL_PULSE,   31
.equ ST_SERIAL_HASH,    32
.equ ST_FLASH_ELAPSED,  36
.equ ST_LAST_CUTOUT,    38
.equ ST_LAST_TOP,       40
.equ ST_FLASH_CUR,      44
.equ ST_STALL_TICKS,    46
.equ ST_PREV_CNDTR,     48

/* ── DfuHookState Flag Bitmasks ─────────────────────────────────────────────── */
.equ FLAG_STOCK_K1,     0x01
.equ FLAG_KEY_SEEN_HI,  0x02
.equ FLAG_USB_ACTIVE,   0x04
.equ FLAG_WAS_FLASH,    0x08
.equ FLAG_ARROW_VIS,    0x10
.equ FLAG_BL_OFF,       0x20
.equ FLAG_NO_TAP,       0x40
.equ FLAG_KBHP_VALID,   0x80

/* ── KbhpEnvelope Live Telemetry Block (base = KBHP_SRAM_ADDR + 0x30, 16 B) ─── */
.equ KBHP_LIVE_BASE,    0x30
.equ KBHP_LIVE_STATUS,  0x00
.equ KBHP_LIVE_ERR,     0x01
.equ KBHP_LIVE_FLAGS,   0x03
.equ KBHP_LIVE_PAGES,   0x04
.equ KBHP_LIVE_TOTAL,   0x06
.equ KBHP_LIVE_ELAPSED, 0x08
