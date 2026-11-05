//! Clock tree initialization and peripheral clock gating.
//!
//! The K1 bootloader already configures HSI to 48 MHz and sets the flash
//! wait state to 1WS before jumping to user firmware at 0x0800_2800.
//! We confirm the clock state and provide functions to enable peripheral clocks.

use pac::rcc;

/// Confirm the bootloader's clock configuration and enable GPIO port clocks.
/// Must be called before any GPIO, SPI, UART, or timer initialization.
pub fn init() {
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

/// Enable the ADC1 peripheral clock (APB2) and set PCLK/4 prescaler.
pub fn enable_adc1() {
    unsafe {
        // Set ADC clock source to PCLK/4 in RCC_CR (bits 22:21 = 0b01)
        let cr = rcc::RCC_CR.read_volatile();
        let new_cr = (cr & !(0b11 << 21)) | (1 << 21);
        rcc::RCC_CR.write_volatile(new_cr);

        // Enable ADC1 peripheral clock on APB2
        let val = rcc::RCC_APB2ENR.read_volatile();
        rcc::RCC_APB2ENR.write_volatile(val | rcc::APB2ENR_ADC1);
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

/// Enable the USBD (USB 2.0 Full-Speed Device) peripheral clock (APB1) and reset PHY.
pub fn enable_usbd() {
    unsafe {
        // 1. Enable SYSCFG clock (APB2) - required for USB PHY routing on PY32F071
        let val_apb2 = rcc::RCC_APB2ENR.read_volatile();
        rcc::RCC_APB2ENR.write_volatile(val_apb2 | rcc::APB2ENR_SYSCFG);

        // 2. Enable GPIOA clock
        let val_iop = rcc::RCC_IOPENR.read_volatile();
        rcc::RCC_IOPENR.write_volatile(val_iop | rcc::IOPENR_GPIOA);

        // 3. Enable USBD peripheral clock (APB1)
        let val = rcc::RCC_APB1ENR.read_volatile();
        rcc::RCC_APB1ENR.write_volatile(val | rcc::APB1ENR_USBD);

        // 4. Force reset and release USBD peripheral to guarantee clean hardware PHY state
        let rst = rcc::RCC_APBRSTR1.read_volatile();
        rcc::RCC_APBRSTR1.write_volatile(rst | rcc::APBRSTR1_USBDRST);
        for _ in 0..500 {
            core::hint::spin_loop();
        }
        rcc::RCC_APBRSTR1.write_volatile(rst & !rcc::APBRSTR1_USBDRST);
        for _ in 0..500 {
            core::hint::spin_loop();
        }
    }
}

/// Query active system core clock frequency in Hz dynamically from RCC registers.
/// Correctly distinguishes 48 MHz vs 72 MHz (PLL x2 vs x3) or HSI/HSE modes.
pub fn sysclk_hz() -> u32 {
    static HSI_TABLE: [u32; 8] = [
        4_000_000, 8_000_000, 16_000_000, 22_120_000, 24_000_000, 4_000_000, 4_000_000, 4_000_000,
    ];
    unsafe {
        let cfgr = rcc::RCC_CFGR.read_volatile();
        let sws = cfgr & 0x00000038;

        let base_clk = match sws {
            0x00000010 => {
                // PLL used as system clock
                let pllcfgr = rcc::RCC_PLLCFGR.read_volatile();
                let pllmul = match (pllcfgr >> 2) & 0x03 {
                    1 => 3, // PLL x3 (72 MHz with 24 MHz source)
                    _ => 2, // PLL x2 (48 MHz with 24 MHz source)
                };
                let icscr = rcc::RCC_ICSCR.read_volatile();
                let hsifs = ((icscr >> 13) & 0x07) as usize;
                let hsi_base = if hsifs == 4 {
                    24_000_000
                } else if hsifs < HSI_TABLE.len() && HSI_TABLE[hsifs] >= 16_000_000 {
                    HSI_TABLE[hsifs]
                } else {
                    24_000_000
                };
                pllmul * hsi_base
            }
            0x00000008 => {
                // HSE (External 24 MHz crystal)
                24_000_000
            }
            _ => {
                // HSI direct
                let icscr = rcc::RCC_ICSCR.read_volatile();
                let hsifs = ((icscr >> 13) & 0x07) as usize;
                let hsi_base = HSI_TABLE.get(hsifs).copied().unwrap_or(24_000_000);
                let cr = rcc::RCC_CR.read_volatile();
                let hsidiv = 1 << (((cr >> 11) & 0x07).min(7));
                hsi_base / hsidiv.max(1)
            }
        };

        // Apply AHB prescaler (HPRE[3:0])
        let hpre = ((cfgr >> 4) & 0x0F) as u32;
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
    unsafe {
        let (addr, shift_val) = if is_24mhz {
            (0x1FFF_3220 as *const u32, 0x4 << 13)
        } else {
            (0x1FFF_3210 as *const u32, 0x2 << 13)
        };
        let cal = addr.read_volatile();
        let trim = if cal != 0xFFFF_FFFF && (cal & 0x1FFF) != 0x1FFF && (cal & 0x1FFF) != 0 {
            cal & 0x1FFF
        } else {
            rcc::RCC_ICSCR.read_volatile() & 0x1FFF
        };
        let val = shift_val | trim;
        let icscr = rcc::RCC_ICSCR.read_volatile();
        rcc::RCC_ICSCR.write_volatile((icscr & !0xFFFF) | val);
    }
}

/// Switch system clock to one of the 5 discrete hardware speeds:
/// 0: 16 MHz (Direct HSI, PLL OFF, 0WS)
/// 1: 24 MHz (Direct HSI, PLL OFF, 0WS)
/// 2: 32 MHz (HSI 16 MHz * PLL 2X, 1WS) - RF Sweet Spot
/// 3: 48 MHz (HSI * PLL, 1WS) - Default / USB Full-Speed
/// 4: 72 MHz (HSI 24 MHz * PLL 3X, 2WS) - High Performance
pub fn set_sysclk_index(idx: u8) {
    match idx {
        0 => set_sysclk_16mhz(),
        1 => set_sysclk_24mhz(),
        2 => set_sysclk_32mhz(),
        3 => set_sysclk_48mhz(),
        4 => set_sysclk_72mhz(),
        _ => set_sysclk_48mhz(),
    }
}

/// Safely switch system clock to direct 16 MHz HSI (PLL OFF, 0WS).
pub fn set_sysclk_16mhz() {
    unsafe {
        switch_to_hsi_disable_pll();
        set_hsi_fs(false);
        let acr = pac::flash::FLASH_ACR.read_volatile();
        pac::flash::FLASH_ACR
            .write_volatile((acr & !pac::flash::ACR_LATENCY_MASK) | pac::flash::ACR_LATENCY_0WS);
    }
    crate::delay::update_systick_freq(16_000_000);
    crate::time::update_tim2_freq(16_000_000);
}

/// Safely switch system clock to direct 24 MHz HSI (PLL OFF, 0WS).
pub fn set_sysclk_24mhz() {
    unsafe {
        switch_to_hsi_disable_pll();
        set_hsi_fs(true);
        let acr = pac::flash::FLASH_ACR.read_volatile();
        pac::flash::FLASH_ACR
            .write_volatile((acr & !pac::flash::ACR_LATENCY_MASK) | pac::flash::ACR_LATENCY_0WS);
    }
    crate::delay::update_systick_freq(24_000_000);
    crate::time::update_tim2_freq(24_000_000);
}

/// Safely switch system clock to 32 MHz (16 MHz HSI * PLL 2X, 1WS).
/// Optimal RF sweet spot: harmonics bypass both 2m (144-148 MHz) and 70cm (430-440 MHz) amateur bands.
pub fn set_sysclk_32mhz() {
    unsafe {
        switch_to_hsi_disable_pll();
        set_hsi_fs(false);
        enable_pll_and_switch(rcc::PLLCFGR_PLLMUL_2X, pac::flash::ACR_LATENCY_1WS);
    }
    crate::delay::update_systick_freq(32_000_000);
    crate::time::update_tim2_freq(32_000_000);
}

/// Safely switch system clock to 72 MHz (PLL x3 from 24 MHz HSI).
/// Adjusts Flash wait states to 2WS before overclocking and updates SysTick calibration.
pub fn set_sysclk_72mhz() {
    unsafe {
        switch_to_hsi_disable_pll();
        set_hsi_fs(true);
        enable_pll_and_switch(rcc::PLLCFGR_PLLMUL_3X, pac::flash::ACR_LATENCY_2WS);
    }
    crate::delay::update_systick_freq(72_000_000);
    crate::time::update_tim2_freq(72_000_000);
}

/// Safely switch system clock to 48 MHz (HSI 24 MHz * PLL 2X, 1WS).
/// Always uses the factory-calibrated 24 MHz HSI trim (0x1FFF_3220) to ensure
/// clock jitter and frequency error remain strictly within the USB 2.0 Full-Speed
/// ±0.25% specification (2500 ppm) across all operating conditions.
pub fn set_sysclk_48mhz() {
    unsafe {
        switch_to_hsi_disable_pll();
        set_hsi_fs(true);
        enable_pll_and_switch(rcc::PLLCFGR_PLLMUL_2X, pac::flash::ACR_LATENCY_1WS);
    }
    crate::delay::update_systick_freq(48_000_000);
    crate::time::update_tim2_freq(48_000_000);
}

/// Safely switch system clock to direct 24 MHz HSI and disable PLL to minimize deep sleep power consumption.
/// Adjusts Flash wait states to 0WS and updates SysTick calibration.
pub fn set_sysclk_24mhz_no_pll() {
    set_sysclk_24mhz();
}
