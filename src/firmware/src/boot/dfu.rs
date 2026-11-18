//! Minimal factory bootloader DFU handoff (`0x0800_0000`).
//!
//! The rich DFU UI, splash screen, and SRAM SysTick hook runtime are offloaded
//! to `/apps/dfu.mod`. Firmware retains only the minimal raw fallback handoff
//! for unformatted/corrupted flash or post-wipe bootloader reboots.

use super::{read_backup_tag, write_backup_tag};
use drivers::st7565::St7565;

/// Bitmask in `KeypadScanner::poll_raw_mask()` for `PTT` (bit 18) + `Side Key 2` (bit 1).
pub const PTT_SIDE2_DFU_MASK: u32 = (1 << 18) | (1 << 1);

/// Returns `true` when both `PTT` and `Side Key 2` are held simultaneously.
#[inline(always)]
pub const fn is_ptt_side2_chord(raw_mask: u32) -> bool {
    (raw_mask & PTT_SIDE2_DFU_MASK) == PTT_SIDE2_DFU_MASK
}

/// Minimal raw DFU handoff when `/apps/dfu.mod` is unavailable or `FLAG_BOOTLOADER_HOLD` is set.
#[inline(never)]
pub fn enter_dfu_bootloader(lcd: &mut St7565) -> ! {
    let mut tag = read_backup_tag();
    tag.flags = (tag.flags & !kernel::boot::BackupCrashTag::FLAG_BOOTLOADER_HOLD)
        | kernel::boot::BackupCrashTag::FLAG_CLEAN_REBOOT;
    write_backup_tag(tag);

    bsp::set_status_led(bsp::StatusLed::Off);
    bsp::pins::BACKLIGHT.set_low();
    lcd.turn_off();

    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        let gpiob_idr = (py32_pac::gpio::GPIOB_BASE + 0x10) as *const u32;
        while (gpiob_idr.read_volatile() & 0xC000) != 0xC000 {
            py32_hal::delay::delay_ms(10);
        }
        py32_hal::delay::delay_ms(20);
    }

    handoff_to_factory_bootloader();
}

/// Applies `MODER`/`PUPDR` configuration and commits hardware pin lock via `GPIOx->LCKR`.
#[cfg(all(not(test), target_arch = "arm"))]
#[inline(never)]
unsafe fn gpio_apply_and_lock(
    base: u32,
    clear_2b: u32,
    pupdr_set: u32,
    moder_set: u32,
    lock_pins: u32,
) {
    unsafe {
        let port = base as *mut u32;
        let pupdr = port.byte_add(0x0C).read_volatile();
        port.byte_add(0x0C)
            .write_volatile((pupdr & !clear_2b) | pupdr_set);
        let moder = port.read_volatile();
        port.write_volatile((moder & !clear_2b) | moder_set);
        let lckr = port.byte_add(0x1C);
        let lckk = (1u32 << 16) | lock_pins;
        lckr.write_volatile(lckk);
        lckr.write_volatile(lock_pins);
        lckr.write_volatile(lckk);
        let _ = lckr.read_volatile();
    }
}

/// Locks keypad/flashlight pins, restores HSI clock, disconnects USB (`SE0`), resets DMA/APB
/// peripherals, and clears SysTick/NVIC before jumping to factory bootloader.
#[cfg(all(not(test), target_arch = "arm"))]
#[inline(never)]
unsafe fn quiesce_hw_for_dfu() {
    unsafe {
        let gpiob = py32_pac::gpio::GPIOB_BASE as *mut u32;
        let ptt_pin = 1u32 << 10;
        gpiob.byte_add(0x28).write_volatile(ptt_pin);
        let otyper = gpiob.byte_add(0x04).read_volatile();
        gpiob.byte_add(0x04).write_volatile(otyper | ptt_pin);
        gpio_apply_and_lock(
            py32_pac::gpio::GPIOB_BASE,
            0xF030_3FC0,
            0x5000_1540,
            0b01 << 20,
            0xC470,
        );

        let rcc = py32_pac::rcc::RCC_BASE as *mut u32;
        let iopenr = rcc.byte_add(0x34);
        iopenr.write_volatile(iopenr.read_volatile() | (1 << 2) | 0x1);
        let gpioc = py32_pac::gpio::GPIOC_BASE as *mut u32;
        gpioc.byte_add(0x28).write_volatile(1u32 << 13);
        gpio_apply_and_lock(
            py32_pac::gpio::GPIOC_BASE,
            0b11u32 << 26,
            0b10u32 << 26,
            0,
            1u32 << 13,
        );

        rcc.byte_add(0x08).write_volatile(0);
        while (rcc.byte_add(0x08).read_volatile() & 0x38) != 0 {}
        let cr = rcc.read_volatile();
        rcc.write_volatile(cr & !py32_pac::rcc::CR_PLLON);

        if (rcc.byte_add(0x3C).read_volatile() & py32_pac::rcc::APB1ENR_USBD) != 0 {
            py32_pac::usbd::USBD_POWER_U8.write_volatile(0);
        }

        rcc.byte_add(0x28).write_volatile(1);
        rcc.byte_add(0x28).write_volatile(0);
        rcc.byte_add(0x2C).write_volatile(0xFFFF_FFFF);
        rcc.byte_add(0x2C).write_volatile(0);
        rcc.byte_add(0x30).write_volatile(0xFFFF_FFFF);
        rcc.byte_add(0x30).write_volatile(0);
        rcc.byte_add(0x3C).write_volatile(0);
        rcc.byte_add(0x40).write_volatile(0);

        let gpioa = py32_pac::gpio::GPIOA_BASE as *mut u32;
        gpioa.byte_add(0x28).write_volatile(1u32 << 12);
        let pa_pupdr = gpioa.byte_add(0x0C).read_volatile();
        gpioa
            .byte_add(0x0C)
            .write_volatile(pa_pupdr & !(0b11u32 << 24));
        let pa_moder = gpioa.read_volatile();
        gpioa.write_volatile((pa_moder & !(0b11u32 << 24)) | (0b01u32 << 24));
        cortex_m::asm::delay(600_000);
        gpioa.write_volatile(pa_moder & !(0b11u32 << 24));

        let scs = 0xE000_E000usize as *mut u32;
        scs.byte_add(0x010).write_volatile(0);
        scs.byte_add(0xD04).write_volatile(1 << 25);
        scs.byte_add(0x180).write_volatile(0xFFFF_FFFF);
        scs.byte_add(0x280).write_volatile(0xFFFF_FFFF);
    }
}

/// Minimal raw DFU handoff to factory bootloader (`0x0800_0000`).
#[inline(never)]
fn handoff_to_factory_bootloader() -> ! {
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        cortex_m::interrupt::disable();
        quiesce_hw_for_dfu();

        let scs = 0xE000_E000usize as *mut u32;
        scs.byte_add(0xD08).write_volatile(0x0800_0000);
        let msp = (0x0800_0000 as *const u32).read_volatile();
        let entry = (0x0800_0004 as *const u32).read_volatile();
        core::arch::asm!(
            "dsb",
            "isb",
            "msr msp, {msp}",
            "cpsie i",
            "bx {entry}",
            msp = in(reg) msp,
            entry = in(reg) entry,
            options(noreturn)
        );
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptt_side2_chord_detection() {
        assert!(!is_ptt_side2_chord(0));
        assert!(!is_ptt_side2_chord(1 << 18));
        assert!(!is_ptt_side2_chord(1 << 1));
        assert!(is_ptt_side2_chord((1 << 18) | (1 << 1)));
        assert!(is_ptt_side2_chord((1 << 18) | (1 << 1) | (1 << 4)));
    }
}
