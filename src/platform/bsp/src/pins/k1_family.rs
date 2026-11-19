use py32_hal::gpio::Pin;
use py32_pac::gpio;

// ── Target Feature Descriptor ───────────────────────────────────────────────
pub const HAS_FLASHLIGHT: bool = true;
pub const HAS_RGB_BACKLIGHT: bool = false;
pub const HAS_COPROC: bool = false;
pub const HAS_GNSS: bool = false;
pub const HAS_EXTRA_KEY_ROW: bool = false;

// ── LCD (ST7565 via HW SPI1) ────────────────────────────────────────────────
pub const LCD_A0: Pin = Pin::new(gpio::GPIOA_BASE, 6);
pub const LCD_CS: Pin = Pin::new(gpio::GPIOB_BASE, 2);

// ── BK4819/BK4829 RF Transceiver (bit-bang SPI) ─────────────────────────────
pub const BK_SCL: Pin = Pin::new(gpio::GPIOB_BASE, 8);
pub const BK_SDA: Pin = Pin::new(gpio::GPIOB_BASE, 9);
pub const BK_CSN: Pin = Pin::new(gpio::GPIOF_BASE, 9);

// ── Keyboard Matrix ─────────────────────────────────────────────────────────
pub const KB_COLS: [Pin; 4] = [
    Pin::new(gpio::GPIOB_BASE, 6),
    Pin::new(gpio::GPIOB_BASE, 5),
    Pin::new(gpio::GPIOB_BASE, 4),
    Pin::new(gpio::GPIOB_BASE, 3),
];
pub const KB_ROWS: [Pin; 4] = [
    Pin::new(gpio::GPIOB_BASE, 15),
    Pin::new(gpio::GPIOB_BASE, 14),
    Pin::new(gpio::GPIOB_BASE, 13),
    Pin::new(gpio::GPIOB_BASE, 12),
];
pub const KB_ROW5: Option<Pin> = None;

// ── Misc I/O ────────────────────────────────────────────────────────────────
pub const PTT: Pin = Pin::new(gpio::GPIOB_BASE, 10);
pub const AUDIO_PA: Pin = Pin::new(gpio::GPIOA_BASE, 8);

/// Voice DAC Output: GPIOA Pin 4 (`DAC1_OUT`), routed to LN4898 audio amp on UV-K1/K5v3.
pub const VOICE_DAC: Pin = Pin::new(gpio::GPIOA_BASE, 4);

/// Unified Backlight: Drives transistor Q29 (DTC114EE) on GPIOF Pin 8.
pub const BACKLIGHT: Pin = Pin::new(gpio::GPIOF_BASE, 8);
pub const FLASHLIGHT: Pin = Pin::new(gpio::GPIOC_BASE, 13);
pub const FLASH_CS: Pin = Pin::new(gpio::GPIOA_BASE, 3);

// ── BK1080 Broadcast FM Radio (I2C bit-bang) ────────────────────────────────
pub const FM_SCL: Pin = Pin::new(gpio::GPIOF_BASE, 5);
pub const FM_SDA: Pin = Pin::new(gpio::GPIOF_BASE, 6);

/// Initialize all GPIO pins for UV-K1 / UV-K5v3.
pub fn init() {
    use py32_pac as pac;

    // Pulse GPIOB + GPIOC port reset (RCC_IOPRSTR bits 1 & 2) so any LCKR pin locks applied
    // during a software DFU handoff (side keys, PTT, PC13 flashlight) clear on app restart.
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        let ioprstr = (pac::rcc::RCC_BASE + 0x24) as *mut u32;
        ioprstr.write_volatile((1 << 1) | (1 << 2));
        ioprstr.write_volatile(0);
    }

    const CFG_IN_PU: u8 = 1 << 2;
    const CFG_OUT_L: u8 = pac::gpio::SPEED_LOW as u8;
    const CFG_OUT_L_LO: u8 = CFG_OUT_L | (1 << 5);
    const CFG_OUT_L_HI: u8 = CFG_OUT_L | (1 << 4);
    const CFG_OUT_M_HI: u8 = (pac::gpio::SPEED_MEDIUM as u8) | (1 << 4);
    const CFG_OUT_H_HI: u8 = (pac::gpio::SPEED_HIGH as u8) | (1 << 4);
    const CFG_OD_L_HI: u8 = CFG_OUT_L | (1 << 3) | (1 << 4);

    static INIT_PINS: [(Pin, u8); 20] = [
        (KB_ROWS[0], CFG_IN_PU),
        (KB_ROWS[1], CFG_IN_PU),
        (KB_ROWS[2], CFG_IN_PU),
        (KB_ROWS[3], CFG_IN_PU),
        (PTT, CFG_IN_PU),
        (KB_COLS[0], CFG_OUT_H_HI),
        (KB_COLS[1], CFG_OUT_H_HI),
        (KB_COLS[2], CFG_OUT_H_HI),
        (KB_COLS[3], CFG_OUT_H_HI),
        (AUDIO_PA, CFG_OUT_L),
        (FLASHLIGHT, CFG_OUT_L),
        (BACKLIGHT, CFG_OUT_L_LO),
        (FM_SCL, CFG_OD_L_HI),
        (FM_SDA, CFG_OD_L_HI),
        (LCD_A0, CFG_OUT_L_HI),
        (LCD_CS, CFG_OUT_L_HI),
        (FLASH_CS, CFG_OUT_L_HI),
        (BK_SDA, CFG_OUT_L_HI),
        (BK_CSN, CFG_OUT_L_HI),
        (BK_SCL, CFG_OUT_M_HI),
    ];

    for &(pin, cfg) in &INIT_PINS {
        if (cfg & CFG_IN_PU) != 0 {
            pin.configure_input(pac::gpio::PULL_UP);
        } else {
            let speed = (cfg & 0x03) as u32;
            if (cfg & (1 << 3)) != 0 {
                pin.configure_output_od(speed);
            } else {
                pin.configure_output(speed);
            }
            if (cfg & (1 << 4)) != 0 {
                pin.set_high();
            } else if (cfg & (1 << 5)) != 0 {
                pin.set_low();
            }
        }
    }

    // ── Unused & ADC pins → analog (minimize leakage & electromagnetic pickup) ───
    const SWD_MASK: u16 = if cfg!(feature = "swd_debug") {
        0
    } else {
        (pac::gpio::PIN_13 | pac::gpio::PIN_14) as u16
    };
    static ANALOG_PORTS: [(u8, u16); 4] = [
        (
            0,
            (pac::gpio::PIN_0
                | pac::gpio::PIN_1
                | pac::gpio::PIN_2
                | pac::gpio::PIN_4
                | pac::gpio::PIN_5
                | pac::gpio::PIN_7
                | pac::gpio::PIN_15) as u16
                | SWD_MASK,
        ),
        (
            1,
            (pac::gpio::PIN_0 | pac::gpio::PIN_1 | pac::gpio::PIN_7 | pac::gpio::PIN_11) as u16,
        ),
        (
            2,
            (pac::gpio::PIN_0
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
                | pac::gpio::PIN_15) as u16,
        ),
        (
            5,
            (pac::gpio::PIN_0
                | pac::gpio::PIN_1
                | pac::gpio::PIN_2
                | pac::gpio::PIN_3
                | pac::gpio::PIN_4
                | pac::gpio::PIN_7) as u16,
        ),
    ];
    for &(port_idx, mask) in &ANALOG_PORTS {
        py32_hal::gpio::set_pins_analog(
            pac::gpio::GPIOA_BASE + ((port_idx as u32) << 10),
            mask as u32,
        );
    }
}

/// Disables SWD debugging interface on PA13 (SWDIO) and PA14 (SWCLK).
/// Already folded into `init()`'s `GPIOA` analog mask on the K1 family.
#[inline(always)]
pub fn disable_swd() {}

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

    FLASHLIGHT.set_low();
    AUDIO_PA.set_low();
    BACKLIGHT.set_low();
    FLASH_CS.set_high();
    LCD_CS.set_high();
    BK_CSN.set_high();

    let mut gpioa_analog =
        g::PIN_0 | g::PIN_1 | g::PIN_2 | g::PIN_5 | g::PIN_6 | g::PIN_7 | g::PIN_9;
    if !uart_rx_wake {
        gpioa_analog |= g::PIN_10;
    } else {
        let rx = Pin::new(g::GPIOA_BASE, 10);
        rx.configure_input(g::PULL_UP);
    }
    py32_hal::gpio::set_pins_analog(g::GPIOA_BASE, gpioa_analog);

    py32_hal::gpio::set_pins_analog(
        g::GPIOB_BASE,
        g::PIN_4
            | g::PIN_5
            | g::PIN_6
            | g::PIN_8
            | g::PIN_9
            | g::PIN_10
            | g::PIN_12
            | g::PIN_13
            | g::PIN_14,
    );

    py32_hal::gpio::set_pins_analog(g::GPIOF_BASE, g::PIN_5 | g::PIN_6);

    // Arm ONLY Exit key column (PB3 low) and Row 0 (PB15 pull-up) for EXTI15 falling-edge wake.
    KB_COLS[3].configure_output(g::SPEED_LOW);
    KB_COLS[3].set_low();
    KB_ROWS[0].configure_input(g::PULL_UP);

    saved
}

/// Checks whether `KeyCode::Exit` (`KB_COLS[3]` -> `KB_ROWS[0]`) is held while parked for STOP.
pub fn is_exit_held_parked() -> bool {
    use py32_pac::gpio as g;

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

/// Restore GPIO `MODER`/`PUPDR` registers and keypad idle outputs after waking from `PWR Stop`.
pub fn restore_from_stop(saved: &[u32; 6]) {
    use py32_pac::gpio as g;
    cortex_m::interrupt::free(|_| unsafe {
        for (idx, &base) in STOP_PORTS.iter().enumerate() {
            ((base + g::MODER_OFFSET) as *mut u32).write_volatile(saved[idx * 2]);
            ((base + g::PUPDR_OFFSET) as *mut u32).write_volatile(saved[idx * 2 + 1]);
        }
    });
    for col in &KB_COLS {
        col.set_high();
    }
}
