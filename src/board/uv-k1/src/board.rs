use crate::hw::{InputPin, OutputPin, SpiDevice};
use crate::pins as board_pins;
pub use kern_platform_traits::display::{LedPattern, StatusLed};
use kern_platform_traits::spi::SpiSpeed;
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
    fn write_bytes(&mut self, buf: &[u8]) {
        let _ = hal::spi::write_bytes(buf);
    }
    #[inline(always)]
    fn transfer_byte(&mut self, byte: u8) -> u8 {
        hal::spi::transfer_byte(byte).unwrap_or(0)
    }
    #[inline(always)]
    fn wait_idle(&mut self) {
        let _ = hal::spi::wait_idle();
    }
    #[inline(always)]
    fn set_speed(&mut self, speed: SpiSpeed) {
        hal::spi::set_spi1_speed(match speed {
            SpiSpeed::Turbo => hal::spi::Spi1Speed::Turbo,
            SpiSpeed::Standard => hal::spi::Spi1Speed::Standard,
        });
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
    #[inline(always)]
    fn delay_bus() {
        // Cached O(1) ~0.45us setup/hold scaled to active sysclk (zero RCC MMIO reads)
        cortex_m::asm::delay(hal::rcc::radio_half_bit_cycles());
    }

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

    fn begin_cmd(cmd: u8) {
        board_pins::BK_CSN.set_high();
        board_pins::BK_SCL.set_low();
        Self::delay_bus();

        board_pins::BK_CSN.set_low();
        Self::bitbang_write_bits(cmd as u16, 8);
    }

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
        board_pins::BK_SDA.configure_output(py32_pac::gpio::SPEED_LOW);
        val
    }
}

impl kern_platform_traits::radio::RadioBus for RadioBus {
    fn read_reg(&mut self, addr: u8) -> u16 {
        cortex_m::interrupt::free(|_| {
            Self::begin_cmd(addr | 0x80);
            let val = self.bitbang_read_u16();
            Self::end_cmd();
            val
        })
    }

    fn write_reg(&mut self, addr: u8, val: u16) {
        cortex_m::interrupt::free(|_| {
            Self::begin_cmd(addr);
            Self::delay_bus();
            Self::bitbang_write_bits(val, 16);
            Self::delay_bus();
            Self::end_cmd();
        });
    }
}

impl kern_platform_traits::audio::AudioPa for RadioBus {
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

impl kern_platform_traits::time::Delay for RadioBus {
    #[inline(always)]
    fn delay_us(&mut self, us: u32) {
        hal::delay::delay_us(us);
    }

    #[inline(always)]
    fn delay_ms(&mut self, ms: u32) {
        hal::delay::delay_ms(ms);
    }
}

// --- Zero-Cost ZST Board Adapters ---

#[derive(Clone, Copy, Default)]
pub struct BspDelay;

impl kern_platform_traits::time::Delay for BspDelay {
    #[inline(always)]
    fn delay_us(&mut self, us: u32) {
        hal::delay::delay_us(us);
    }

    #[inline(always)]
    fn delay_ms(&mut self, ms: u32) {
        hal::delay::delay_ms(ms);
    }
}

#[derive(Clone, Copy, Default)]
pub struct BspTime;

impl kern_platform_traits::time::TimeSource for BspTime {
    #[inline(always)]
    fn now_us(&self) -> u32 {
        hal::time::now_us()
    }

    #[inline(always)]
    fn now_ms(&self) -> u32 {
        hal::delay::millis()
    }
}

#[derive(Clone, Copy, Default)]
pub struct BspWatchdog;

impl kern_platform_traits::wdt::WatchdogDevice for BspWatchdog {
    #[inline(always)]
    fn feed(&mut self) {
        hal::wdt::Watchdog::feed();
    }

    #[inline(always)]
    fn is_watchdog_reset(&self) -> bool {
        matches!(
            hal::wdt::Watchdog::reset_cause(),
            hal::wdt::ResetCause::IndependentWatchdog | hal::wdt::ResetCause::WindowWatchdog
        )
    }
}

#[derive(Clone, Copy, Default)]
pub struct LcdBus;

impl SpiDevice for LcdBus {
    #[inline(always)]
    fn write_byte(&mut self, byte: u8) {
        Py32Spi1.write_byte(byte);
    }
    #[inline(always)]
    fn write_bytes(&mut self, buf: &[u8]) {
        Py32Spi1.write_bytes(buf);
    }
    #[inline(always)]
    fn transfer_byte(&mut self, byte: u8) -> u8 {
        Py32Spi1.transfer_byte(byte)
    }
    #[inline(always)]
    fn wait_idle(&mut self) {
        Py32Spi1.wait_idle();
    }
    #[inline(always)]
    fn set_speed(&mut self, speed: SpiSpeed) {
        Py32Spi1.set_speed(speed);
    }
}

impl kern_platform_traits::time::Delay for LcdBus {
    #[inline(always)]
    fn delay_us(&mut self, us: u32) {
        hal::delay::delay_us(us);
    }
    #[inline(always)]
    fn delay_ms(&mut self, ms: u32) {
        hal::delay::delay_ms(ms);
    }
}

impl kern_platform_traits::display::CommandDisplayBus for LcdBus {
    #[inline(always)]
    fn cs_low(&mut self) {
        board_pins::LCD_CS.set_low();
    }
    #[inline(always)]
    fn cs_high(&mut self) {
        board_pins::LCD_CS.set_high();
    }
    #[inline(always)]
    fn dc_low(&mut self) {
        board_pins::LCD_A0.set_low();
    }
    #[inline(always)]
    fn dc_high(&mut self) {
        board_pins::LCD_A0.set_high();
    }
}

#[derive(Clone, Copy, Default)]
pub struct FlashBus;

impl SpiDevice for FlashBus {
    #[inline(always)]
    fn write_byte(&mut self, byte: u8) {
        Py32Spi2.write_byte(byte);
    }
    #[inline(always)]
    fn transfer_byte(&mut self, byte: u8) -> u8 {
        Py32Spi2.transfer_byte(byte)
    }
    #[inline(always)]
    fn read_bytes(&mut self, buf: &mut [u8]) {
        Py32Spi2.read_bytes(buf);
    }
    #[inline(always)]
    fn write_bytes(&mut self, buf: &[u8]) {
        Py32Spi2.write_bytes(buf);
    }
    #[inline(always)]
    fn wait_idle(&mut self) {
        Py32Spi2.wait_idle();
    }
}

impl kern_platform_traits::time::Delay for FlashBus {
    #[inline(always)]
    fn delay_us(&mut self, us: u32) {
        hal::delay::delay_us(us);
    }
    #[inline(always)]
    fn delay_ms(&mut self, ms: u32) {
        hal::delay::delay_ms(ms);
    }
}

impl kern_platform_traits::wdt::WatchdogDevice for FlashBus {
    #[inline(always)]
    fn feed(&mut self) {
        hal::wdt::Watchdog::feed();
    }
    #[inline(always)]
    fn is_watchdog_reset(&self) -> bool {
        matches!(
            hal::wdt::Watchdog::reset_cause(),
            hal::wdt::ResetCause::IndependentWatchdog | hal::wdt::ResetCause::WindowWatchdog
        )
    }
}

impl kern_platform_traits::flash::SpiFlashBus for FlashBus {
    #[inline(always)]
    fn cs_low(&mut self) {
        board_pins::FLASH_CS.set_low();
    }
    #[inline(always)]
    fn cs_high(&mut self) {
        board_pins::FLASH_CS.set_high();
    }
}

#[derive(Clone, Copy, Default)]
pub struct BspBacklight;

impl kern_platform_traits::display::BacklightOutput for BspBacklight {
    #[inline(always)]
    fn set_output(&mut self, on: bool, rgb_mask: u8) {
        #[cfg(not(feature = "target_k10pro"))]
        {
            let _ = rgb_mask;
            if on {
                board_pins::BACKLIGHT.set_high();
            } else {
                board_pins::BACKLIGHT.set_low();
            }
        }
        #[cfg(feature = "target_k10pro")]
        {
            let mask = if on { rgb_mask } else { 0 };
            if (mask & 0b001) != 0 {
                board_pins::LCD_RLED.set_high();
            } else {
                board_pins::LCD_RLED.set_low();
            }
            if (mask & 0b010) != 0 {
                board_pins::LCD_GLED.set_high();
            } else {
                board_pins::LCD_GLED.set_low();
            }
            if (mask & 0b100) != 0 {
                board_pins::LCD_BLED.set_high();
            } else {
                board_pins::LCD_BLED.set_low();
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Uart1Serial;

impl kern_platform_traits::serial::SerialBackend for Uart1Serial {
    #[inline(always)]
    fn write_bytes(&mut self, data: &[u8]) {
        hal::uart::mark_activity();
        let _ = hal::uart::write_bytes(data);
    }

    #[inline(always)]
    fn try_read_byte(&mut self) -> Option<u8> {
        let byte = hal::uart::try_read_byte();
        if byte.is_some() {
            hal::uart::mark_activity();
        }
        byte
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

    /// Control an individual BK4829 GPIO output pin (0..=6) in Reg 0x33 without altering other bits.
    /// Used on K10 Pro for `BT_EN` (`GPIO5`) and `GPS_EN` + `BL1551B` mux (`GPIO6`).
    pub fn set_bk_gpio(bus: &mut RadioBus, gpio_idx: u8, high: bool) {
        use kern_platform_traits::radio::RadioBus as _;
        if gpio_idx > 6 {
            return;
        }
        let mut reg33 = bus.read_reg(0x33);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        // BK4829 Reg 0x33 bits [6:0] are GPIO6..GPIO0 output values (bit (6 - idx) or direct idx?
        // Per Reg 0x33 mapping: bit 0..=2 correspond to GPIO0..GPIO2, GPIO5/6 occupy bits 5/6).
        let mask = 1u16 << gpio_idx;
        if high {
            reg33 |= mask;
        } else {
            reg33 &= !mask;
        }
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
        #[cfg(feature = "target_k10pro")]
        hal::uart::init_usart3(115_200);

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

#[cfg(feature = "target_k10pro")]
pub struct Usart3Serial;

#[cfg(feature = "target_k10pro")]
impl kern_platform_traits::serial::SerialBackend for Usart3Serial {
    fn write_bytes(&mut self, data: &[u8]) {
        let _ = hal::uart::UsartInstance::USART3.write_bytes(data);
    }

    #[inline(always)]
    fn try_read_byte(&mut self) -> Option<u8> {
        hal::uart::UsartInstance::USART3.try_read_byte()
    }
}

#[cfg(feature = "target_k10pro")]
impl kern_platform_traits::serial::SerialControl for Usart3Serial {
    fn set_baud(&mut self, baud: u32) {
        hal::uart::set_usart3_baud(baud);
    }

    fn flush(&mut self) {
        hal::uart::UsartInstance::USART3.wait_tx_complete();
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

#[derive(Clone, Copy, Default)]
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

// --- Board Keypad Matrix Scanner (PY32F071 GPIOB / K10 Pro) ---

#[derive(Clone, Copy, Default)]
pub struct BspKeyMatrix;

impl BspKeyMatrix {
    const GPIOB_BASE: u32 = py32_pac::gpio::GPIOB_BASE;
    #[cfg(not(feature = "target_k10pro"))]
    const BSRR_OFFSET: u32 = py32_pac::gpio::BSRR_OFFSET;
    const IDR_OFFSET: u32 = py32_pac::gpio::IDR_OFFSET;
    #[cfg(not(feature = "target_k10pro"))]
    const PIN_MASK_COLS: u32 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3);
    const PIN_MASK_ROWS: u32 = (1 << 15) | (1 << 14) | (1 << 13) | (1 << 12);

    #[inline(always)]
    fn set_cols_high() {
        #[cfg(not(feature = "target_k10pro"))]
        {
            let bsrr = (Self::GPIOB_BASE + Self::BSRR_OFFSET) as *mut u32;
            unsafe {
                bsrr.write_volatile(Self::PIN_MASK_COLS);
            }
        }
        #[cfg(feature = "target_k10pro")]
        {
            for (idx, col) in board_pins::KB_COLS.iter().enumerate() {
                #[cfg(feature = "swd_debug")]
                if idx == 3 {
                    continue;
                }
                let _ = idx;
                col.set_high();
            }
        }
    }

    #[inline(always)]
    fn clear_col(col_idx: usize) {
        #[cfg(not(feature = "target_k10pro"))]
        {
            let pin = 6 - col_idx;
            let bsrr = (Self::GPIOB_BASE + Self::BSRR_OFFSET) as *mut u32;
            unsafe {
                bsrr.write_volatile(1 << (pin + 16));
            }
        }
        #[cfg(feature = "target_k10pro")]
        {
            #[cfg(feature = "swd_debug")]
            if col_idx == 3 {
                return;
            }
            board_pins::KB_COLS[col_idx].set_low();
        }
    }

    #[inline(always)]
    fn read_rows_raw() -> u32 {
        let idr = (Self::GPIOB_BASE + Self::IDR_OFFSET) as *const u32;
        unsafe { idr.read_volatile() & Self::PIN_MASK_ROWS }
    }

    #[inline(always)]
    fn read_ptt_raw() -> bool {
        #[cfg(not(feature = "target_k10pro"))]
        {
            let idr = (Self::GPIOB_BASE + Self::IDR_OFFSET) as *const u32;
            unsafe { (idr.read_volatile() & (1 << 10)) == 0 }
        }
        #[cfg(feature = "target_k10pro")]
        {
            board_pins::PTT.is_low()
        }
    }

    #[inline(always)]
    fn set_all_cols_low() {
        #[cfg(not(feature = "target_k10pro"))]
        {
            let bsrr = (Self::GPIOB_BASE + Self::BSRR_OFFSET) as *mut u32;
            unsafe {
                bsrr.write_volatile(Self::PIN_MASK_COLS << 16);
            }
        }
        #[cfg(feature = "target_k10pro")]
        {
            for (idx, col) in board_pins::KB_COLS.iter().enumerate() {
                #[cfg(feature = "swd_debug")]
                if idx == 3 {
                    continue;
                }
                let _ = idx;
                col.set_low();
            }
        }
    }

    /// Scan the 5-column matrix and dedicated PTT pin into a 19-bit active-key bitmask.
    pub fn scan_raw_mask() -> u32 {
        let settle_2us = (hal::rcc::sysclk_hz() >> 19) + 5;

        // Fast idle check: pull all matrix columns low at once. If all row pull-ups remain high
        // and PTT is inactive, zero keys (matrix, side1/2, or PTT) are pressed — skip 5-col settle loops.
        Self::set_all_cols_low();
        cortex_m::asm::delay(settle_2us);
        let any_row_low = Self::read_rows_raw() != Self::PIN_MASK_ROWS;
        let ptt_down = Self::read_ptt_raw();
        if !any_row_low {
            Self::set_cols_high();
            return if ptt_down { 1 << 18 } else { 0 };
        }

        let settle_1us = (settle_2us >> 1).max(16);
        let mut mask: u32 = 0;

        let mut side_keys = 0u32;
        for j in 0..5 {
            Self::set_cols_high();
            if j > 0 {
                Self::clear_col(j - 1);
            }

            cortex_m::asm::delay(settle_2us);

            let mut reg = 0u32;
            let mut i = 0usize;
            let mut k = 0usize;
            while i < 3 && k < 12 {
                cortex_m::asm::delay(settle_1us);
                let reg2 = Self::read_rows_raw();
                if reg == reg2 {
                    i += 1;
                } else {
                    i = 0;
                    reg = reg2;
                }
                k += 1;
            }

            if i >= 2 {
                let row_bits = (!reg >> 12) & 0x0F;
                let rev = ((row_bits & 1) << 3)
                    | ((row_bits & 2) << 1)
                    | ((row_bits & 4) >> 1)
                    | ((row_bits & 8) >> 3);
                if j == 0 {
                    side_keys = rev & 0x03;
                    mask |= side_keys;
                } else {
                    let col_rev = rev & !side_keys;
                    mask |= col_rev << (4 * j - 2);
                }
            }
        }

        Self::set_cols_high();

        if Self::read_ptt_raw() {
            mask |= 1 << 18;
        }

        mask
    }

    pub fn reset_gpio_pins() {
        for col in &board_pins::KB_COLS {
            col.configure_output(py32_pac::gpio::SPEED_HIGH);
            col.set_high();
        }
        for row in &board_pins::KB_ROWS {
            row.configure_input(py32_pac::gpio::PULL_UP);
        }
        board_pins::PTT.configure_input(py32_pac::gpio::PULL_UP);
    }
}

impl kern_platform_traits::input::KeyMatrix for BspKeyMatrix {
    #[inline(always)]
    fn poll_raw_mask(&self) -> u32 {
        Self::scan_raw_mask()
    }

    #[inline(always)]
    fn reset_pins(&self) {
        Self::reset_gpio_pins();
    }

    #[inline(always)]
    fn now_ms(&self) -> u32 {
        py32_hal::delay::millis()
    }
}

/// Zero-sized board USB 2.0 Full-Speed Device Controller bus for `PY32F071xB`.
#[derive(Clone, Copy, Default)]
pub struct BspUsbBus;

impl kern_platform_traits::usb::UsbBus for BspUsbBus {
    #[inline(always)]
    fn reg_read(&self, off: usize) -> u8 {
        unsafe { ((py32_pac::usbd::USBD_BASE as *const u8).add(off)).read_volatile() }
    }

    #[inline(always)]
    fn reg_write(&self, off: usize, val: u8) {
        unsafe { ((py32_pac::usbd::USBD_BASE as *mut u8).add(off)).write_volatile(val) }
    }

    #[inline(always)]
    fn reg_read16(&self, off: usize) -> u16 {
        unsafe { ((py32_pac::usbd::USBD_BASE as *const u8).add(off) as *const u16).read_volatile() }
    }

    #[inline(always)]
    fn fifo_read_byte(&self, ep: u8) -> u8 {
        unsafe { py32_pac::usbd::fifo_ep_u8(ep).read_volatile() }
    }

    #[inline(always)]
    fn fifo_write_byte(&self, ep: u8, b: u8) {
        unsafe { py32_pac::usbd::fifo_ep_u8(ep).write_volatile(b) }
    }

    #[inline(always)]
    fn millis(&self) -> u32 {
        py32_hal::delay::millis()
    }

    #[inline(always)]
    fn enable_usbd(&self) {
        py32_hal::rcc::enable_usbd();
    }

    #[inline(always)]
    fn disable_usbd(&self) {
        py32_hal::rcc::disable_usbd();
    }

    #[inline(always)]
    fn ensure_48mhz(&self) {
        if py32_hal::rcc::sysclk_hz() != 48_000_000 {
            py32_hal::rcc::set_sysclk_48mhz();
        }
    }

    #[inline(always)]
    fn set_nvic(&self, enable: bool) {
        #[cfg(all(not(test), target_arch = "arm"))]
        unsafe {
            let irq = py32_pac::Interrupt::USB as u8;
            py32_pac::nvic_clear_pending(irq);
            if enable {
                py32_pac::nvic_enable(irq);
            } else {
                py32_pac::nvic_disable(irq);
            }
        }
        #[cfg(not(all(not(test), target_arch = "arm")))]
        let _ = enable;
    }

    #[inline(always)]
    fn mark_serial_activity(&self) {
        py32_hal::uart::mark_activity();
    }

    #[inline(always)]
    fn fill_device_identity(&self, mac: &mut [u8; 6], crock: &mut [u8; 15]) {
        crate::uid::fill_device_identity(mac, crock);
    }

    #[inline(always)]
    fn default_product_name(&self) -> &'static str {
        crate::pins::BOARD_SPEC.usb_product_default
    }
}

// --- On-Chip Voice ADC/DAC Frontend (PA4 / ADC1 CH4 + DAC1 CH1) ---

#[derive(Clone, Copy, Default)]
pub struct BspVoiceAdc;

impl BspVoiceAdc {
    unsafe fn adc_convert_single(cr2: u32) -> i32 {
        use py32_pac as pac;
        unsafe {
            pac::adc::ADC_SR.write_volatile(!pac::adc::SR_EOC);
            pac::adc::ADC_CR2.write_volatile(cr2 | pac::adc::CR2_EXTTRIG | pac::adc::CR2_SWSTART);
            let mut timeout = 2000;
            while (pac::adc::ADC_SR.read_volatile() & pac::adc::SR_EOC) == 0 && timeout > 0 {
                timeout -= 1;
            }
            (pac::adc::ADC_DR.read_volatile() & 0x0FFF) as i32
        }
    }

    unsafe fn adc_select_channel(ch: u32) {
        use py32_pac as pac;
        unsafe {
            pac::adc::ADC_SQR1.write_volatile(0);
            pac::adc::ADC_SQR3.write_volatile(ch);
        }
    }
}

impl kern_platform_traits::time::Delay for BspVoiceAdc {
    #[inline(always)]
    fn delay_us(&mut self, us: u32) {
        hal::delay::delay_us(us);
    }
    #[inline(always)]
    fn delay_ms(&mut self, ms: u32) {
        hal::delay::delay_ms(ms);
    }
}

impl kern_platform_traits::audio::VoiceAdcBackend for BspVoiceAdc {
    fn begin_voice_bias(&mut self) {
        use py32_pac as pac;
        unsafe {
            board_pins::VOICE_DAC.configure_analog();
            let apb1 = pac::rcc::RCC_APB1ENR.read_volatile();
            pac::rcc::RCC_APB1ENR.write_volatile(apb1 | pac::rcc::APB1ENR_DAC);

            pac::dac::DAC_DHR12R1.write_volatile(2048);
            pac::dac::DAC_CR.write_volatile(pac::dac::CR_EN1);
            pac::dac::DAC_SWTRIGR.write_volatile(pac::dac::SWTRIGR_SWTRIG1);

            let mut cr2 = pac::adc::ADC_CR2.read_volatile();
            if (cr2 & pac::adc::CR2_ADON) == 0 {
                cr2 |= pac::adc::CR2_ADON | pac::adc::CR2_EXTSEL_SWSTART;
                pac::adc::ADC_CR2.write_volatile(cr2);
                hal::delay::delay_us(150);
            }
            Self::adc_select_channel(4);
            hal::delay::delay_us(50);
        }
    }

    #[inline(always)]
    fn select_voice_channel(&mut self) {
        unsafe {
            Self::adc_select_channel(4);
        }
    }

    #[inline(always)]
    fn read_voice_sample(&mut self) -> i32 {
        unsafe {
            let cr2 = py32_pac::adc::ADC_CR2.read_volatile();
            Self::adc_convert_single(cr2)
        }
    }

    fn end_voice_bias(&mut self) {
        use py32_pac as pac;
        unsafe {
            pac::dac::DAC_CR.write_volatile(0);
            let apb1 = pac::rcc::RCC_APB1ENR.read_volatile();
            pac::rcc::RCC_APB1ENR.write_volatile(apb1 & !pac::rcc::APB1ENR_DAC);
            board_pins::VOICE_DAC.configure_analog();
            Self::adc_select_channel(8);
        }
    }

    #[inline(always)]
    fn should_abort(&self) -> bool {
        BspKeyMatrix::scan_raw_mask() != 0
    }
}
