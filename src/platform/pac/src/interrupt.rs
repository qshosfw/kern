//! PY32F071xB NVIC interrupt definitions and Cortex-M0+ vector table.

/// Number of NVIC priority bits implemented on PY32F071xB.
pub const NVIC_PRIO_BITS: u8 = 4;

/// Enumeration of all PY32F071xB NVIC interrupts (`0..=31`).
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Interrupt {
    WWDG = 0,
    PVD = 1,
    RTC = 2,
    FLASH = 3,
    RCC_CTC = 4,
    EXTI0_1 = 5,
    EXTI2_3 = 6,
    EXTI4_15 = 7,
    LCD = 8,
    DMA1_CHANNEL1 = 9,
    DMA1_CHANNEL2_3 = 10,
    DMA1_CHANNEL4_5_6_7 = 11,
    ADC_COMP = 12,
    TIM1_BRK_UP_TRG_COM = 13,
    TIM1_CC = 14,
    TIM2 = 15,
    TIM3 = 16,
    TIM6_LPTIM1_DAC = 17,
    TIM7 = 18,
    TIM14 = 19,
    TIM15 = 20,
    TIM16 = 21,
    TIM17 = 22,
    I2C1 = 23,
    I2C2 = 24,
    SPI1 = 25,
    SPI2 = 26,
    USART1 = 27,
    USART2 = 28,
    USART3_4 = 29,
    USB = 31,
}

unsafe impl cortex_m::interrupt::InterruptNumber for Interrupt {
    #[inline(always)]
    fn number(self) -> u16 {
        self as u16
    }
}

#[cfg(all(target_arch = "arm", target_os = "none"))]
mod vectors {
    unsafe extern "C" {
        fn WWDG();
        fn PVD();
        fn RTC();
        fn FLASH();
        fn RCC_CTC();
        fn EXTI0_1();
        fn EXTI2_3();
        fn EXTI4_15();
        fn LCD();
        fn DMA1_CHANNEL1();
        fn DMA1_CHANNEL2_3();
        fn DMA1_CHANNEL4_5_6_7();
        fn ADC_COMP();
        fn TIM1_BRK_UP_TRG_COM();
        fn TIM1_CC();
        fn TIM2();
        fn TIM3();
        fn TIM6_LPTIM1_DAC();
        fn TIM7();
        fn TIM14();
        fn TIM15();
        fn TIM16();
        fn TIM17();
        fn I2C1();
        fn I2C2();
        fn SPI1();
        fn SPI2();
        fn USART1();
        fn USART2();
        fn USART3_4();
        fn USB();
    }

    #[repr(C)]
    pub union Vector {
        _handler: unsafe extern "C" fn(),
        _reserved: u32,
    }

    #[unsafe(link_section = ".vector_table.interrupts")]
    #[unsafe(no_mangle)]
    pub static __INTERRUPTS: [Vector; 32] = [
        Vector { _handler: WWDG },
        Vector { _handler: PVD },
        Vector { _handler: RTC },
        Vector { _handler: FLASH },
        Vector { _handler: RCC_CTC },
        Vector { _handler: EXTI0_1 },
        Vector { _handler: EXTI2_3 },
        Vector { _handler: EXTI4_15 },
        Vector { _handler: LCD },
        Vector {
            _handler: DMA1_CHANNEL1,
        },
        Vector {
            _handler: DMA1_CHANNEL2_3,
        },
        Vector {
            _handler: DMA1_CHANNEL4_5_6_7,
        },
        Vector { _handler: ADC_COMP },
        Vector {
            _handler: TIM1_BRK_UP_TRG_COM,
        },
        Vector { _handler: TIM1_CC },
        Vector { _handler: TIM2 },
        Vector { _handler: TIM3 },
        Vector {
            _handler: TIM6_LPTIM1_DAC,
        },
        Vector { _handler: TIM7 },
        Vector { _handler: TIM14 },
        Vector { _handler: TIM15 },
        Vector { _handler: TIM16 },
        Vector { _handler: TIM17 },
        Vector { _handler: I2C1 },
        Vector { _handler: I2C2 },
        Vector { _handler: SPI1 },
        Vector { _handler: SPI2 },
        Vector { _handler: USART1 },
        Vector { _handler: USART2 },
        Vector { _handler: USART3_4 },
        Vector { _reserved: 0 },
        Vector { _handler: USB },
    ];
}
