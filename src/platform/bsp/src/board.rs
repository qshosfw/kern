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
    fn read_bytes(&mut self, buf: &mut [u8]) {
        let _ = hal::spi::spi2_read_bytes(buf);
    }
    #[inline(always)]
    fn write_bytes(&mut self, buf: &[u8]) {
        let _ = hal::spi::spi2_write_bytes(buf);
    }
    #[inline(always)]
    fn wait_idle(&mut self) {
        let _ = hal::spi::spi2_wait_idle();
    }
}

// --- BK4829 Bus Wrapper ---
pub struct RadioBus;

impl RadioBus {
    #[inline(never)]
    fn delay_bus() {
        // ~0.5us setup/hold scaled to current sysclk (~22 cycles @ 48MHz, ~34 @ 72MHz)
        cortex_m::asm::delay((hal::rcc::sysclk_hz() >> 21).max(1));
    }

    #[inline(never)]
    fn bitbang_write_bits(data: u16, bits: u8) {
        for i in (0..bits).rev() {
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

    #[inline(never)]
    fn begin_cmd(cmd: u8) {
        board_pins::BK_CSN.set_high();
        board_pins::BK_SCL.set_low();
        Self::delay_bus();

        board_pins::BK_CSN.set_low();
        Self::bitbang_write_bits(cmd as u16, 8);
    }

    #[inline(never)]
    fn end_cmd() {
        board_pins::BK_CSN.set_high();

        Self::delay_bus();
        board_pins::BK_SCL.set_high();
        board_pins::BK_SDA.set_high();
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

impl kern_platform_traits::radio::RadioBus for RadioBus {
    fn read_reg(&mut self, addr: u8) -> u16 {
        Self::begin_cmd(addr | 0x80);
        let val = self.bitbang_read_u16();
        Self::end_cmd();
        val
    }

    fn write_reg(&mut self, addr: u8, val: u16) {
        Self::begin_cmd(addr);
        Self::delay_bus();
        Self::bitbang_write_bits(val, 16);
        Self::delay_bus();
        Self::end_cmd();
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
#[repr(u8)]
pub enum StatusLed {
    Off = 0,
    Red = 1,
    Green = 2,
    Orange = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedPattern {
    Off,
    Solid(StatusLed),
    BlinkFast(StatusLed),
    BlinkMedium(StatusLed),
    BlinkSlow(StatusLed),
    Pulse(StatusLed),
    Alternate(StatusLed, StatusLed),
}

impl LedPattern {
    #[inline(always)]
    pub fn evaluate(&self, now_ms: u32) -> StatusLed {
        match self {
            Self::Off => StatusLed::Off,
            Self::Solid(c) => *c,
            Self::BlinkFast(c) => {
                if (now_ms / 100) & 1 == 0 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::BlinkMedium(c) => {
                if (now_ms / 250) & 1 == 0 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::BlinkSlow(c) => {
                if (now_ms / 500) & 1 == 0 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::Pulse(c) => {
                if (now_ms % 1000) < 50 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::Alternate(c1, c2) => {
                if (now_ms / 250) & 1 == 0 {
                    *c1
                } else {
                    *c2
                }
            }
        }
    }
}

impl Board {
    pub fn set_status_led(bus: &mut RadioBus, led: StatusLed) {
        use kern_platform_traits::radio::RadioBus as _;
        board_pins::BK_CSN.configure_output(py32_pac::gpio::SPEED_LOW);
        board_pins::BK_SCL.configure_output(py32_pac::gpio::SPEED_MEDIUM);
        board_pins::BK_SDA.configure_output(py32_pac::gpio::SPEED_LOW);

        let mut reg33 = bus.read_reg(0x33);
        if reg33 == 0xFFFF || (reg33 & 0x9000) != 0x9000 {
            reg33 = 0x9000;
        }
        reg33 = (reg33 & !0x0003) | (led as u16);
        bus.write_reg(0x33, reg33);
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

    pub fn uart_set_baud(baud: u32) {
        hal::uart::set_baud(baud);
    }

    #[inline(never)]
    pub fn init() -> Self {
        // Disable unused timers & DAC to prevent dynamic clock power leakage
        unsafe {
            let val = py32_pac::rcc::RCC_APB1ENR.read_volatile();
            // Disable TIM3, TIM6
            py32_pac::rcc::RCC_APB1ENR
                .write_volatile(val & !(py32_pac::rcc::APB1ENR_TIM3 | py32_pac::rcc::APB1ENR_TIM6));
        }

        // 1. Clock tree & GPIO clock gating (explicitly establish 48 MHz PLL & 1WS flash latency)
        hal::rcc::init();
        hal::rcc::set_sysclk_48mhz();

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

/// Standalone zero-cost helper to control board status LEDs via BK4819/BK4829 bus.
pub fn set_status_led(led: StatusLed) {
    #[cfg(target_arch = "arm")]
    {
        let mut bus = RadioBus;
        Board::set_status_led(&mut bus, led);
    }
    #[cfg(not(target_arch = "arm"))]
    {
        let _ = led;
    }
}

/// Immediately quench all RF PA/TX stages, audio amp, and flashlight during a fault.
#[inline(never)]
pub fn bk4829_fault_quench() {
    use kern_platform_traits::radio::RadioBus as _;
    board_pins::AUDIO_PA.set_low();
    board_pins::FLASHLIGHT.set_low();
    board_pins::BK_CSN.configure_output(py32_pac::gpio::SPEED_LOW);
    board_pins::BK_SCL.configure_output(py32_pac::gpio::SPEED_MEDIUM);
    board_pins::BK_SDA.configure_output(py32_pac::gpio::SPEED_LOW);
    let mut bus = RadioBus;
    bus.write_reg(0x36, 0x0022); // PA bias off
    bus.write_reg(0x30, 0x0000); // Disable TX, RX, VCO, PLL, AF, DSP
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
