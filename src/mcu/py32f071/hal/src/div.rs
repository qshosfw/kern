//! Hardware 32÷32 Divider (`DIV` @ `0x4002_3800`) driver for PY32F071.
//!
//! Replaces ~100-cycle Cortex-M0+ software division loops (`__aeabi_uidivmod`) with
//! ~8-cycle AHB hardware division. Critical-section guarded so both thread mode and
//! ISRs can divide safely without corrupting `DIV_DEND` / `DIV_SOR`.

#[cfg(all(target_arch = "arm", not(test)))]
use pac::{div, rcc};

/// Compute `(num / den, num % den)` using the PY32F071 hardware divider (`(0, num)` when `den == 0`).
#[inline(never)]
pub fn udivmod(num: u32, den: u32) -> (u32, u32) {
    if den == 0 {
        return (0, num);
    }
    #[cfg(all(target_arch = "arm", not(test)))]
    {
        cortex_m::interrupt::free(|_| unsafe {
            let ahb = rcc::RCC_AHBENR.read_volatile();
            if (ahb & rcc::AHBENR_DIV) == 0 {
                rcc::RCC_AHBENR.write_volatile(ahb | rcc::AHBENR_DIV);
            }
            div::DIV_SIGN.write_volatile(0);
            div::DIV_DEND.write_volatile(num);
            div::DIV_SOR.write_volatile(den);
            while (div::DIV_STAT.read_volatile() & div::STAT_DIV_END) == 0 {}
            (div::DIV_QUOT.read_volatile(), div::DIV_REMA.read_volatile())
        })
    }
    #[cfg(not(all(target_arch = "arm", not(test))))]
    {
        (num / den, num % den)
    }
}

/// Compute `num / den` (`0` when `den == 0`) via the PY32F071 hardware divider.
#[inline(always)]
pub fn udiv(num: u32, den: u32) -> u32 {
    udivmod(num, den).0
}

/// Signed 32÷32 hardware division `(num / den, num % den)` (`(0, num)` on `den == 0`).
pub fn sdivmod(num: i32, den: i32) -> (i32, i32) {
    if den == 0 {
        return (0, num);
    }
    if num == i32::MIN && den == -1 {
        return (i32::MIN, 0);
    }
    #[cfg(all(target_arch = "arm", not(test)))]
    {
        cortex_m::interrupt::free(|_| unsafe {
            let ahb = rcc::RCC_AHBENR.read_volatile();
            if (ahb & rcc::AHBENR_DIV) == 0 {
                rcc::RCC_AHBENR.write_volatile(ahb | rcc::AHBENR_DIV);
            }
            div::DIV_SIGN.write_volatile(1);
            div::DIV_DEND.write_volatile(num as u32);
            div::DIV_SOR.write_volatile(den as u32);
            while (div::DIV_STAT.read_volatile() & div::STAT_DIV_END) == 0 {}
            (
                div::DIV_QUOT.read_volatile() as i32,
                div::DIV_REMA.read_volatile() as i32,
            )
        })
    }
    #[cfg(not(all(target_arch = "arm", not(test))))]
    {
        (num / den, num % den)
    }
}
