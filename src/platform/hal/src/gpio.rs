//! Type-safe GPIO pin control for PY32F071.
//!
//! Provides a [`Pin`] struct that correctly addresses BSRR (set/reset),
//! IDR (input read), and MODER (mode configuration) registers at the
//! right offsets from the port base address.

use pac::gpio;

/// A GPIO pin bound to a specific port and pin number.
///
/// Created via [`Pin::new`] with a port base address and pin number.
/// All register accesses use volatile reads/writes to prevent the
/// compiler from reordering or eliding hardware interactions.
#[derive(Clone, Copy)]
pub struct Pin {
    /// Upper 4 bits: port index (`(port_base >> 10) & 0x0F`), lower 4 bits: pin number (`0..15`).
    packed: u8,
}

impl Pin {
    /// Create a new GPIO pin reference.
    ///
    /// # Arguments
    /// - `port_base`: One of `pac::gpio::GPIOA_BASE`, `GPIOB_BASE`, etc.
    /// - `pin`: Pin number 0-15
    #[inline(always)]
    pub const fn new(port_base: u32, pin: u8) -> Self {
        let port_idx = ((port_base >> 10) & 0x0F) as u8;
        Self {
            packed: (port_idx << 4) | (pin & 0x0F),
        }
    }

    #[inline(always)]
    const fn port_base(self) -> u32 {
        0x5000_0000 + (((self.packed >> 4) as u32) << 10)
    }

    #[inline(always)]
    const fn pin(self) -> u8 {
        self.packed & 0x0F
    }

    /// Drive the pin HIGH via the BSRR set bits (lower 16 bits).
    #[inline(always)]
    pub fn set_high(self) {
        let bsrr = (self.port_base() + gpio::BSRR_OFFSET) as *mut u32;
        unsafe {
            bsrr.write_volatile(1 << self.pin());
        }
    }

    /// Drive the pin LOW via the BSRR reset bits (upper 16 bits).
    #[inline(always)]
    pub fn set_low(self) {
        let bsrr = (self.port_base() + gpio::BSRR_OFFSET) as *mut u32;
        unsafe {
            bsrr.write_volatile(1 << (self.pin() + 16));
        }
    }

    /// Read the pin level from the IDR register.
    #[inline(always)]
    pub fn is_high(self) -> bool {
        let idr = (self.port_base() + gpio::IDR_OFFSET) as *const u32;
        unsafe { (idr.read_volatile() >> self.pin()) & 1 != 0 }
    }

    /// Read the pin level (inverse of `is_high`).
    #[inline(always)]
    pub fn is_low(self) -> bool {
        !self.is_high()
    }

    #[inline(never)]
    fn modify_2bit(self, offset: u32, mode: u32) {
        let reg = (self.port_base() + offset) as *mut u32;
        let shift = (self.pin() as u32) * 2;
        cortex_m::interrupt::free(|_| unsafe {
            let val = reg.read_volatile();
            reg.write_volatile((val & !(0b11 << shift)) | (mode << shift));
        });
    }

    /// Set the pin mode in the MODER register.
    ///
    /// Each pin occupies 2 bits in MODER. Mode values:
    /// - `MODE_INPUT` (0b00)
    /// - `MODE_OUTPUT` (0b01)
    /// - `MODE_AF` (0b10)
    /// - `MODE_ANALOG` (0b11)
    #[inline(always)]
    pub fn set_mode(self, mode: u32) {
        self.modify_2bit(gpio::MODER_OFFSET, mode);
    }

    /// Set the output speed in the OSPEEDR register.
    #[inline(always)]
    pub fn set_speed(self, speed: u32) {
        self.modify_2bit(gpio::OSPEEDR_OFFSET, speed);
    }

    /// Set the pull-up/pull-down configuration in PUPDR.
    #[inline(always)]
    pub fn set_pull(self, pull: u32) {
        self.modify_2bit(gpio::PUPDR_OFFSET, pull);
    }

    /// Set the alternate function for this pin.
    ///
    /// Pins 0-7 use AFRL (offset 0x20), pins 8-15 use AFRH (offset 0x24).
    /// Each pin occupies 4 bits.
    #[inline(never)]
    pub fn set_af(self, af: u32) {
        let pin = self.pin();
        let (reg_offset, shift) = if pin < 8 {
            (gpio::AFRL_OFFSET, (pin as u32) * 4)
        } else {
            (gpio::AFRH_OFFSET, ((pin - 8) as u32) * 4)
        };
        let reg = (self.port_base() + reg_offset) as *mut u32;
        cortex_m::interrupt::free(|_| unsafe {
            let val = reg.read_volatile();
            reg.write_volatile((val & !(0xF << shift)) | (af << shift));
        });
    }

    #[inline(never)]
    fn configure_out(self, speed: u32, open_drain: bool) {
        self.set_mode(gpio::MODE_OUTPUT);
        self.set_speed(speed);
        let otyper = (self.port_base() + gpio::OTYPER_OFFSET) as *mut u32;
        let bit = 1u32 << self.pin();
        cortex_m::interrupt::free(|_| unsafe {
            let val = otyper.read_volatile();
            otyper.write_volatile(if open_drain { val | bit } else { val & !bit });
        });
    }

    /// Configure this pin as push-pull output with the given speed.
    #[inline(always)]
    pub fn configure_output(self, speed: u32) {
        self.configure_out(speed, false);
    }

    /// Configure this pin as open-drain output with the given speed.
    #[inline(always)]
    pub fn configure_output_od(self, speed: u32) {
        self.configure_out(speed, true);
    }

    /// Configure this pin as input with the given pull configuration.
    #[inline(never)]
    pub fn configure_input(self, pull: u32) {
        self.set_mode(gpio::MODE_INPUT);
        self.set_pull(pull);
    }

    /// Configure this pin as alternate function with the given AF number.
    #[inline(never)]
    pub fn configure_af(self, af: u32, speed: u32) {
        self.set_af(af);
        self.set_mode(gpio::MODE_AF);
        self.set_speed(speed);
    }

    /// Configure this pin as analog (lowest power, used for unused pins).
    #[inline(never)]
    pub fn configure_analog(self) {
        self.set_mode(gpio::MODE_ANALOG);
        self.set_pull(gpio::PULL_NONE);
    }
}

/// Configure multiple pins on a port as analog in one call.
/// `pin_mask` is a bitmask of pins (e.g., `PIN_0 | PIN_1 | PIN_5`).
pub fn set_pins_analog(port_base: u32, pin_mask: u32) {
    let moder = (port_base + gpio::MODER_OFFSET) as *mut u32;
    let pupdr = (port_base + gpio::PUPDR_OFFSET) as *mut u32;
    let mut moder_set = 0u32;
    for pin_num in 0..16u32 {
        if (pin_mask & (1 << pin_num)) != 0 {
            moder_set |= 0b11 << (pin_num * 2);
        }
    }
    cortex_m::interrupt::free(|_| unsafe {
        let val = moder.read_volatile();
        moder.write_volatile(val | moder_set);
        let pval = pupdr.read_volatile();
        pupdr.write_volatile(pval & !moder_set);
    });
}

// ── embedded-hal 1.0 Trait Implementations ─────────────────────────────────

impl embedded_hal::digital::ErrorType for Pin {
    type Error = core::convert::Infallible;
}

impl embedded_hal::digital::OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Pin::set_low(*self);
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        Pin::set_high(*self);
        Ok(())
    }
}

impl embedded_hal::digital::InputPin for Pin {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(Pin::is_high(*self))
    }
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(Pin::is_low(*self))
    }
}
