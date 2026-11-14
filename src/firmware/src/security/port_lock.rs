//! External Hardware Port Security & Debug Lockdown (SWD PA13/PA14, UART Memory Dump Guard)

use py32_hal::gpio::Pin;
use py32_hal::pac::gpio::{AF0, GPIOA_BASE, MODE_AF, MODE_INPUT, MODER_OFFSET};

const SWDIO_PIN: u8 = 13;
const SWCLK_PIN: u8 = 14;
const SWD_MODER_MASK: u32 = (0b11 << (SWDIO_PIN * 2)) | (0b11 << (SWCLK_PIN * 2));
const SWD_MODER_AF: u32 = (MODE_AF << (SWDIO_PIN * 2)) | (MODE_AF << (SWCLK_PIN * 2));

/// Sets SWD debug port lock status (PA13/PA14 pins).
/// When `enabled` is false, SWD pins are disabled to prevent external memory dumping via SWD programmer.
/// When `enabled` is true, SWD debugging is enabled (AF0 on both PA13 SWDIO and PA14 SWCLK).
pub fn set_swd_enabled(enabled: bool) {
    if enabled {
        Pin::new(GPIOA_BASE, SWDIO_PIN).set_af(AF0);
        Pin::new(GPIOA_BASE, SWCLK_PIN).set_af(AF0);
    }

    let moder_reg = (GPIOA_BASE + MODER_OFFSET) as *mut u32;
    cortex_m::interrupt::free(|_| unsafe {
        let moder = moder_reg.read_volatile();
        let next = if enabled {
            (moder & !SWD_MODER_MASK) | SWD_MODER_AF
        } else {
            (moder & !SWD_MODER_MASK)
                | ((MODE_INPUT << (SWDIO_PIN * 2)) | (MODE_INPUT << (SWCLK_PIN * 2)))
        };
        moder_reg.write_volatile(next);
    });
}

/// Queries current SWD debug port lock status (PA13/PA14 pins).
/// Returns true only if both PA13 (SWDIO) and PA14 (SWCLK) are in Alternate Function mode.
pub fn is_swd_enabled() -> bool {
    let moder_reg = (GPIOA_BASE + MODER_OFFSET) as *const u32;
    unsafe { (moder_reg.read_volatile() & SWD_MODER_MASK) == SWD_MODER_AF }
}
