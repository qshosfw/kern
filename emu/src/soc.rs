//! PY32F071xB System-on-Chip memory map & peripheral bus.

use std::collections::VecDeque;
use std::path::Path;

use crate::board::{Bk4829Chip, FlashInitMode, KeypadMatrix, Py25q16Flash, St7565Panel};

pub const FLASH_BASE: u32 = 0x0800_0000;
pub const FLASH_LEN: usize = 128 * 1024;
pub const APP_FLASH_OFFSET: usize = 0x2800; // 0x0800_2800
pub const SRAM_BASE: u32 = 0x2000_0000;
pub const SRAM_LEN: usize = 16 * 1024;

#[derive(Clone, Copy, Default)]
pub struct GpioPort {
    pub moder: u32,
    pub otyper: u32,
    pub ospeedr: u32,
    pub pupdr: u32,
    pub odr: u32,
    pub afrl: u32,
    pub afrh: u32,
}

impl GpioPort {
    pub fn read(&self, off: u32, idr: u32) -> u32 {
        match off {
            0x00 => self.moder,
            0x04 => self.otyper,
            0x08 => self.ospeedr,
            0x0C => self.pupdr,
            0x10 => idr,
            0x14 => self.odr & 0xFFFF,
            0x20 => self.afrl,
            0x24 => self.afrh,
            _ => 0,
        }
    }

    pub fn write(&mut self, off: u32, val: u32) -> Option<(u32, u32)> {
        match off {
            0x00 => {
                self.moder = val;
                None
            }
            0x04 => {
                self.otyper = val & 0xFFFF;
                None
            }
            0x08 => {
                self.ospeedr = val;
                None
            }
            0x0C => {
                self.pupdr = val;
                None
            }
            0x14 => {
                let prev = self.odr;
                self.odr = val & 0xFFFF;
                Some((prev, self.odr))
            }
            0x18 => {
                let prev = self.odr;
                let set = val & 0xFFFF;
                let rst = (val >> 16) & 0xFFFF;
                self.odr = ((self.odr | set) & !rst) & 0xFFFF;
                Some((prev, self.odr))
            }
            0x20 => {
                self.afrl = val;
                None
            }
            0x24 => {
                self.afrh = val;
                None
            }
            0x28 => {
                let prev = self.odr;
                self.odr &= !(val & 0xFFFF);
                Some((prev, self.odr))
            }
            _ => None,
        }
    }

    pub fn pin_out_high(&self, pin: u8) -> bool {
        ((self.odr >> pin) & 1) != 0
    }

    pub fn base_idr(&self) -> u32 {
        // Output pins reflect ODR; input pins with pull-up read 1, pull-down read 0
        let mut idr = 0u32;
        for p in 0..16u32 {
            let mode = (self.moder >> (p * 2)) & 0b11;
            let pull = (self.pupdr >> (p * 2)) & 0b11;
            let high = match mode {
                0b01 | 0b10 => ((self.odr >> p) & 1) != 0,
                0b00 => pull != 0b10, // default high unless pull-down
                _ => false,
            };
            if high {
                idr |= 1 << p;
            }
        }
        idr
    }
}

pub struct Py32Soc {
    pub flash: Vec<u8>,
    pub sram: [u8; SRAM_LEN],
    pub uid_rom: [u8; 0x400],

    // Cortex-M0+ System Control Space
    pub syst_csr: u32,
    pub syst_rvr: u32,
    pub syst_cvr: u32,
    pub nvic_iser: u32,
    pub nvic_ispr: u32,
    pub nvic_ipr: [u8; 32],
    pub scb_vtor: u32,
    pub scb_scr: u32,
    pub scb_shpr2: u32,
    pub scb_shpr3: u32,
    pub pendsv_pending: bool,
    pub systick_pending: bool,
    pub warm_reset_requested: bool,

    // Clocks & timing
    pub virtual_cycles: u64,
    pub virtual_us_rem_cycles: u32,
    pub virtual_us: u64,
    pub sysclk_hz: u32,
    pub rcc_cr: u32,
    pub rcc_icscr: u32,
    pub rcc_cfgr: u32,
    pub rcc_pllcfgr: u32,
    pub rcc_iopenr: u32,
    pub rcc_ahbenr: u32,
    pub rcc_apb1enr: u32,
    pub rcc_apb2enr: u32,
    pub rcc_ccipr: u32,
    pub rcc_bdcr: u32,
    pub rcc_csr: u32,
    pub flash_acr: u32,

    // TIM2
    pub tim2_cr1: u32,
    pub tim2_psc: u32,
    pub tim2_arr: u32,
    pub tim2_cnt_base_us: u64,
    pub tim2_cnt_offset: u32,

    // CRC32 (MPEG-2 0x04C11DB7)
    pub crc_dr: u32,
    pub crc_idr: u8,

    // ADC1 & Battery simulation
    pub adc_sr: u32,
    pub adc_cr1: u32,
    pub adc_cr2: u32,
    pub adc_sqr3: u32,
    pub adc_dr: u32,
    pub adc_ltr: u32,
    pub adc_htr: u32,
    pub battery_adc_raw: u16, // ~2227 => 7.60V (~82%)

    // GPIO ports A(0), B(1), C(2), D(3), F(5)
    pub gpio: [GpioPort; 6],
    pub exti_imr: u32,
    pub exti_rtsr: u32,
    pub exti_ftsr: u32,
    pub exti_pr: u32,
    pub syscfg_regs: [u32; 16],

    // SPI1 / SPI2
    pub spi1_cr1: u32,
    pub spi1_rx: u8,
    pub spi1_rxne: bool,
    pub spi2_cr1: u32,
    pub spi2_rx: u8,
    pub spi2_rxne: bool,

    // USART1 (RigWire + boot serial log)
    pub usart1_cr1: u32,
    pub usart1_brr: u32,
    pub usart1_rx_dr: Option<u8>,
    pub uart_rx_fifo: VecDeque<u8>,
    pub uart_tx_fifo: VecDeque<u8>,
    pub serial_line_buf: String,
    pub serial_lines: VecDeque<String>,

    // Backup / RTC / PWR / LPTIM / USBD scratch
    pub rtc_bkp: [u32; 32],
    pub pwr_cr1: u32,
    pub pwr_csr: u32,
    pub lptim_isr: u32,
    pub lptim_ier: u32,
    pub lptim_cr: u32,
    pub lptim_arr: u32,
    pub usbd_regs: [u8; 0x800],

    // Board peripherals
    pub lcd: St7565Panel,
    pub spiflash: Py25q16Flash,
    pub bk4829: Bk4829Chip,
    pub keypad: KeypadMatrix,
    pub boot_vector_base: u32,
}

impl Py32Soc {
    pub fn new(
        firmware_image: &[u8],
        flash_path: Option<&Path>,
        ephemeral_flash: bool,
        flash_mode: FlashInitMode,
    ) -> std::io::Result<Self> {
        let mut flash = vec![0xFFu8; FLASH_LEN];
        populate_bootloader_stub(&mut flash);
        let boot_vector_base = load_firmware_into_flash(&mut flash, firmware_image)?;

        let mut uid_rom = [0xFFu8; 0x400];
        // Deterministic 96-bit device UID ("KERN-EMU-001") at 0x1FFF_3000
        uid_rom[0..12].copy_from_slice(b"KERNEMU00001");
        // FLASH_SIZE_REG at 0x1FFF_31FC = 128 KB
        uid_rom[0x1FC..0x1FE].copy_from_slice(&128u16.to_le_bytes());
        // HSI calibration trims at 0x1FFF_3210 (16 MHz) & 0x1FFF_3220 (24 MHz)
        uid_rom[0x210..0x214].copy_from_slice(&0x0000_1000u32.to_le_bytes());
        uid_rom[0x220..0x224].copy_from_slice(&0x0000_1000u32.to_le_bytes());
        // Temperature sensor calibration @ 30C (0x1FFF_3228) & 105C (0x1FFF_3230)
        uid_rom[0x228..0x22A].copy_from_slice(&910u16.to_le_bytes());
        uid_rom[0x230..0x232].copy_from_slice(&1160u16.to_le_bytes());

        let spiflash = Py25q16Flash::new(flash_path, ephemeral_flash, flash_mode)?;

        let mut soc = Self {
            flash,
            sram: [0u8; SRAM_LEN],
            uid_rom,
            syst_csr: 0,
            syst_rvr: 47_999,
            syst_cvr: 47_999,
            nvic_iser: 0,
            nvic_ispr: 0,
            nvic_ipr: [0; 32],
            scb_vtor: boot_vector_base,
            scb_scr: 0,
            scb_shpr2: 0,
            scb_shpr3: 0,
            pendsv_pending: false,
            systick_pending: false,
            warm_reset_requested: false,
            virtual_cycles: 0,
            virtual_us_rem_cycles: 0,
            virtual_us: 0,
            sysclk_hz: 48_000_000,
            rcc_cr: (1 << 8) | (1 << 10) | (1 << 24) | (1 << 25),
            rcc_icscr: 2 << 13, // 16 MHz HSI base -> PLL x3 = 48 MHz
            rcc_cfgr: 0x0000_0012,
            rcc_pllcfgr: 0x0000_0006,
            rcc_iopenr: 0,
            rcc_ahbenr: 1 << 8,
            rcc_apb1enr: 0,
            rcc_apb2enr: 0,
            rcc_ccipr: 0,
            rcc_bdcr: 0,
            rcc_csr: 1 << 27, // PWRRSTF on cold boot
            flash_acr: 1,
            tim2_cr1: 0,
            tim2_psc: 47,
            tim2_arr: 0xFFFF_FFFF,
            tim2_cnt_base_us: 0,
            tim2_cnt_offset: 0,
            crc_dr: 0xFFFF_FFFF,
            crc_idr: 0,
            adc_sr: 0,
            adc_cr1: 0,
            adc_cr2: 0,
            adc_sqr3: 8,
            adc_dr: 2227,
            adc_ltr: 0,
            adc_htr: 0x0FFF,
            battery_adc_raw: 2227,
            gpio: [GpioPort::default(); 6],
            exti_imr: 0,
            exti_rtsr: 0,
            exti_ftsr: 0,
            exti_pr: 0,
            syscfg_regs: [0; 16],
            spi1_cr1: 0,
            spi1_rx: 0,
            spi1_rxne: false,
            spi2_cr1: 0,
            spi2_rx: 0,
            spi2_rxne: false,
            usart1_cr1: 0,
            usart1_brr: 417,
            usart1_rx_dr: None,
            uart_rx_fifo: VecDeque::with_capacity(4096),
            uart_tx_fifo: VecDeque::with_capacity(4096),
            serial_line_buf: String::new(),
            serial_lines: VecDeque::with_capacity(64),
            rtc_bkp: [0; 32],
            pwr_cr1: 0,
            pwr_csr: 0,
            lptim_isr: 0,
            lptim_ier: 0,
            lptim_cr: 0,
            lptim_arr: 0,
            usbd_regs: [0; 0x800],
            lcd: St7565Panel::new(),
            spiflash,
            bk4829: Bk4829Chip::new(),
            keypad: KeypadMatrix::new(),
            boot_vector_base,
        };
        soc.rtc_bkp[1] = 1 << 5; // RTC CRL_RTOFF ready
        Ok(soc)
    }

    /// Warm reset (`SYSRESETREQ`): preserves `SRAM` (for `.uninit` crash BootRecord) and `rtc_bkp`!
    pub fn warm_reset_peripherals(&mut self) {
        self.syst_csr = 0;
        self.syst_rvr = 47_999;
        self.syst_cvr = 47_999;
        self.nvic_iser = 0;
        self.nvic_ispr = 0;
        self.pendsv_pending = false;
        self.systick_pending = false;
        self.warm_reset_requested = false;
        self.scb_vtor = self.boot_vector_base;
        self.rcc_csr = 1 << 28; // SFTRSTF
        self.usart1_rx_dr = None;
    }

    pub fn set_battery_mv(&mut self, mv: u16) {
        // Linear map around authentic 7600 mV = 2227 ADC counts (~0.293 counts/mV)
        let clamped = mv.clamp(5800, 8500) as u32;
        self.battery_adc_raw = ((clamped * 2227) / 7600).min(4095) as u16;
    }

    pub fn backlight_on(&self) -> bool {
        self.gpio[5].pin_out_high(8) // PF8
    }

    pub fn flashlight_on(&self) -> bool {
        self.gpio[2].pin_out_high(13) // PC13
    }

    pub fn audio_pa_on(&self) -> bool {
        self.gpio[0].pin_out_high(8) // PA8
    }

    #[inline]
    pub fn step_cycles(&mut self, cycles: u32) {
        self.virtual_cycles = self.virtual_cycles.wrapping_add(cycles as u64);

        // Advance microsecond clock from sysclk
        let c_per_us = (self.sysclk_hz / 1_000_000).max(1);
        self.virtual_us_rem_cycles += cycles;
        if self.virtual_us_rem_cycles >= c_per_us {
            let add_us = self.virtual_us_rem_cycles / c_per_us;
            self.virtual_us_rem_cycles %= c_per_us;
            self.virtual_us = self.virtual_us.wrapping_add(add_us as u64);
        }

        // Advance SysTick when enabled (bit 0)
        if (self.syst_csr & 1) != 0 {
            let rvr = (self.syst_rvr & 0x00FF_FFFF).max(1);
            if self.syst_cvr > cycles {
                self.syst_cvr -= cycles;
            } else {
                let rem = cycles - self.syst_cvr;
                self.syst_cvr = rvr - (rem % (rvr + 1));
                self.syst_csr |= 1 << 16; // COUNTFLAG
                if (self.syst_csr & 2) != 0 {
                    self.systick_pending = true;
                }
            }
        }

        // Pump next incoming RigWire UART byte into USART1 RX DR once previous byte is read
        if self.usart1_rx_dr.is_none()
            && (self.usart1_cr1 & (1 << 2)) != 0 // RE enabled
            && let Some(b) = self.uart_rx_fifo.pop_front()
        {
            self.usart1_rx_dr = Some(b);
            if (self.usart1_cr1 & (1 << 5)) != 0 {
                // RXNEIE -> IRQ 27 (USART1)
                self.nvic_ispr |= 1 << 27;
            }
        }
    }

    /// Fast-forward idle `WFI` up to the next SysTick boundary (instant sleep wakeup / zero busy-spin).
    pub fn fast_forward_wfi(&mut self) {
        if !self.uart_rx_fifo.is_empty() || self.usart1_rx_dr.is_some() {
            self.step_cycles(8);
            return;
        }
        // If deep sleep armed EXTI15 (PB15 Exit key) and Exit or Side1 is pressed, fire EXTI4_15 (IRQ 7)!
        if (self.exti_imr & (1 << 15)) != 0 && (self.keypad.held_mask & ((1 << 14) | (1 << 0))) != 0 {
            self.exti_pr |= 1 << 15;
            self.nvic_ispr |= 1 << 7;
            return;
        }
        if (self.syst_csr & 1) != 0 && self.syst_cvr > 0 {
            let jump = self.syst_cvr;
            self.step_cycles(jump);
        } else if (self.lptim_cr & 1) != 0 {
            // LPTIM1 wakeup in deep sleep STOP mode
            self.virtual_us = self.virtual_us.wrapping_add(50_000);
            self.lptim_isr |= 1 << 1; // ARRM
            self.exti_pr |= 1 << 29;
            self.nvic_ispr |= 1 << 17; // TIM6_LPTIM1_DAC
        } else {
            self.step_cycles(64);
        }
    }

    fn resync_sysclk(&mut self) {
        const HSI_KHZ: [u32; 5] = [4_000, 8_000, 16_000, 22_120, 24_000];
        let hsifs = ((self.rcc_icscr >> 13) & 0x07) as usize;
        let hsi = HSI_KHZ.get(hsifs).copied().unwrap_or(16_000) * 1_000;
        let sws = self.rcc_cfgr & 0x38;
        let base = match sws {
            0x10 => {
                let mul = if ((self.rcc_pllcfgr >> 2) & 0x03) == 1 { 3 } else { 2 };
                mul * if hsi >= 16_000_000 { hsi } else { 24_000_000 }
            }
            0x08 => 24_000_000,
            _ => {
                let div = (self.rcc_cr >> 11) & 0x07;
                hsi >> div
            }
        };
        let hpre = (self.rcc_cfgr >> 4) & 0x0F;
        let shift = if hpre >= 8 { (hpre - 7).min(9) } else { 0 };
        self.sysclk_hz = (base >> shift).max(1_000_000);
    }

    fn sync_gpio_side_effects(&mut self) {
        // PA3 -> SPI Flash CS
        let flash_cs_low = !self.gpio[0].pin_out_high(3);
        self.spiflash.set_cs(flash_cs_low);

        // BK4829 3-wire: PF9 = CSN, PB8 = SCL, PB9 = SDA
        let csn = self.gpio[5].pin_out_high(9);
        let scl = self.gpio[1].pin_out_high(8);
        let sda = self.gpio[1].pin_out_high(9);
        self.bk4829.on_pins(csn, scl, sda);
    }

    fn read_tim2_cnt(&self) -> u32 {
        if (self.tim2_cr1 & 1) == 0 {
            return self.tim2_cnt_offset;
        }
        let elapsed = self.virtual_us.saturating_sub(self.tim2_cnt_base_us) as u32;
        self.tim2_cnt_offset.wrapping_add(elapsed)
    }

    #[inline]
    pub fn read_u8(&mut self, addr: u32) -> u8 {
        match addr {
            0x0000_0000..=0x0001_FFFF => self.flash[addr as usize],
            0x0800_0000..=0x0801_FFFF => self.flash[(addr - FLASH_BASE) as usize],
            0x2000_0000..=0x2000_3FFF => self.sram[(addr - SRAM_BASE) as usize],
            0x1FFF_0F04..=0x1FFF_0F05 => {
                let v = 1489u16.to_le_bytes();
                v[(addr & 1) as usize]
            }
            0x1FFF_3000..=0x1FFF_33FF => self.uid_rom[(addr - 0x1FFF_3000) as usize],
            0x4001_300C => {
                self.spi1_rxne = false;
                self.spi1_rx
            }
            0x4000_380C => {
                self.spi2_rxne = false;
                self.spi2_rx
            }
            0x4002_3004 => self.crc_idr,
            0x4000_5C00..=0x4000_63FF => self.usbd_regs[(addr - 0x4000_5C00) as usize],
            0xE000_E400..=0xE000_E41F => self.nvic_ipr[(addr - 0xE000_E400) as usize],
            _ => {
                let w = self.read_u32(addr & !3);
                ((w >> ((addr & 3) * 8)) & 0xFF) as u8
            }
        }
    }

    #[inline]
    pub fn read_u16(&mut self, addr: u32) -> u16 {
        match addr {
            0x0000_0000..=0x0001_FFFE => {
                let i = addr as usize;
                u16::from_le_bytes([self.flash[i], self.flash[i + 1]])
            }
            0x0800_0000..=0x0801_FFFE => {
                let i = (addr - FLASH_BASE) as usize;
                u16::from_le_bytes([self.flash[i], self.flash[i + 1]])
            }
            0x2000_0000..=0x2000_3FFE => {
                let i = (addr - SRAM_BASE) as usize;
                u16::from_le_bytes([self.sram[i], self.sram[i + 1]])
            }
            _ => {
                let b0 = self.read_u8(addr);
                let b1 = self.read_u8(addr.wrapping_add(1));
                u16::from_le_bytes([b0, b1])
            }
        }
    }

    pub fn read_u32(&mut self, addr: u32) -> u32 {
        match addr {
            0x0000_0000..=0x0001_FFFC => {
                let i = addr as usize;
                u32::from_le_bytes(self.flash[i..i + 4].try_into().unwrap())
            }
            0x0800_0000..=0x0801_FFFC => {
                let i = (addr - FLASH_BASE) as usize;
                u32::from_le_bytes(self.flash[i..i + 4].try_into().unwrap())
            }
            0x2000_0000..=0x2000_3FFC => {
                let i = (addr - SRAM_BASE) as usize;
                u32::from_le_bytes(self.sram[i..i + 4].try_into().unwrap())
            }
            0x1FFF_3000..=0x1FFF_33FC => {
                let i = (addr - 0x1FFF_3000) as usize;
                u32::from_le_bytes(self.uid_rom[i..i + 4].try_into().unwrap())
            }
            // TIM2
            0x4000_0000 => self.tim2_cr1,
            0x4000_0024 => self.read_tim2_cnt(),
            0x4000_0028 => self.tim2_psc,
            0x4000_002C => self.tim2_arr,
            // RTC / Backup domain (0x4000_2800..0x4000_287C)
            0x4000_2800..=0x4000_287C => {
                let idx = ((addr - 0x4000_2800) >> 2) as usize;
                self.rtc_bkp.get(idx).copied().unwrap_or(0)
            }
            // IWDG (SR always 0 = not busy updating)
            0x4000_3000..=0x4000_3010 => 0,
            // SPI2
            0x4000_3800 => self.spi2_cr1,
            0x4000_3808 => (1 << 1) | u32::from(self.spi2_rxne), // SR_TXE | SR_RXNE
            0x4000_380C => {
                self.spi2_rxne = false;
                self.spi2_rx as u32
            }
            // PWR
            0x4000_7000 => self.pwr_cr1,
            0x4000_7004 => self.pwr_csr, // PVDO bit 2 = 0 (VDD healthy > 2.8V)
            // LPTIM1
            0x4000_7C00 => self.lptim_isr | (1 << 4), // ARROK ready
            0x4000_7C08 => self.lptim_ier,
            0x4000_7C10 => self.lptim_cr,
            0x4000_7C18 => self.lptim_arr,
            // SYSCFG
            0x4001_0000..=0x4001_003C => self.syscfg_regs[((addr - 0x4001_0000) >> 2) as usize],
            // ADC1
            0x4001_2400 => self.adc_sr,
            0x4001_2404 => self.adc_cr1,
            0x4001_2408 => self.adc_cr2,
            0x4001_2424 => self.adc_htr,
            0x4001_2428 => self.adc_ltr,
            0x4001_2438 => self.adc_sqr3,
            0x4001_2440 => self.adc_dr,
            0x4001_2450 => {
                self.adc_sr &= !(1 << 1); // reading DR clears EOC
                self.adc_dr
            }
            // SPI1
            0x4001_3000 => self.spi1_cr1,
            0x4001_3008 => (1 << 1) | u32::from(self.spi1_rxne),
            0x4001_300C => {
                self.spi1_rxne = false;
                self.spi1_rx as u32
            }
            // USART1
            0x4001_3800 => {
                let mut sr = (1 << 7) | (1 << 6); // TXE | TC
                if self.usart1_rx_dr.is_some() {
                    sr |= 1 << 5; // RXNE
                }
                sr
            }
            0x4001_3804 => self.usart1_rx_dr.take().unwrap_or(0) as u32,
            0x4001_3808 => self.usart1_brr,
            0x4001_380C => self.usart1_cr1,
            // USART3 (K10Pro stub)
            0x4000_4800 => (1 << 7) | (1 << 6),
            // RCC
            0x4002_1000 => self.rcc_cr,
            0x4002_1004 => self.rcc_icscr,
            0x4002_1008 => self.rcc_cfgr,
            0x4002_100C => self.rcc_pllcfgr,
            0x4002_1034 => self.rcc_iopenr,
            0x4002_1038 => self.rcc_ahbenr,
            0x4002_103C => self.rcc_apb1enr,
            0x4002_1040 => self.rcc_apb2enr,
            0x4002_1054 => self.rcc_ccipr,
            0x4002_105C => self.rcc_bdcr,
            0x4002_1060 => self.rcc_csr,
            // EXTI
            0x4002_1800 => self.exti_rtsr,
            0x4002_1804 => self.exti_ftsr,
            0x4002_180C => self.exti_pr,
            0x4002_1880 => self.exti_imr,
            // FLASH controller (SR bit 16 BSY = 0)
            0x4002_2000 => self.flash_acr,
            0x4002_2010 => 0,
            // CRC32
            0x4002_3000 => self.crc_dr,
            0x4002_3004 => self.crc_idr as u32,
            // GPIO Ports (0x5000_0000 .. 0x5000_17FF)
            0x5000_0000..=0x5000_17FF => {
                let port_idx = ((addr - 0x5000_0000) >> 10) as usize;
                let off = addr & 0x3FF;
                if port_idx >= self.gpio.len() {
                    return 0;
                }
                let p = &self.gpio[port_idx];
                let mut idr = p.base_idr();
                if port_idx == 1 {
                    // GPIOB: overlay keypad rows/PTT and BK4829 SDA (PB9) when input
                    idr = self.keypad.overlay_gpiob_idr(idr, p.moder, p.odr);
                    if ((p.moder >> 18) & 0b11) == 0b00 {
                        if self.bk4829.sda_level() {
                            idr |= 1 << 9;
                        } else {
                            idr &= !(1 << 9);
                        }
                    }
                }
                p.read(off, idr)
            }
            // Cortex-M0+ System Control Space
            0xE000_E010 => {
                let v = self.syst_csr;
                self.syst_csr &= !(1 << 16); // reading CSR clears COUNTFLAG
                v
            }
            0xE000_E014 => self.syst_rvr,
            0xE000_E018 => {
                // Guarantee forward progress on tight polling loops (`delay_us`)
                self.step_cycles(4);
                self.syst_cvr & 0x00FF_FFFF
            }
            0xE000_E100 | 0xE000_E180 => self.nvic_iser,
            0xE000_E200 | 0xE000_E280 => self.nvic_ispr,
            0xE000_E400..=0xE000_E41C => {
                let i = (addr - 0xE000_E400) as usize;
                u32::from_le_bytes(self.nvic_ipr[i..i + 4].try_into().unwrap())
            }
            0xE000_ED00 => 0x410C_C601, // Cortex-M0+ r0p1 CPUID
            0xE000_ED04 => {
                let mut icsr = 0u32;
                if self.pendsv_pending {
                    icsr |= 1 << 28;
                }
                if self.systick_pending {
                    icsr |= 1 << 26;
                }
                icsr
            }
            0xE000_ED08 => self.scb_vtor,
            0xE000_ED0C => 0xFA05_0000,
            0xE000_ED10 => self.scb_scr,
            0xE000_ED1C => self.scb_shpr2,
            0xE000_ED20 => self.scb_shpr3,
            0xE000_EDF0 => 0, // DHCSR: no debugger attached
            _ => 0,
        }
    }

    #[inline]
    pub fn write_u8(&mut self, addr: u32, val: u8) {
        match addr {
            0x2000_0000..=0x2000_3FFF => self.sram[(addr - SRAM_BASE) as usize] = val,
            0x4001_300C => {
                // SPI1 8-bit TX -> ST7565 LCD when PB2 (LCD_CS) is LOW
                if !self.gpio[1].pin_out_high(2) {
                    let dc_high = self.gpio[0].pin_out_high(6);
                    self.lcd.spi_byte(dc_high, val);
                }
                self.spi1_rx = 0xFF;
                self.spi1_rxne = true;
            }
            0x4000_380C => {
                // SPI2 8-bit TX -> PY25Q16 Flash
                self.spi2_rx = self.spiflash.transfer(val);
                self.spi2_rxne = true;
            }
            0x4002_3004 => self.crc_idr = val,
            0x4000_5C00..=0x4000_63FF => self.usbd_regs[(addr - 0x4000_5C00) as usize] = val,
            0xE000_E400..=0xE000_E41F => self.nvic_ipr[(addr - 0xE000_E400) as usize] = val,
            _ => {
                let shift = (addr & 3) * 8;
                let cur = self.read_u32(addr & !3);
                let next = (cur & !(0xFF << shift)) | ((val as u32) << shift);
                self.write_u32(addr & !3, next);
            }
        }
    }

    #[inline]
    pub fn write_u16(&mut self, addr: u32, val: u16) {
        if (0x2000_0000..=0x2000_3FFE).contains(&addr) {
            let i = (addr - SRAM_BASE) as usize;
            let b = val.to_le_bytes();
            self.sram[i] = b[0];
            self.sram[i + 1] = b[1];
            return;
        }
        let b = val.to_le_bytes();
        self.write_u8(addr, b[0]);
        self.write_u8(addr.wrapping_add(1), b[1]);
    }

    pub fn write_u32(&mut self, addr: u32, val: u32) {
        match addr {
            0x2000_0000..=0x2000_3FFC => {
                let i = (addr - SRAM_BASE) as usize;
                self.sram[i..i + 4].copy_from_slice(&val.to_le_bytes());
            }
            // TIM2
            0x4000_0000 => {
                let was_en = (self.tim2_cr1 & 1) != 0;
                let now_en = (val & 1) != 0;
                if was_en && !now_en {
                    self.tim2_cnt_offset = self.read_tim2_cnt();
                } else if !was_en && now_en {
                    self.tim2_cnt_base_us = self.virtual_us;
                }
                self.tim2_cr1 = val;
            }
            0x4000_0014 => {
                if (val & 1) != 0 {
                    // EGR_UG resets counter
                    self.tim2_cnt_offset = 0;
                    self.tim2_cnt_base_us = self.virtual_us;
                }
            }
            0x4000_0024 => {
                self.tim2_cnt_offset = val;
                self.tim2_cnt_base_us = self.virtual_us;
            }
            0x4000_0028 => self.tim2_psc = val,
            0x4000_002C => self.tim2_arr = val,
            // RTC / Backup domain
            0x4000_2800..=0x4000_287C => {
                let idx = ((addr - 0x4000_2800) >> 2) as usize;
                if idx < self.rtc_bkp.len() {
                    if idx == 1 {
                        self.rtc_bkp[1] = (val | (1 << 5) | (1 << 3)) & !0; // RTOFF | RSF always ready
                    } else {
                        self.rtc_bkp[idx] = val;
                    }
                }
            }
            // SPI2
            0x4000_3800 => self.spi2_cr1 = val,
            0x4000_380C => {
                self.spi2_rx = self.spiflash.transfer((val & 0xFF) as u8);
                self.spi2_rxne = true;
            }
            // PWR
            0x4000_7000 => self.pwr_cr1 = val,
            // LPTIM1
            0x4000_7C04 => self.lptim_isr &= !val, // ICR
            0x4000_7C08 => self.lptim_ier = val,
            0x4000_7C10 => self.lptim_cr = val,
            0x4000_7C18 => self.lptim_arr = val,
            // SYSCFG
            0x4001_0000..=0x4001_003C => {
                self.syscfg_regs[((addr - 0x4001_0000) >> 2) as usize] = val;
            }
            // ADC1
            0x4001_2400 => self.adc_sr &= val, // rc_w0
            0x4001_2404 => self.adc_cr1 = val,
            0x4001_2408 => {
                // Clear CAL (bit 31) & SWSTART (bit 22) immediately and latch sample into DR + EOC
                self.adc_cr2 = val & !((1 << 31) | (1 << 22));
                if (val & (1 << 22)) != 0 {
                    let ch = self.adc_sqr3 & 0x1F;
                    self.adc_dr = match ch {
                        8 => self.battery_adc_raw as u32,
                        16 => 900,  // ~28°C
                        17 => 1489, // 1.20V @ 3.30V VDDA
                        4 => 2048,  // mid-scale audio
                        _ => 2048,
                    };
                    self.adc_sr |= 1 << 1; // SR_EOC
                }
            }
            0x4001_2424 => self.adc_htr = val & 0x0FFF,
            0x4001_2428 => self.adc_ltr = val & 0x0FFF,
            0x4001_2438 => self.adc_sqr3 = val & 0x1F,
            // SPI1
            0x4001_3000 => self.spi1_cr1 = val,
            0x4001_300C => {
                let b = (val & 0xFF) as u8;
                if !self.gpio[1].pin_out_high(2) {
                    let dc_high = self.gpio[0].pin_out_high(6);
                    self.lcd.spi_byte(dc_high, b);
                }
                self.spi1_rx = 0xFF;
                self.spi1_rxne = true;
            }
            // USART1
            0x4001_3804 => {
                let b = (val & 0xFF) as u8;
                self.uart_tx_fifo.push_back(b);
                if b == b'\n' {
                    let line = self.serial_line_buf.trim_end_matches('\r').to_string();
                    if !line.is_empty() {
                        if self.serial_lines.len() >= 64 {
                            self.serial_lines.pop_front();
                        }
                        self.serial_lines.push_back(line);
                    }
                    self.serial_line_buf.clear();
                } else if (b.is_ascii_graphic() || b == b' ' || b == b'\r')
                    && self.serial_line_buf.len() < 160
                {
                    self.serial_line_buf.push(b as char);
                } else if b == 0 {
                    // RigWire COBS delimiter: discard binary noise from ASCII banner buffer
                    self.serial_line_buf.clear();
                }
            }
            0x4001_3808 => self.usart1_brr = val,
            0x4001_380C => self.usart1_cr1 = val,
            // RCC
            0x4002_1000 => {
                let mut cr = val | (1 << 8) | (1 << 10); // HSION + HSIRDY
                if (val & (1 << 24)) != 0 {
                    cr |= 1 << 25; // PLLRDY mirrors PLLON immediately
                } else {
                    cr &= !(1 << 25);
                }
                self.rcc_cr = cr;
                self.resync_sysclk();
            }
            0x4002_1004 => {
                self.rcc_icscr = val;
                self.resync_sysclk();
            }
            0x4002_1008 => {
                let sw = val & 0x07;
                self.rcc_cfgr = (val & !0x38) | (sw << 3);
                self.resync_sysclk();
            }
            0x4002_100C => {
                self.rcc_pllcfgr = val;
                self.resync_sysclk();
            }
            0x4002_1034 => self.rcc_iopenr = val,
            0x4002_1038 => self.rcc_ahbenr = val,
            0x4002_103C => self.rcc_apb1enr = val,
            0x4002_1040 => self.rcc_apb2enr = val,
            0x4002_1054 => self.rcc_ccipr = val,
            0x4002_105C => {
                let mut bd = val;
                if (val & 1) != 0 {
                    bd |= 2; // LSERDY
                }
                self.rcc_bdcr = bd;
            }
            0x4002_1060 => {
                if (val & (1 << 23)) != 0 {
                    // RMVF clears reset cause flags
                    self.rcc_csr &= 0x00FF_FFFF;
                }
                if (val & 1) != 0 {
                    self.rcc_csr |= 1 | 2; // LSION + LSIRDY
                } else {
                    self.rcc_csr &= !(1 | 2);
                }
            }
            // EXTI
            0x4002_1800 => self.exti_rtsr = val,
            0x4002_1804 => self.exti_ftsr = val,
            0x4002_180C => self.exti_pr &= !val, // W1C
            0x4002_1880 => self.exti_imr = val,
            // FLASH ACR
            0x4002_2000 => self.flash_acr = val,
            // CRC32
            0x4002_3000 => {
                self.crc_dr = crc32_mpeg2_word(self.crc_dr, val);
            }
            0x4002_3004 => self.crc_idr = (val & 0xFF) as u8,
            0x4002_3008 => {
                if (val & 1) != 0 {
                    self.crc_dr = 0xFFFF_FFFF;
                }
            }
            // GPIO
            0x5000_0000..=0x5000_17FF => {
                let port_idx = ((addr - 0x5000_0000) >> 10) as usize;
                let off = addr & 0x3FF;
                if port_idx < self.gpio.len()
                    && self.gpio[port_idx].write(off, val).is_some()
                    && matches!(port_idx, 0 | 1 | 5)
                {
                    self.sync_gpio_side_effects();
                }
            }
            // Cortex-M0+ System Control Space
            0xE000_E010 => self.syst_csr = (self.syst_csr & (1 << 16)) | (val & 0x07),
            0xE000_E014 => self.syst_rvr = val & 0x00FF_FFFF,
            0xE000_E018 => {
                self.syst_cvr = self.syst_rvr & 0x00FF_FFFF;
                self.syst_csr &= !(1 << 16);
            }
            0xE000_E100 => self.nvic_iser |= val,
            0xE000_E180 => self.nvic_iser &= !val,
            0xE000_E200 => self.nvic_ispr |= val,
            0xE000_E280 => self.nvic_ispr &= !val,
            0xE000_E400..=0xE000_E41C => {
                let i = (addr - 0xE000_E400) as usize;
                self.nvic_ipr[i..i + 4].copy_from_slice(&val.to_le_bytes());
            }
            0xE000_ED04 => {
                if (val & (1 << 28)) != 0 {
                    self.pendsv_pending = true;
                }
                if (val & (1 << 27)) != 0 {
                    self.pendsv_pending = false;
                }
                if (val & (1 << 26)) != 0 {
                    self.systick_pending = true;
                }
                if (val & (1 << 25)) != 0 {
                    self.systick_pending = false;
                }
            }
            0xE000_ED08 => self.scb_vtor = val & !0x7F,
            0xE000_ED0C => {
                if (val >> 16) == 0x05FA && (val & (1 << 2)) != 0 {
                    self.warm_reset_requested = true;
                }
            }
            0xE000_ED10 => self.scb_scr = val,
            0xE000_ED1C => self.scb_shpr2 = val,
            0xE000_ED20 => self.scb_shpr3 = val,
            _ => {}
        }
    }
}

fn crc32_mpeg2_word(mut crc: u32, word: u32) -> u32 {
    crc ^= word;
    for _ in 0..32 {
        if (crc & 0x8000_0000) != 0 {
            crc = (crc << 1) ^ 0x04C1_1DB7;
        } else {
            crc <<= 1;
        }
    }
    crc
}

fn populate_bootloader_stub(flash: &mut [u8]) {
    // Version string at 0x0800_1F80 ("7.02.02\0") + synthetic factory cert at 0x0800_2700
    flash[0x1F80..0x1F88].copy_from_slice(b"7.02.02\0");
    let cert: [u8; 104] = [
        0x2C, 0x40, 0xC8, 0x9B, 0x27, 0x88, 0xF2, 0xDE, 0x48, 0x11, 0x51, 0x34, 0x73, 0x26, 0xFC,
        0xEE, 0x8C, 0xC4, 0xF8, 0xB4, 0x44, 0xB9, 0xAD, 0xA1, 0x8B, 0x1D, 0x6D, 0x34, 0xBD, 0xC0,
        0xBB, 0xC6, 0x32, 0x14, 0x3F, 0x19, 0x81, 0x26, 0x17, 0xFC, 0x4A, 0x33, 0xF1, 0x49, 0x30,
        0x1F, 0xEF, 0x7C, 0x6A, 0xB1, 0xEC, 0x90, 0x45, 0x30, 0xEB, 0x0C, 0xC2, 0xD3, 0x10, 0x05,
        0xF4, 0x56, 0x15, 0x73, 0x02, 0xC7, 0xC6, 0x9B, 0x5F, 0xBC, 0xFD, 0xAB, 0x40, 0x76, 0x87,
        0x9F, 0xFE, 0x24, 0x78, 0x0F, 0x31, 0x2D, 0x73, 0x71, 0x43, 0x31, 0x31, 0x31, 0x35, 0x32,
        0x30, 0x32, 0x32, 0x32, 0x39, 0x30, 0x39, 0x31, 0x36, 0x31, 0x40, 0xA4, 0x38, 0x34,
    ];
    flash[0x2700..0x2700 + cert.len()].copy_from_slice(&cert);
}

/// Load either a 32-bit Little-Endian ARM ELF (`\x7FELF`) or a raw `.bin` image into internal Flash.
/// Returns the vector-table base address (`0x0800_2800` or `0x0800_0000`).
fn load_firmware_into_flash(flash: &mut [u8], img: &[u8]) -> std::io::Result<u32> {
    if img.len() >= 52 && &img[0..4] == b"\x7FELF" {
        if img[4] != 1 || img[5] != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Expected 32-bit little-endian ARM ELF",
            ));
        }
        let phoff = u32::from_le_bytes(img[28..32].try_into().unwrap()) as usize;
        let phentsize = u16::from_le_bytes(img[42..44].try_into().unwrap()) as usize;
        let phnum = u16::from_le_bytes(img[44..46].try_into().unwrap()) as usize;
        let mut vtor_cand: Option<u32> = None;

        for i in 0..phnum {
            let off = phoff + i * phentsize;
            if off + 32 > img.len() {
                break;
            }
            let p_type = u32::from_le_bytes(img[off..off + 4].try_into().unwrap());
            if p_type != 1 {
                continue; // PT_LOAD == 1
            }
            let p_offset = u32::from_le_bytes(img[off + 4..off + 8].try_into().unwrap()) as usize;
            let p_paddr = u32::from_le_bytes(img[off + 12..off + 16].try_into().unwrap());
            let p_filesz = u32::from_le_bytes(img[off + 16..off + 20].try_into().unwrap()) as usize;
            if p_filesz == 0 || p_offset + p_filesz > img.len() {
                continue;
            }
            // Skip rust-lld's synthetic PT_LOAD covering the ELF header itself at p_offset == 0
            if p_offset == 0 && img.starts_with(b"\x7FELF") && p_paddr < FLASH_BASE + APP_FLASH_OFFSET as u32 {
                continue;
            }
            if (FLASH_BASE..FLASH_BASE + FLASH_LEN as u32).contains(&p_paddr) {
                let dst = (p_paddr - FLASH_BASE) as usize;
                let n = p_filesz.min(FLASH_LEN.saturating_sub(dst));
                flash[dst..dst + n].copy_from_slice(&img[p_offset..p_offset + n]);
                if vtor_cand.is_none() && n >= 8 {
                    let sp = u32::from_le_bytes(flash[dst..dst + 4].try_into().unwrap());
                    let rst = u32::from_le_bytes(flash[dst + 4..dst + 8].try_into().unwrap());
                    if (SRAM_BASE..=SRAM_BASE + SRAM_LEN as u32).contains(&sp)
                        && (FLASH_BASE..FLASH_BASE + FLASH_LEN as u32).contains(&(rst & !1))
                        && (rst & 1) == 1
                    {
                        vtor_cand = Some(p_paddr);
                    }
                }
            }
        }
        Ok(vtor_cand.unwrap_or(FLASH_BASE + APP_FLASH_OFFSET as u32))
    } else {
        // Raw firmware.bin linked at 0x0800_2800 (unless full 128 KB dump starting with SP @ 0x2000xxxx and reset @ 0x08000xxx)
        let rst_vec = if img.len() >= 8 {
            u32::from_le_bytes(img[4..8].try_into().unwrap())
        } else {
            0
        };
        let dst_off = if (0x0800_0000..0x0800_2800).contains(&rst_vec) {
            0
        } else {
            APP_FLASH_OFFSET
        };
        let n = img.len().min(FLASH_LEN.saturating_sub(dst_off));
        flash[dst_off..dst_off + n].copy_from_slice(&img[..n]);
        Ok(FLASH_BASE + dst_off as u32)
    }
}
