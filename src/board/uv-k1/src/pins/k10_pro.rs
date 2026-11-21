//! Pinout and hardware descriptors for the Quansheng UV-K10 Pro (`PY32F071C1` LQFP48).
//!
//! Verified against `inspo/k10pro/UVK10PRO_SCH.pdf` and `K10PRO_ENG_v8.02.16_publish.bin`.

use py32_hal::gpio::Pin;
use py32_pac::gpio;

// ── Target Feature Descriptor ───────────────────────────────────────────────
pub const HAS_FLASHLIGHT: bool = false;
pub const HAS_RGB_BACKLIGHT: bool = true;
pub const HAS_COPROC: bool = true;
pub const HAS_GNSS: bool = true;
pub const HAS_EXTRA_KEY_ROW: bool = true;

// ── LCD (ST7565 via HW SPI1) ────────────────────────────────────────────────
pub const LCD_A0: Pin = Pin::new(gpio::GPIOF_BASE, 2);
pub const LCD_CS: Pin = Pin::new(gpio::GPIOB_BASE, 2);

// ── BK4829 RF Transceiver (bit-bang SPI) ────────────────────────────────────
pub const BK_SCL: Pin = Pin::new(gpio::GPIOF_BASE, 5);
pub const BK_SDA: Pin = Pin::new(gpio::GPIOF_BASE, 6);
pub const BK_CSN: Pin = Pin::new(gpio::GPIOF_BASE, 9);

/// BK4829 general-purpose output controlling `PHY6235SC` power gate (`Q800`).
pub const BK_GPIO_BT_EN: u8 = 5;
/// BK4829 general-purpose output controlling `ATGM336H` LDO (`XC6204B332`) and `BL1551B` UART1 mux.
pub const BK_GPIO_GPS_EN: u8 = 6;

// ── Keyboard Matrix (4x5 + Side + PTT) ──────────────────────────────────────
// Note: `KEY_OUT4` shares `PA14` (`SWCLK`); SWD debug builds preserve SWD pins.
pub const KB_COLS: [Pin; 4] = [
    Pin::new(gpio::GPIOF_BASE, 8),
    Pin::new(gpio::GPIOB_BASE, 4),
    Pin::new(gpio::GPIOB_BASE, 3),
    Pin::new(gpio::GPIOA_BASE, 14),
];
pub const KB_ROWS: [Pin; 4] = [
    Pin::new(gpio::GPIOB_BASE, 15),
    Pin::new(gpio::GPIOB_BASE, 14),
    Pin::new(gpio::GPIOB_BASE, 13),
    Pin::new(gpio::GPIOB_BASE, 12),
];
pub const KB_ROW5: Option<Pin> = Some(Pin::new(gpio::GPIOA_BASE, 8));

// ── Misc I/O ────────────────────────────────────────────────────────────────
pub const PTT: Pin = Pin::new(gpio::GPIOC_BASE, 13);
pub const AUDIO_PA: Pin = Pin::new(gpio::GPIOA_BASE, 6);
pub const VOICE_DAC: Pin = Pin::new(gpio::GPIOA_BASE, 4);
pub const FLASH_CS: Pin = Pin::new(gpio::GPIOA_BASE, 3);
pub const FLASHLIGHT: Pin = Pin::none();

// ── RGB LCD & Keypad Backlight ──────────────────────────────────────────────
// LCD_RLED = PB6 (TIM1_CH3), LCD_GLED = PA15 (TIM1_CH1), LCD_BLED = PB5 (TIM3_CH2)
pub const LCD_RLED: Pin = Pin::new(gpio::GPIOB_BASE, 6);
pub const LCD_GLED: Pin = Pin::new(gpio::GPIOA_BASE, 15);
pub const LCD_BLED: Pin = Pin::new(gpio::GPIOB_BASE, 5);
/// Keypad backlight LED transistor (`Q29`) shared with `SWDIO` (`PA13`).
pub const KEY_LED: Pin = Pin::new(gpio::GPIOA_BASE, 13);
/// Primary backlight pin alias for monochrome/legacy consumers.
pub const BACKLIGHT: Pin = LCD_RLED;

// ── Analog / Baseband ───────────────────────────────────────────────────────
pub const VBAT_ADC: Pin = Pin::new(gpio::GPIOB_BASE, 0);
pub const APRS_AF: Pin = Pin::new(gpio::GPIOB_BASE, 1);

// ── BK1080 Broadcast FM Radio & Shared I2C Bus ──────────────────────────────
pub const FM_SCL: Pin = Pin::new(gpio::GPIOB_BASE, 8);
pub const FM_SDA: Pin = Pin::new(gpio::GPIOB_BASE, 9);

// ── Coprocessor Link (`PHY6235SC` over `USART3` AF4) ────────────────────────
pub const BT_UART_TX: Pin = Pin::new(gpio::GPIOB_BASE, 10);
pub const BT_UART_RX: Pin = Pin::new(gpio::GPIOB_BASE, 11);

/// Initialize all GPIO pins for UV-K10 Pro.
pub fn init() {
    use py32_pac as pac;

    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        let ioprstr = (pac::rcc::RCC_BASE + 0x24) as *mut u32;
        ioprstr.write_volatile((1 << 1) | (1 << 2));
        ioprstr.write_volatile(0);
    }

    for &pin in &[KB_ROWS[0], KB_ROWS[1], KB_ROWS[2], KB_ROWS[3], PTT] {
        pin.configure_input(pac::gpio::PULL_UP);
    }
    if let Some(row5) = KB_ROW5 {
        row5.configure_input(pac::gpio::PULL_UP);
    }

    for (idx, &col) in KB_COLS.iter().enumerate() {
        #[cfg(feature = "swd_debug")]
        if idx == 3 {
            continue;
        }
        let _ = idx;
        col.configure_output(pac::gpio::SPEED_HIGH);
        col.set_high();
    }

    AUDIO_PA.configure_output(pac::gpio::SPEED_LOW);
    AUDIO_PA.set_low();

    for &led in &[LCD_RLED, LCD_GLED, LCD_BLED] {
        led.configure_output(pac::gpio::SPEED_LOW);
        led.set_low();
    }
    #[cfg(not(feature = "swd_debug"))]
    {
        KEY_LED.configure_output(pac::gpio::SPEED_LOW);
        KEY_LED.set_low();
    }

    FM_SCL.configure_output_od(pac::gpio::SPEED_LOW);
    FM_SDA.configure_output_od(pac::gpio::SPEED_LOW);
    FM_SCL.set_high();
    FM_SDA.set_high();

    for &pin in &[LCD_A0, LCD_CS, FLASH_CS, BK_SDA, BK_CSN] {
        pin.configure_output(pac::gpio::SPEED_LOW);
        pin.set_high();
    }
    BK_SCL.configure_output(pac::gpio::SPEED_MEDIUM);
    BK_SCL.set_high();

    py32_hal::gpio::set_pins_analog(
        pac::gpio::GPIOA_BASE,
        pac::gpio::PIN_0 | pac::gpio::PIN_1 | pac::gpio::PIN_2 | pac::gpio::PIN_4,
    );
    py32_hal::gpio::set_pins_analog(
        pac::gpio::GPIOB_BASE,
        pac::gpio::PIN_0 | pac::gpio::PIN_1 | pac::gpio::PIN_7,
    );
    // Park unused GPIOC (all except PC13 PTT) and GPIOF pins to analog to minimize leakage/EMI.
    py32_hal::gpio::set_pins_analog(
        pac::gpio::GPIOC_BASE,
        pac::gpio::PIN_0
            | pac::gpio::PIN_1
            | pac::gpio::PIN_2
            | pac::gpio::PIN_3
            | pac::gpio::PIN_4
            | pac::gpio::PIN_5
            | pac::gpio::PIN_6
            | pac::gpio::PIN_7
            | pac::gpio::PIN_8
            | pac::gpio::PIN_9
            | pac::gpio::PIN_10
            | pac::gpio::PIN_11
            | pac::gpio::PIN_12
            | pac::gpio::PIN_14
            | pac::gpio::PIN_15,
    );
    py32_hal::gpio::set_pins_analog(
        pac::gpio::GPIOF_BASE,
        pac::gpio::PIN_0
            | pac::gpio::PIN_1
            | pac::gpio::PIN_3
            | pac::gpio::PIN_4
            | pac::gpio::PIN_7,
    );

    // Enable hardware GPIO glitch filters on slow mechanical inputs (PTT PC13, keypad rows
    // PB12..PB15 + PA8) to reject RF pickup during TX; never enable on PA11/PA12 (12 Mbps USB).
    #[cfg(all(not(test), target_arch = "arm"))]
    {
        py32_hal::rcc::enable_syscfg();
        unsafe {
            let pa = pac::syscfg::SYSCFG_PAENS.read_volatile();
            pac::syscfg::SYSCFG_PAENS.write_volatile(pa | pac::gpio::PIN_8);
            let pb = pac::syscfg::SYSCFG_PBENS.read_volatile();
            pac::syscfg::SYSCFG_PBENS.write_volatile(
                pb | pac::gpio::PIN_12 | pac::gpio::PIN_13 | pac::gpio::PIN_14 | pac::gpio::PIN_15,
            );
            let pc = pac::syscfg::SYSCFG_PCENS.read_volatile();
            pac::syscfg::SYSCFG_PCENS.write_volatile(pc | pac::gpio::PIN_13);
        }
    }
}

/// On UV-K10 Pro, `PA14` drives `KEY_OUT4` and `PA13` drives `KEY_LED`, so disabling SWD configures
/// them as keypad outputs rather than placing `PA14` into Analog mode.
pub fn disable_swd() {
    #[cfg(not(feature = "swd_debug"))]
    {
        use py32_pac as pac;
        KB_COLS[3].configure_output(pac::gpio::SPEED_HIGH);
        KB_COLS[3].set_high();
        KEY_LED.configure_output(pac::gpio::SPEED_LOW);
        KEY_LED.set_low();
    }
}

const STOP_PORTS: [u32; 3] = [
    py32_pac::gpio::GPIOA_BASE,
    py32_pac::gpio::GPIOB_BASE,
    py32_pac::gpio::GPIOF_BASE,
];

/// Park active peripheral and matrix GPIO pins before entering `PWR Stop`.
pub fn park_for_stop(uart_rx_wake: bool) -> [u32; 6] {
    use py32_pac::gpio as g;

    let mut saved = [0u32; 6];
    for (idx, &base) in STOP_PORTS.iter().enumerate() {
        unsafe {
            saved[idx * 2] = ((base + g::MODER_OFFSET) as *const u32).read_volatile();
            saved[idx * 2 + 1] = ((base + g::PUPDR_OFFSET) as *const u32).read_volatile();
        }
    }

    AUDIO_PA.set_low();
    LCD_RLED.set_low();
    LCD_GLED.set_low();
    LCD_BLED.set_low();
    #[cfg(not(feature = "swd_debug"))]
    KEY_LED.set_low();
    FLASH_CS.set_high();
    LCD_CS.set_high();
    BK_CSN.set_high();

    let mut gpioa_analog =
        g::PIN_0 | g::PIN_1 | g::PIN_2 | g::PIN_5 | g::PIN_6 | g::PIN_7 | g::PIN_8 | g::PIN_9;
    if !uart_rx_wake {
        gpioa_analog |= g::PIN_10;
    } else {
        Pin::new(g::GPIOA_BASE, 10).configure_input(g::PULL_UP);
    }
    py32_hal::gpio::set_pins_analog(g::GPIOA_BASE, gpioa_analog);
    py32_hal::gpio::set_pins_analog(
        g::GPIOB_BASE,
        g::PIN_3
            | g::PIN_4
            | g::PIN_8
            | g::PIN_9
            | g::PIN_10
            | g::PIN_11
            | g::PIN_12
            | g::PIN_13
            | g::PIN_14,
    );
    py32_hal::gpio::set_pins_analog(g::GPIOF_BASE, g::PIN_5 | g::PIN_6 | g::PIN_8);

    // Arm Exit column + Row 0 (PB15 pull-up) for wake.
    #[cfg(not(feature = "swd_debug"))]
    {
        KB_COLS[3].configure_output(g::SPEED_LOW);
        KB_COLS[3].set_low();
    }
    KB_ROWS[0].configure_input(g::PULL_UP);

    saved
}

/// Checks whether `KeyCode::Exit` (`KB_COLS[3]` -> `KB_ROWS[0]`) is held while parked for STOP.
pub fn is_exit_held_parked() -> bool {
    use py32_pac::gpio as g;

    #[cfg(feature = "swd_debug")]
    return false;

    #[cfg(not(feature = "swd_debug"))]
    {
        KB_COLS[3].configure_output(g::SPEED_LOW);
        KB_COLS[3].set_low();
        cortex_m::asm::delay(160);
        if KB_ROWS[0].is_high() {
            return false;
        }
        KB_COLS[3].configure_input(g::PULL_NONE);
        cortex_m::asm::delay(800);
        let released_high = KB_ROWS[0].is_high();
        KB_COLS[3].configure_output(g::SPEED_LOW);
        KB_COLS[3].set_low();
        cortex_m::asm::delay(32);
        released_high
    }
}

/// Restore GPIO `MODER`/`PUPDR` registers and keypad idle outputs after waking from `PWR Stop`.
pub fn restore_from_stop(saved: &[u32; 6]) {
    use py32_pac::gpio as g;
    cortex_m::interrupt::free(|_| unsafe {
        for (idx, &base) in STOP_PORTS.iter().enumerate() {
            ((base + g::MODER_OFFSET) as *mut u32).write_volatile(saved[idx * 2]);
            ((base + g::PUPDR_OFFSET) as *mut u32).write_volatile(saved[idx * 2 + 1]);
        }
    });
    for (idx, &col) in KB_COLS.iter().enumerate() {
        #[cfg(feature = "swd_debug")]
        if idx == 3 {
            continue;
        }
        let _ = idx;
        col.set_high();
    }
}
