//! External Hardware Port Security & Debug Lockdown (SWD PA13/PA14, UART Memory Dump Guard)

/// Sets SWD debug port lock status (PA13/PA14 pins).
/// When `enabled` is false, SWD pins are disabled to prevent external memory dumping via SWD programmer.
/// When `enabled` is true, SWD debugging is enabled.
pub fn set_swd_enabled(enabled: bool) {
    let gpioa_base = 0x5000_0000 as *mut u32; // GPIOA base on PY32F071
    unsafe {
        let moder = core::ptr::read_volatile(gpioa_base);
        if enabled {
            // Re-enable Alternate Function (AF0) for PA13 (SWDIO) and PA14 (SWCLK)
            let new_moder =
                (moder & !((0b11 << 26) | (0b11 << 28))) | ((0b10 << 26) | (0b10 << 28));
            core::ptr::write_volatile(gpioa_base, new_moder);
        } else {
            // Change PA13/PA14 to Input Mode (00) to lock out SWD hardware probes
            let new_moder = moder & !((0b11 << 26) | (0b11 << 28));
            core::ptr::write_volatile(gpioa_base, new_moder);
        }
    }
}

/// Queries current SWD debug port lock status (PA13/PA14 pins).
/// Returns true if SWD is enabled (Alternate Function mode), false if locked (Input mode).
pub fn is_swd_enabled() -> bool {
    let gpioa_base = 0x5000_0000 as *const u32;
    unsafe {
        let moder = core::ptr::read_volatile(gpioa_base);
        // PA13 moder is bits 26..27. If 0b10, Alternate Function is active (SWD enabled).
        ((moder >> 26) & 0b11) == 0b10
    }
}
