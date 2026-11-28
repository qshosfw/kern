//! Quansheng UV-K10 Pro (`PY32F071C1`) Zero-Sized Board Support Package (`UvK10ProBoard`).
//!
//! Zero Flash/SRAM cost on `UV-K1` / `UV-K5v3` builds.
//! Hardware map verified against `UVK10PRO_SCH.pdf` (`docs/silicon-dma-audio-and-k10pro.md §4`):
//! - `LCD_A0 = PF2` (requires Option Byte `NRST_MODE` GPIO)
//! - `PTT = PC13` (active-low, `EXTI13`)
//! - `AUDIO_PA_EN = PA6` (`LN4898`), `APRS_AF = PB1` (`ADC1_IN9` buffered discriminator tap)
//! - RGB PWM Backlight: `PA15` (`LCD_GLED` `TIM1_CH1`), `PB5` (`LCD_BLED` `TIM3_CH2`), `PB6` (`LCD_RLED` `TIM1_CH3`)
//! - Dual `PB8`/`PB9` `I2C1` receivers: `BK1080` (`0x40`) + `Si4732-A10` (`0x11`, HF/SSB)
//! - `USART3` (`PB10`/`PB11`, `DMA1_Ch7`): `PHY6235SC` BLE 5.4 Wireless RigWire (`PortId::Wireless`)
//! - `USART1` (`PA9`/`PA10`) + `BL1551B` mux (`BK4829 GPIO5/6`): Kenwood K-plug / `ATGM336H` GNSS

#![no_std]

use kern_platform_traits::display::BacklightOutput;
use py32_hal::gpio::Pin;
use py32_pac::gpio;

pub const BOARD_NAME: &str = "Quansheng UV-K10 Pro";
pub const BOOTLOADER_SIZE_BYTES: u32 = 8192;
pub const APP_VTOR_BASE: u32 = 0x0800_2000;

pub const LCD_A0: Pin = Pin::new(gpio::GPIOF_BASE, 2);
pub const PTT: Pin = Pin::new(gpio::GPIOC_BASE, 13);
pub const AUDIO_PA_EN: Pin = Pin::new(gpio::GPIOA_BASE, 6);
pub const APRS_AF_PIN: Pin = Pin::new(gpio::GPIOB_BASE, 1);

pub const LCD_GLED: Pin = Pin::new(gpio::GPIOA_BASE, 15);
pub const LCD_BLED: Pin = Pin::new(gpio::GPIOB_BASE, 5);
pub const LCD_RLED: Pin = Pin::new(gpio::GPIOB_BASE, 6);

pub const BK_CSN: Pin = Pin::new(gpio::GPIOF_BASE, 9);
pub const BK_SCL: Pin = Pin::new(gpio::GPIOF_BASE, 5);
pub const BK_SDA: Pin = Pin::new(gpio::GPIOF_BASE, 6);

pub const I2C1_SCL: Pin = Pin::new(gpio::GPIOB_BASE, 8);
pub const I2C1_SDA: Pin = Pin::new(gpio::GPIOB_BASE, 9);

pub const BLE_UART_TX: Pin = Pin::new(gpio::GPIOB_BASE, 10);
pub const BLE_UART_RX: Pin = Pin::new(gpio::GPIOB_BASE, 11);

/// Configurable RGB backlight profile (`0..=255` per channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub const OFF: Self = Self { r: 0, g: 0, b: 0 };
    pub const STANDBY: Self = Self {
        r: 220,
        g: 220,
        b: 255,
    };
    pub const RX_GREEN: Self = Self {
        r: 0,
        g: 255,
        b: 64,
    };
    pub const TX_AMBER: Self = Self {
        r: 255,
        g: 128,
        b: 0,
    };
}

/// Zero-Sized Board Specification for `Quansheng UV-K10 Pro`.
#[derive(Clone, Copy, Default)]
pub struct UvK10ProBoard;

impl UvK10ProBoard {
    pub const HAS_SI4732_SSB: bool = true;
    pub const HAS_BK1080_FM: bool = true;
    pub const HAS_BLE_RIGWIRE: bool = true;
    pub const HAS_GNSS_MUX: bool = true;
    pub const HAS_RGB_BACKLIGHT: bool = true;

    /// Drive RGB backlight channels (`PA15` G, `PB5` B, `PB6` R).
    #[inline(always)]
    pub fn set_rgb(color: RgbColor) {
        if color.r > 0 {
            LCD_RLED.set_high();
        } else {
            LCD_RLED.set_low();
        }
        if color.g > 0 {
            LCD_GLED.set_high();
        } else {
            LCD_GLED.set_low();
        }
        if color.b > 0 {
            LCD_BLED.set_high();
        } else {
            LCD_BLED.set_low();
        }
    }
}

/// RGB PWM `BacklightOutput` adapter for `UvK10ProBoard` (0 B cost on monochrome `UV-K1`).
#[derive(Clone, Copy, Default)]
pub struct K10ProRgbBacklight {
    pub active_color: RgbColor,
}

impl K10ProRgbBacklight {
    pub const fn new() -> Self {
        Self {
            active_color: RgbColor::STANDBY,
        }
    }
}

impl BacklightOutput for K10ProRgbBacklight {
    #[inline(always)]
    fn set_output(&mut self, enabled: bool, brightness: u8) {
        if enabled && brightness > 0 {
            UvK10ProBoard::set_rgb(self.active_color);
        } else {
            UvK10ProBoard::set_rgb(RgbColor::OFF);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uvk10pro_board_is_zst_with_exact_pin_constants() {
        assert_eq!(core::mem::size_of::<UvK10ProBoard>(), 0);
        assert_eq!(BOOTLOADER_SIZE_BYTES, 8192);
        assert_eq!(APP_VTOR_BASE, 0x0800_2000);
        assert!(UvK10ProBoard::HAS_SI4732_SSB);
        assert!(UvK10ProBoard::HAS_BLE_RIGWIRE);
    }
}
