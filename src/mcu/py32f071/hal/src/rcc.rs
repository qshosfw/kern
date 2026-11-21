//! Clock tree initialization and peripheral clock gating.
//!
//! The K1 bootloader already configures HSI to 48 MHz and sets the flash
//! wait state to 1WS before jumping to user firmware at 0x0800_2800.
//! We confirm the clock state and provide functions to enable peripheral clocks.

use pac::rcc;

static mut BOOT_ICSCR: u32 = 0;

/// Confirm the bootloader's clock configuration and enable GPIO port clocks.
/// Must be called before any GPIO, SPI, UART, or timer initialization.
pub fn init() {
    unsafe {
        if BOOT_ICSCR == 0 {
            BOOT_ICSCR = rcc::RCC_ICSCR.read_volatile();
        }
    }
    // Enable all GPIO port clocks (A, B, C, F) — required before any pin config
    enable_gpio_clocks();
}

/// Enable clocks for all GPIO ports used on the K1 board.
pub fn enable_gpio_clocks() {
    unsafe {
        let val = rcc::RCC_IOPENR.read_volatile();
        rcc::RCC_IOPENR.write_volatile(
            val | rcc::IOPENR_GPIOA | rcc::IOPENR_GPIOB | rcc::IOPENR_GPIOC | rcc::IOPENR_GPIOF,
        );
    }
}

/// Enable the SPI1 peripheral clock (APB2).
pub fn enable_spi1() {
    unsafe {
        let val = rcc::RCC_APB2ENR.read_volatile();
        rcc::RCC_APB2ENR.write_volatile(val | rcc::APB2ENR_SPI1);
    }
}

/// Enable the USART1 peripheral clock (APB2).
pub fn enable_usart1() {
    unsafe {
        let val = rcc::RCC_APB2ENR.read_volatile();
        rcc::RCC_APB2ENR.write_volatile(val | rcc::APB2ENR_USART1);
    }
}

/// Enable the USART3 peripheral clock (APB1).
pub fn enable_usart3() {
    unsafe {
        let val = rcc::RCC_APB1ENR.read_volatile();
        rcc::RCC_APB1ENR.write_volatile(val | rcc::APB1ENR_USART3);
    }
}

/// Enable the ADC1 peripheral clock (APB2) and set a clock-safe prescaler (`ADCCLK <= 12 MHz`).
pub fn enable_adc1() {
    resync_adc_prescaler(sysclk_hz());
    unsafe {
        // Enable ADC1 peripheral clock on APB2
        let val = rcc::RCC_APB2ENR.read_volatile();
        rcc::RCC_APB2ENR.write_volatile(val | rcc::APB2ENR_ADC1);
    }
}

/// Keep `ADCCLK <= 12 MHz` across all system clock speeds (`PCLK/4` for <=48 MHz, `PCLK/6` at 72 MHz).
pub fn resync_adc_prescaler(sysclk_hz: u32) {
    unsafe {
        let div_bits = if sysclk_hz > 48_000_000 { 0b10 } else { 0b01 };
        let cr = rcc::RCC_CR.read_volatile();
        rcc::RCC_CR.write_volatile((cr & !(0b11 << 21)) | (div_bits << 21));
    }
}

/// Enable the TIM2 peripheral clock (APB1).
pub fn enable_tim2() {
    unsafe {
        let val = rcc::RCC_APB1ENR.read_volatile();
        rcc::RCC_APB1ENR.write_volatile(val | rcc::APB1ENR_TIM2);
    }
}

/// Enable the DMA1 peripheral clock (AHB1).
pub fn enable_dma1() {
    unsafe {
        let val = rcc::RCC_AHBENR.read_volatile();
        rcc::RCC_AHBENR.write_volatile(val | rcc::AHBENR_DMA1);
    }
}

/// Enable the I2C1 peripheral clock (APB1).
pub fn enable_i2c1() {
    unsafe {
        let val = rcc::RCC_APB1ENR.read_volatile();
        rcc::RCC_APB1ENR.write_volatile(val | rcc::APB1ENR_I2C1);
    }
}

/// Enable the SYSCFG peripheral clock (APB2).
pub fn enable_syscfg() {
    unsafe {
        let val = rcc::RCC_APB2ENR.read_volatile();
        rcc::RCC_APB2ENR.write_volatile(val | rcc::APB2ENR_SYSCFG);
    }
}

/// Enable the CRC peripheral clock (AHB1).
pub fn enable_crc() {
    unsafe {
        let val = rcc::RCC_AHBENR.read_volatile();
        rcc::RCC_AHBENR.write_volatile(val | rcc::AHBENR_CRC);
    }
}

/// Enable the PWR peripheral clock (APB1).
pub fn enable_pwr() {
    unsafe {
        let val = rcc::RCC_APB1ENR.read_volatile();
        rcc::RCC_APB1ENR.write_volatile(val | rcc::APB1ENR_PWR);
    }
}

/// Ensure the internal 32.768 kHz LSI oscillator is enabled and ready.
#[inline(never)]
pub fn enable_lsi() {
    unsafe {
        let csr = rcc::RCC_CSR.read_volatile();
        if (csr & rcc::CSR_LSION) == 0 {
            rcc::RCC_CSR.write_volatile(csr | rcc::CSR_LSION);
        }
        let mut timeout = 50_000u32;
        while (rcc::RCC_CSR.read_volatile() & rcc::CSR_LSIRDY) == 0 && timeout > 0 {
            timeout -= 1;
        }
    }
}

/// Enable LPTIM1 clock gated on LSI (~32.768 kHz).
pub fn enable_lptim1_lsi() {
    enable_lsi();
    unsafe {
        let ccipr = rcc::RCC_CCIPR.read_volatile();
        rcc::RCC_CCIPR
            .write_volatile((ccipr & !rcc::CCIPR_LPTIM1SEL_MASK) | rcc::CCIPR_LPTIM1SEL_LSI);
        let apb1 = rcc::RCC_APB1ENR.read_volatile();
        rcc::RCC_APB1ENR.write_volatile(apb1 | rcc::APB1ENR_LPTIM);
    }
}

/// Enable RTC APB clock, unlock backup domain, and select LSI as RTC clock source.
pub fn enable_rtc_lsi() {
    enable_pwr();
    crate::pwr::set_backup_write_enabled(true);
    enable_lsi();
    unsafe {
        let apb1 = rcc::RCC_APB1ENR.read_volatile();
        rcc::RCC_APB1ENR.write_volatile(apb1 | rcc::APB1ENR_RTCAPB);
        let bdcr = rcc::RCC_BDCR.read_volatile();
        if (bdcr & rcc::BDCR_RTCEN) == 0 || (bdcr & rcc::BDCR_RTCSEL_MASK) != rcc::BDCR_RTCSEL_LSI {
            rcc::RCC_BDCR.write_volatile(
                (bdcr & !rcc::BDCR_RTCSEL_MASK) | rcc::BDCR_RTCSEL_LSI | rcc::BDCR_RTCEN,
            );
        }
    }
}

/// Enable or disable hardware CTC auto-trim of 16 MHz HSI (PLL x3 = 48 MHz) locked to `USBD_SOF`.
#[inline(never)]
pub fn set_ctc_usb_sof(enable: bool) {
    unsafe {
        let rcc_b = rcc::RCC_BASE as *mut u32;
        let apb1 = rcc_b.byte_add(0x3C);
        let ctc = rcc::CTC_BASE as *mut u32;
        if enable {
            apb1.write_volatile(apb1.read_volatile() | rcc::APB1ENR_CTC);
            let rstr1 = rcc_b.byte_add(0x2C);
            let r = rstr1.read_volatile();
            rstr1.write_volatile(r | rcc::APBRSTR1_CTCRST);
            rstr1.write_volatile(r & !rcc::APBRSTR1_CTCRST);
            ctc.byte_add(0x04).write_volatile(rcc::CTC_CTL1_USB_SOF_48M);
            ctc.write_volatile(ctc.read_volatile() | rcc::CTC_CTL0_AUTOTRIM | rcc::CTC_CTL0_CNTEN);
        } else if (apb1.read_volatile() & rcc::APB1ENR_CTC) != 0 {
            ctc.write_volatile(
                ctc.read_volatile() & !(rcc::CTC_CTL0_AUTOTRIM | rcc::CTC_CTL0_CNTEN),
            );
        }
    }
}

/// Enable or truly detach the PY32F071 USBD Full-Speed PHY + CTC SOF clock lock.
#[inline(never)]
pub fn set_usbd_enabled(enable: bool) {
    unsafe {
        let rcc_b = rcc::RCC_BASE as *mut u32;
        let rstr1 = rcc_b.byte_add(0x2C);
        let apb1 = rcc_b.byte_add(0x3C);
        if enable {
            enable_syscfg();
            let iop = rcc_b.byte_add(0x34);
            iop.write_volatile(iop.read_volatile() | rcc::IOPENR_GPIOA);
            apb1.write_volatile(apb1.read_volatile() | rcc::APB1ENR_USBD);
            let rst = rstr1.read_volatile();
            rstr1.write_volatile(rst | rcc::APBRSTR1_USBDRST);
            cortex_m::asm::delay(500);
            rstr1.write_volatile(rst & !rcc::APBRSTR1_USBDRST);
            cortex_m::asm::delay(500);
            if sysclk_hz() == 48_000_000 {
                set_ctc_usb_sof(true);
            }
        } else {
            set_ctc_usb_sof(false);
            let rst = rstr1.read_volatile();
            rstr1.write_volatile(rst | rcc::APBRSTR1_USBDRST);
            apb1.write_volatile(apb1.read_volatile() & !rcc::APB1ENR_USBD);
            rstr1.write_volatile(rst & !rcc::APBRSTR1_USBDRST);
            crate::gpio::set_pins_analog(
                pac::gpio::GPIOA_BASE,
                pac::gpio::PIN_11 | pac::gpio::PIN_12,
            );
        }
    }
}

/// Enable the USBD (USB 2.0 Full-Speed Device) peripheral clock (APB1), reset PHY, and arm CTC.
#[inline(always)]
pub fn enable_usbd() {
    set_usbd_enabled(true);
}

/// True PY32F071 hardware USB detach: stops CTC, resets/gates USBD, and parks `PA11`/`PA12` Hi-Z.
#[inline(always)]
pub fn disable_usbd() {
    set_usbd_enabled(false);
}

/// Query active system core clock frequency in Hz dynamically from RCC registers.
/// Correctly distinguishes 48 MHz vs 72 MHz (PLL x2 vs x3) or HSI/HSE modes.
pub fn sysclk_hz() -> u32 {
    static HSI_KHZ: [u16; 5] = [4_000, 8_000, 16_000, 22_120, 24_000];
    unsafe {
        let cfgr = rcc::RCC_CFGR.read_volatile();
        let sws = cfgr & 0x00000038;
        let hsifs = ((rcc::RCC_ICSCR.read_volatile() >> 13) & 0x07) as usize;
        let hsi_base = (*HSI_KHZ.get(hsifs).unwrap_or(&4_000) as u32) * 1_000;

        let base_clk = match sws {
            0x00000010 => {
                // PLL used as system clock
                let pllcfgr = rcc::RCC_PLLCFGR.read_volatile();
                let pllmul = if ((pllcfgr >> 2) & 0x03) == 1 { 3 } else { 2 };
                let src = if hsi_base >= 16_000_000 {
                    hsi_base
                } else {
                    24_000_000
                };
                pllmul * src
            }
            0x00000008 => 24_000_000,
            _ => {
                // HSI direct
                let hsidiv_exp = (rcc::RCC_CR.read_volatile() >> 11) & 0x07;
                hsi_base >> hsidiv_exp
            }
        };

        // Apply AHB prescaler (HPRE[3:0])
        let hpre = (cfgr >> 4) & 0x0F;
        let shift = if hpre >= 8 { (hpre - 7).min(9) } else { 0 };
        base_clk >> shift
    }
}

#[inline(never)]
unsafe fn switch_to_hsi_disable_pll() {
    unsafe {
        let cfgr = rcc::RCC_CFGR.read_volatile();
        rcc::RCC_CFGR.write_volatile((cfgr & !rcc::CFGR_SW_MASK) | rcc::CFGR_SW_HSI);
        while (rcc::RCC_CFGR.read_volatile() & rcc::CFGR_SWS_MASK) != rcc::CFGR_SWS_HSI {}

        let cr = rcc::RCC_CR.read_volatile();
        rcc::RCC_CR.write_volatile(cr & !rcc::CR_PLLON);
        while (rcc::RCC_CR.read_volatile() & rcc::CR_PLLRDY) != 0 {}
    }
}

#[inline(never)]
unsafe fn enable_pll_and_switch(pll_mul: u32, ws: u32) {
    unsafe {
        let acr = pac::flash::FLASH_ACR.read_volatile();
        pac::flash::FLASH_ACR.write_volatile((acr & !pac::flash::ACR_LATENCY_MASK) | ws);

        let pllcfgr = rcc::RCC_PLLCFGR.read_volatile();
        let new_pllcfgr = (pllcfgr & !(rcc::PLLCFGR_PLLSRC_MASK | rcc::PLLCFGR_PLLMUL_MASK))
            | rcc::PLLCFGR_PLLSRC_HSI
            | pll_mul;
        rcc::RCC_PLLCFGR.write_volatile(new_pllcfgr);

        rcc::RCC_CR.write_volatile(rcc::RCC_CR.read_volatile() | rcc::CR_PLLON);
        while (rcc::RCC_CR.read_volatile() & rcc::CR_PLLRDY) == 0 {}

        let cfgr = rcc::RCC_CFGR.read_volatile();
        rcc::RCC_CFGR.write_volatile((cfgr & !rcc::CFGR_SW_MASK) | rcc::CFGR_SW_PLL);
        while (rcc::RCC_CFGR.read_volatile() & rcc::CFGR_SWS_MASK) != rcc::CFGR_SWS_PLL {}
    }
}

#[inline(never)]
unsafe fn set_hsi_fs(is_24mhz: bool) {
    let target_fs = if is_24mhz { 4 } else { 2 };
    unsafe {
        let icscr = rcc::RCC_ICSCR.read_volatile();
        if ((icscr >> 13) & 0x07) == target_fs {
            return;
        }

        let boot_fs = (BOOT_ICSCR >> 13) & 0x07;
        let val = if target_fs == boot_fs && BOOT_ICSCR != 0 {
            BOOT_ICSCR & 0xFFFF
        } else {
            let addr = if is_24mhz {
                0x1FFF_3220 as *const u32
            } else {
                0x1FFF_3210 as *const u32
            };
            let cal = addr.read_volatile();
            let trim = if cal != 0xFFFF_FFFF && (cal & 0x1FFF) != 0x1FFF && (cal & 0x1FFF) != 0 {
                cal & 0x1FFF
            } else {
                icscr & 0x1FFF
            };
            (target_fs << 13) | trim
        };

        rcc::RCC_ICSCR.write_volatile((icscr & !0xFFFF) | val);

        while (rcc::RCC_CR.read_volatile() & rcc::CR_HSIRDY) == 0 {
            core::hint::spin_loop();
        }
        crate::flash::InternalFlash::sync_timing_for_hsi(is_24mhz);
    }
}

/// Switch system clock to one of the 5 discrete hardware speeds:
/// 0: 16 MHz (Direct HSI, PLL OFF, 0WS)
/// 1: 24 MHz (Direct HSI, PLL OFF, 0WS)
/// 2: 32 MHz (HSI 16 MHz * PLL 2X, 1WS) - RF Sweet Spot
/// 3: 48 MHz (HSI 16 MHz * PLL 3X, 1WS) - Default / USB Full-Speed (CTC SOF-lockable)
/// 4: 72 MHz (HSI 24 MHz * PLL 3X, 2WS) - High Performance
#[inline(never)]
pub fn set_sysclk_index(idx: u8) {
    let idx = if idx > 4 { 3 } else { idx };
    let hsi_24 = idx == 1 || idx == 4;
    let wait_states = if idx < 2 {
        pac::flash::ACR_LATENCY_0WS
    } else if idx == 4 {
        pac::flash::ACR_LATENCY_2WS
    } else {
        pac::flash::ACR_LATENCY_1WS
    };
    cortex_m::interrupt::free(|_| {
        set_ctc_usb_sof(false);
        unsafe {
            switch_to_hsi_disable_pll();
            set_hsi_fs(hsi_24);
            if idx >= 2 {
                let mul = if idx >= 3 {
                    rcc::PLLCFGR_PLLMUL_3X
                } else {
                    rcc::PLLCFGR_PLLMUL_2X
                };
                enable_pll_and_switch(mul, wait_states);
            } else {
                let acr = pac::flash::FLASH_ACR.read_volatile();
                pac::flash::FLASH_ACR
                    .write_volatile((acr & !pac::flash::ACR_LATENCY_MASK) | wait_states);
            }
            if idx == 3 && (rcc::RCC_APB1ENR.read_volatile() & rcc::APB1ENR_USBD) != 0 {
                set_ctc_usb_sof(true);
            }
        }
    });
    let hz = sysclk_hz();
    resync_adc_prescaler(hz);
    crate::delay::update_systick_freq(hz);
    crate::time::update_tim2_freq(hz);
    crate::spi::resync_clock(hz);
    crate::uart::resync_clock(hz);
    #[cfg(feature = "target_k10pro")]
    crate::uart::resync_usart3_clock(hz);
}

#[inline]
pub fn set_sysclk_16mhz() {
    set_sysclk_index(0);
}

#[inline]
pub fn set_sysclk_24mhz() {
    set_sysclk_index(1);
}

#[inline]
pub fn set_sysclk_32mhz() {
    set_sysclk_index(2);
}

#[inline]
pub fn set_sysclk_48mhz() {
    set_sysclk_index(3);
}

#[inline]
pub fn set_sysclk_72mhz() {
    set_sysclk_index(4);
}

#[inline]
pub fn set_sysclk_24mhz_no_pll() {
    set_sysclk_index(1);
}
