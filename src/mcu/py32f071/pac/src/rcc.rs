//! Reset and Clock Control (RCC) registers for PY32F071.
//!
//! The bootloader configures HSI to 48 MHz before jumping to firmware.
//! We primarily use RCC to enable peripheral clocks.

use crate::{AHB1_BASE, APB1_BASE};

pub const RCC_BASE: u32 = AHB1_BASE + 0x1000; // 0x4002_1000
pub const CTC_BASE: u32 = APB1_BASE + 0x6C00; // 0x4000_6C00

/// CTC Control Register 0 (hardware HSI auto-trim against USBD_SOF)
pub const CTC_CTL0: *mut u32 = CTC_BASE as *mut u32;
/// CTC Control Register 1 (reference select, prescaler, limit, reload)
pub const CTC_CTL1: *mut u32 = (CTC_BASE + 0x04) as *mut u32;
pub const CTC_CTL0_CNTEN: u32 = 1 << 5;
pub const CTC_CTL0_AUTOTRIM: u32 = 1 << 6;
/// USBD_SOF (rising, /1), CKLIM = 0x22, RLVALUE = 0xBB7F (48 MHz / 1 kHz - 1)
pub const CTC_CTL1_USB_SOF_48M: u32 = (0x2 << 28) | (0x22 << 16) | 0xBB7F;

// ── Register Addresses ──────────────────────────────────────────────────────

/// Clock control register
pub const RCC_CR: *mut u32 = RCC_BASE as *mut u32;

/// Internal clock sources calibration register
pub const RCC_ICSCR: *mut u32 = (RCC_BASE + 0x04) as *mut u32;

/// Clock configuration register
pub const RCC_CFGR: *mut u32 = (RCC_BASE + 0x08) as *mut u32;

/// PLL configuration register
pub const RCC_PLLCFGR: *mut u32 = (RCC_BASE + 0x0C) as *mut u32;

/// APB peripheral reset register 1
pub const RCC_APBRSTR1: *mut u32 = (RCC_BASE + 0x2C) as *mut u32;

/// AHB peripheral clock enable register (DMA1, FLASH, CRC)
pub const RCC_AHBENR: *mut u32 = (RCC_BASE + 0x38) as *mut u32;

/// APB1 peripheral clock enable register
pub const RCC_APB1ENR: *mut u32 = (RCC_BASE + 0x3C) as *mut u32;

/// APB2 peripheral clock enable register (SPI1, USART1, ADC1, TIM1)
pub const RCC_APB2ENR: *mut u32 = (RCC_BASE + 0x40) as *mut u32;

/// Peripherals independent clock configuration register
pub const RCC_CCIPR: *mut u32 = (RCC_BASE + 0x54) as *mut u32;

/// Backup / RTC domain control register
pub const RCC_BDCR: *mut u32 = (RCC_BASE + 0x5C) as *mut u32;

/// Control / status register (LSI & Reset cause flags)
pub const RCC_CSR: *mut u32 = (RCC_BASE + 0x60) as *mut u32;

/// IO port clock enable register (GPIOA, GPIOB, GPIOC, GPIOF)
pub const RCC_IOPENR: *mut u32 = (RCC_BASE + 0x34) as *mut u32;

// ── APBRSTR1 Bit Definitions ────────────────────────────────────────────────

/// USBD peripheral reset enable
pub const APBRSTR1_USBDRST: u32 = 1 << 23;
/// CTC (Clock Trim Controller) peripheral reset enable
pub const APBRSTR1_CTCRST: u32 = 1 << 27;

// ── AHBENR Bit Definitions ─────────────────────────────────────────────────

pub const AHBENR_DMA1: u32 = 1 << 0;
pub const AHBENR_FLASH: u32 = 1 << 8;
pub const AHBENR_CRC: u32 = 1 << 12;
pub const AHBENR_DIV: u32 = 1 << 24;

// ── IOPENR Bit Definitions ─────────────────────────────────────────────────

pub const IOPENR_GPIOA: u32 = 1 << 0;
pub const IOPENR_GPIOB: u32 = 1 << 1;
pub const IOPENR_GPIOC: u32 = 1 << 2;
pub const IOPENR_GPIOF: u32 = 1 << 5;

// ── APB1ENR Bit Definitions ─────────────────────────────────────────────────

/// TIM2 clock enable
pub const APB1ENR_TIM2: u32 = 1 << 0;
/// TIM3 clock enable
pub const APB1ENR_TIM3: u32 = 1 << 1;
/// TIM6 clock enable
pub const APB1ENR_TIM6: u32 = 1 << 4;
/// RTC APB clock enable
pub const APB1ENR_RTCAPB: u32 = 1 << 10;
/// WWDG clock enable
pub const APB1ENR_WWDG: u32 = 1 << 11;
/// SPI2 clock enable
pub const APB1ENR_SPI2: u32 = 1 << 14;
/// USART2 clock enable
pub const APB1ENR_USART2: u32 = 1 << 17;
/// USART3 clock enable
pub const APB1ENR_USART3: u32 = 1 << 18;
/// USART4 clock enable
pub const APB1ENR_USART4: u32 = 1 << 19;
/// I2C1 clock enable
pub const APB1ENR_I2C1: u32 = 1 << 21;
/// USBD (USB 2.0 Full-Speed) clock enable
pub const APB1ENR_USBD: u32 = 1 << 23;
/// CTC (Clock Trim Controller) clock enable
pub const APB1ENR_CTC: u32 = 1 << 27;
/// PWR interface clock enable
pub const APB1ENR_PWR: u32 = 1 << 28;
/// DAC1 clock enable
pub const APB1ENR_DAC: u32 = 1 << 29;
/// LPTIM1 clock enable
pub const APB1ENR_LPTIM: u32 = 1 << 31;

// ── CCIPR Bit Definitions ──────────────────────────────────────────────────

pub const CCIPR_LPTIM1SEL_SHIFT: u32 = 18;
pub const CCIPR_LPTIM1SEL_MASK: u32 = 0b11 << 18;
pub const CCIPR_LPTIM1SEL_PCLK: u32 = 0b00 << 18;
pub const CCIPR_LPTIM1SEL_LSI: u32 = 0b01 << 18;
pub const CCIPR_LPTIM1SEL_LSE: u32 = 0b11 << 18;

// ── BDCR Bit Definitions ───────────────────────────────────────────────────

pub const BDCR_LSEON: u32 = 1 << 0;
pub const BDCR_LSERDY: u32 = 1 << 1;
pub const BDCR_RTCSEL_MASK: u32 = 0b11 << 8;
pub const BDCR_RTCSEL_LSE: u32 = 0b01 << 8;
pub const BDCR_RTCSEL_LSI: u32 = 0b10 << 8;
pub const BDCR_RTCEN: u32 = 1 << 15;
pub const BDCR_BDRST: u32 = 1 << 16;

// ── CSR Bit Definitions ────────────────────────────────────────────────────

pub const CSR_LSION: u32 = 1 << 0;
pub const CSR_LSIRDY: u32 = 1 << 1;
pub const CSR_RMVF: u32 = 1 << 23;
pub const CSR_OBLRSTF: u32 = 1 << 25;
pub const CSR_PINRSTF: u32 = 1 << 26;
pub const CSR_PWRRSTF: u32 = 1 << 27;
pub const CSR_SFTRSTF: u32 = 1 << 28;
pub const CSR_IWDGRSTF: u32 = 1 << 29;
pub const CSR_WWDGRSTF: u32 = 1 << 30;

// ── APB2ENR Bit Definitions ─────────────────────────────────────────────────

/// SYSCFG clock enable
pub const APB2ENR_SYSCFG: u32 = 1 << 0;
/// SPI1 clock enable
pub const APB2ENR_SPI1: u32 = 1 << 12;
/// USART1 clock enable
pub const APB2ENR_USART1: u32 = 1 << 14;
/// ADC1 clock enable
pub const APB2ENR_ADC1: u32 = 1 << 9;

// ── CR Bit Definitions ─────────────────────────────────────────────────────

pub const CR_HSION: u32 = 1 << 8;
pub const CR_HSIRDY: u32 = 1 << 10;
pub const CR_PLLON: u32 = 1 << 24;
pub const CR_PLLRDY: u32 = 1 << 25;

// ── CFGR Bit Definitions ───────────────────────────────────────────────────

pub const CFGR_SW_MASK: u32 = 0x07;
pub const CFGR_SW_HSI: u32 = 0x00;
pub const CFGR_SW_HSE: u32 = 0x01;
pub const CFGR_SW_PLL: u32 = 0x02;

pub const CFGR_SWS_MASK: u32 = 0x38;
pub const CFGR_SWS_HSI: u32 = 0x00;
pub const CFGR_SWS_HSE: u32 = 0x08;
pub const CFGR_SWS_PLL: u32 = 0x10;

// ── PLLCFGR Bit Definitions ─────────────────────────────────────────────────

pub const PLLCFGR_PLLSRC_MASK: u32 = 0x03;
pub const PLLCFGR_PLLSRC_HSI: u32 = 0x02;
pub const PLLCFGR_PLLMUL_MASK: u32 = 0x0C;
pub const PLLCFGR_PLLMUL_2X: u32 = 0x00 << 2; // 48 MHz with 24 MHz HSI
pub const PLLCFGR_PLLMUL_3X: u32 = 0x01 << 2; // 72 MHz with 24 MHz HSI
