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
        let rst_ptr = (rcc::RCC_BASE + 0x24) as *mut u32; // RCC_APBRSTR1
        let rst = rst_ptr.read_volatile();
        rst_ptr.write_volatile(rst | (1 << 23)); // RCC_APBRSTR1_USBDRST
        for _ in 0..200 {
            core::hint::spin_loop();
        }
        rst_ptr.write_volatile(rst & !(1 << 23));
        for _ in 0..200 {
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
                let hsi_base = HSI_TABLE.get(hsifs).copied().unwrap_or(24_000_000);
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

/// Safely switch system clock to 72 MHz (PLL x3 from 24 MHz HSI).
/// Adjusts Flash wait states to 2WS before overclocking and updates SysTick calibration.
pub fn set_sysclk_72mhz() {
    unsafe {
        // 1. If increasing clock frequency, increase flash latency first (2WS for <= 72 MHz)
        let acr = pac::flash::FLASH_ACR.read_volatile();
        pac::flash::FLASH_ACR
            .write_volatile((acr & !pac::flash::ACR_LATENCY_MASK) | pac::flash::ACR_LATENCY_2WS);

        // 2. Temporarily switch SYSCLK source to direct HSI (24 MHz)
        let cfgr = rcc::RCC_CFGR.read_volatile();
        rcc::RCC_CFGR.write_volatile((cfgr & !rcc::CFGR_SW_MASK) | rcc::CFGR_SW_HSI);
        while (rcc::RCC_CFGR.read_volatile() & rcc::CFGR_SWS_MASK) != rcc::CFGR_SWS_HSI {}

        // 3. Disable PLL to allow re-programming multiplier
        let cr = rcc::RCC_CR.read_volatile();
        rcc::RCC_CR.write_volatile(cr & !rcc::CR_PLLON);
        while (rcc::RCC_CR.read_volatile() & rcc::CR_PLLRDY) != 0 {}

        // 4. Configure PLL for x3 (72 MHz) with HSI source
        let pllcfgr = rcc::RCC_PLLCFGR.read_volatile();
        let new_pllcfgr = (pllcfgr & !(rcc::PLLCFGR_PLLSRC_MASK | rcc::PLLCFGR_PLLMUL_MASK))
            | rcc::PLLCFGR_PLLSRC_HSI
            | rcc::PLLCFGR_PLLMUL_3X;
        rcc::RCC_PLLCFGR.write_volatile(new_pllcfgr);

        // 5. Enable PLL and wait for lock
        rcc::RCC_CR.write_volatile(rcc::RCC_CR.read_volatile() | rcc::CR_PLLON);
        while (rcc::RCC_CR.read_volatile() & rcc::CR_PLLRDY) == 0 {}

        // 6. Switch SYSCLK source back to PLL
        let cfgr = rcc::RCC_CFGR.read_volatile();
        rcc::RCC_CFGR.write_volatile((cfgr & !rcc::CFGR_SW_MASK) | rcc::CFGR_SW_PLL);
        while (rcc::RCC_CFGR.read_volatile() & rcc::CFGR_SWS_MASK) != rcc::CFGR_SWS_PLL {}
    }

    // 7. Update SysTick reload value and microsecond scaling to maintain millisecond precision
    crate::delay::update_systick_freq(72_000_000);
}

/// Safely switch system clock to 48 MHz (PLL x2 from 24 MHz HSI).
/// Adjusts Flash wait states to 1WS and updates SysTick calibration.
pub fn set_sysclk_48mhz() {
    unsafe {
        // 1. Temporarily switch SYSCLK source to direct HSI (24 MHz)
        let cfgr = rcc::RCC_CFGR.read_volatile();
        rcc::RCC_CFGR.write_volatile((cfgr & !rcc::CFGR_SW_MASK) | rcc::CFGR_SW_HSI);
        while (rcc::RCC_CFGR.read_volatile() & rcc::CFGR_SWS_MASK) != rcc::CFGR_SWS_HSI {}

        // 2. Disable PLL to allow re-programming multiplier
        let cr = rcc::RCC_CR.read_volatile();
        rcc::RCC_CR.write_volatile(cr & !rcc::CR_PLLON);
        while (rcc::RCC_CR.read_volatile() & rcc::CR_PLLRDY) != 0 {}

        // 3. Configure PLL for x2 (48 MHz) with HSI source
        let pllcfgr = rcc::RCC_PLLCFGR.read_volatile();
        let new_pllcfgr = (pllcfgr & !(rcc::PLLCFGR_PLLSRC_MASK | rcc::PLLCFGR_PLLMUL_MASK))
            | rcc::PLLCFGR_PLLSRC_HSI
            | rcc::PLLCFGR_PLLMUL_2X;
        rcc::RCC_PLLCFGR.write_volatile(new_pllcfgr);

        // 4. Enable PLL and wait for lock
        rcc::RCC_CR.write_volatile(rcc::RCC_CR.read_volatile() | rcc::CR_PLLON);
        while (rcc::RCC_CR.read_volatile() & rcc::CR_PLLRDY) == 0 {}

        // 5. Switch SYSCLK source back to PLL
        let cfgr = rcc::RCC_CFGR.read_volatile();
        rcc::RCC_CFGR.write_volatile((cfgr & !rcc::CFGR_SW_MASK) | rcc::CFGR_SW_PLL);
        while (rcc::RCC_CFGR.read_volatile() & rcc::CFGR_SWS_MASK) != rcc::CFGR_SWS_PLL {}

        // 6. After reducing clock frequency, set flash latency to 1WS (<= 48 MHz)
        let acr = pac::flash::FLASH_ACR.read_volatile();
        pac::flash::FLASH_ACR
            .write_volatile((acr & !pac::flash::ACR_LATENCY_MASK) | pac::flash::ACR_LATENCY_1WS);
    }

    // 7. Update SysTick reload value and microsecond scaling to maintain millisecond precision
    crate::delay::update_systick_freq(48_000_000);
}
