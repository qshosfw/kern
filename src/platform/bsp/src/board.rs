use crate::hw::{InputPin, OutputPin, SpiDevice};
use crate::pins as board_pins;
use py32_hal as hal;

// --- GPIO Wrappers ---
pub struct Py32OutputPin {
    pub pin: hal::gpio::Pin,
}

impl OutputPin for Py32OutputPin {
    #[inline(always)]
    fn set_high(&mut self) {
        self.pin.set_high();
    }
    #[inline(always)]
    fn set_low(&mut self) {
        self.pin.set_low();
    }
    #[inline(always)]
    fn toggle(&mut self) {
        if self.pin.is_high() {
            self.pin.set_low();
        } else {
            self.pin.set_high();
        }
    }
}

pub struct Py32InputPin {
    pub pin: hal::gpio::Pin,
}

impl InputPin for Py32InputPin {
    #[inline(always)]
    fn is_high(&self) -> bool {
        self.pin.is_high()
    }
    #[inline(always)]
    fn is_low(&self) -> bool {
        self.pin.is_low()
    }
}

// --- SPI Wrappers ---
pub struct Py32Spi1;

impl SpiDevice for Py32Spi1 {
    #[inline(always)]
    fn write_byte(&mut self, byte: u8) {
        let _ = hal::spi::write_byte(byte);
    }
    #[inline(always)]
    fn transfer_byte(&mut self, byte: u8) -> u8 {
        hal::spi::transfer_byte(byte).unwrap_or(0)
    }
    #[inline(always)]
    fn wait_idle(&mut self) {
        let _ = hal::spi::wait_idle();
    }
}

pub struct Py32Spi2;

impl SpiDevice for Py32Spi2 {
    #[inline(always)]
    fn write_byte(&mut self, byte: u8) {
        let _ = hal::spi::spi2_write_byte(byte);
    }
    #[inline(always)]
    fn transfer_byte(&mut self, byte: u8) -> u8 {
        hal::spi::spi2_transfer(byte).unwrap_or(0)
    }
    #[inline(always)]
    fn wait_idle(&mut self) {
        let _ = hal::spi::spi2_wait_idle();
    }
}

// --- BK4829 Bus Wrapper ---
pub struct RadioBus;

impl RadioBus {
    #[inline(always)]
    fn delay_bus() {
        cortex_m::asm::delay(24); // ~0.5us at 48MHz for clean RF bitbang SPI
    }

    #[inline(always)]
    fn bitbang_write_u8(&mut self, data: u8) {
        for i in (0..8).rev() {
            board_pins::BK_SCL.set_low();
            if ((data >> i) & 1) != 0 {
                board_pins::BK_SDA.set_high();
            } else {
                board_pins::BK_SDA.set_low();
            }
            Self::delay_bus();
            board_pins::BK_SCL.set_high();
            Self::delay_bus();
        }
        board_pins::BK_SCL.set_low();
    }

    #[inline(always)]
    fn bitbang_write_u16(&mut self, data: u16) {
        for i in (0..16).rev() {
            board_pins::BK_SCL.set_low();
            if ((data >> i) & 1) != 0 {
                board_pins::BK_SDA.set_high();
            } else {
                board_pins::BK_SDA.set_low();
            }
            Self::delay_bus();
            board_pins::BK_SCL.set_high();
            Self::delay_bus();
        }
        board_pins::BK_SCL.set_low();
    }

    #[inline(always)]
    fn bitbang_read_u16(&mut self) -> u16 {
        board_pins::BK_SDA.configure_input(py32_pac::gpio::PULL_UP);
        Self::delay_bus();
        let mut val: u16 = 0;
        for _ in 0..16 {
            val <<= 1;
            board_pins::BK_SCL.set_high();
            Self::delay_bus();
            if board_pins::BK_SDA.is_high() {
                val |= 1;
            }
            board_pins::BK_SCL.set_low();
            Self::delay_bus();
        }
        board_pins::BK_SDA.configure_output(py32_pac::gpio::SPEED_HIGH);
        val
    }
}

impl crate::hw::Bk4829Bus for RadioBus {
    fn read_reg(&mut self, addr: u8) -> u16 {
        board_pins::BK_CSN.set_high();
        board_pins::BK_SCL.set_low();
        Self::delay_bus();

        board_pins::BK_CSN.set_low();
        self.bitbang_write_u8(addr | 0x80);
        let val = self.bitbang_read_u16();
        board_pins::BK_CSN.set_high();

        Self::delay_bus();
        board_pins::BK_SCL.set_high();
        board_pins::BK_SDA.set_high();
        val
    }

    fn write_reg(&mut self, addr: u8, val: u16) {
        board_pins::BK_CSN.set_high();
        board_pins::BK_SCL.set_low();
        Self::delay_bus();

        board_pins::BK_CSN.set_low();
        self.bitbang_write_u8(addr);
        Self::delay_bus();
        self.bitbang_write_u16(val);
        Self::delay_bus();
        board_pins::BK_CSN.set_high();

        Self::delay_bus();
        board_pins::BK_SCL.set_high();
        board_pins::BK_SDA.set_high();
    }
}

// --- Board Struct ---
pub struct Board {
    pub spi_lcd: Py32Spi1,
    pub spi_flash: Py32Spi2,
    pub radio_bus: RadioBus,
    pub lcd_cs: Py32OutputPin,
    pub lcd_a0: Py32OutputPin,
    pub flash_cs: Py32OutputPin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLed {
    Off,
    Red,
    Green,
    Yellow,
}

impl Board {
    pub fn set_status_led(bus: &mut RadioBus, led: StatusLed) {
        use crate::hw::Bk4829Bus;
        let reg0d = match led {
            StatusLed::Off => 0x0000,
            StatusLed::Red => 1 << 5,
            StatusLed::Green => 1 << 6,
            StatusLed::Yellow => (1 << 5) | (1 << 6),
        };
        bus.write_reg(0x0D, reg0d);
    }

    pub fn wdt_feed() {
        hal::wdt::Watchdog::feed();
    }

    pub fn uart_write_bytes(data: &[u8]) {
        let _ = hal::uart::write_bytes(data);
    }

    pub fn uart_try_read_byte() -> Option<u8> {
        hal::uart::try_read_byte()
    }

    pub fn init() -> Self {
        // Disable unused timers & DAC to prevent dynamic clock power leakage
        unsafe {
            let val = py32_pac::rcc::RCC_APB1ENR.read_volatile();
            // Disable TIM3, TIM6
            py32_pac::rcc::RCC_APB1ENR
                .write_volatile(val & !(py32_pac::rcc::APB1ENR_TIM3 | py32_pac::rcc::APB1ENR_TIM6));
        }

        // 1. Clock tree & GPIO clock gating
        hal::rcc::init();

        // 2. Board GPIO initialization
        board_pins::init();
        board_pins::disable_swd();

        // 3. System timing peripherals (SysTick 1ms + TIM2 1us counter)
        hal::delay::init();
        hal::time::init();

        // 4. Peripherals (SPI1 for LCD, SPI2 for Flash, USART1 for serial console)
        hal::spi::init();
        hal::spi::init_spi2();
        hal::uart::init();

        // Create the trait wrappers
        Self {
            spi_lcd: Py32Spi1,
            spi_flash: Py32Spi2,
            radio_bus: RadioBus,
            lcd_cs: Py32OutputPin {
                pin: board_pins::LCD_CS,
            },
            lcd_a0: Py32OutputPin {
                pin: board_pins::LCD_A0,
            },
            flash_cs: Py32OutputPin {
                pin: board_pins::FLASH_CS,
            },
        }
    }
}

pub struct BspAudioPa;

impl kern_platform_traits::audio::AudioPa for BspAudioPa {
    #[inline(always)]
    fn enable(&mut self) {
        board_pins::AUDIO_PA.set_high();
    }

    #[inline(always)]
    fn disable(&mut self) {
        board_pins::AUDIO_PA.set_low();
    }

    #[inline(always)]
    fn is_enabled(&self) -> bool {
        board_pins::AUDIO_PA.is_high()
    }
}
