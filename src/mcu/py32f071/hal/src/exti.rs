//! External Interrupt Driver (`EXTI` at `0x4002_1800` on PY32F071).

use pac::exti;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TriggerEdge {
    Rising,
    Falling,
    Both,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Port {
    GpioA = exti::PORT_GPIOA,
    GpioB = exti::PORT_GPIOB,
    GpioC = exti::PORT_GPIOC,
    GpioF = exti::PORT_GPIOF,
}

/// Bind an EXTI GPIO line `0..=15` to its physical GPIO port (`EXTI_EXTICR1..4`).
pub fn select_port(pin: u8, port: Port) {
    if pin >= 16 {
        return;
    }
    let reg = exti::exticr(pin);
    let shift = ((pin & 3) as u32) * 8;
    cortex_m::interrupt::free(|_| unsafe {
        let cur = reg.read_volatile();
        reg.write_volatile((cur & !(0xFF << shift)) | ((port as u32) << shift));
    });
}

pub fn enable_line(line: u8, trigger: TriggerEdge) {
    if line >= 32 {
        return;
    }
    let bit = 1u32 << line;
    cortex_m::interrupt::free(|_| unsafe {
        // Enable interrupt mask for this line
        let imr = exti::EXTI_IMR.read_volatile();
        exti::EXTI_IMR.write_volatile(imr | bit);

        match trigger {
            TriggerEdge::Rising => {
                let rtsr = exti::EXTI_RTSR.read_volatile();
                exti::EXTI_RTSR.write_volatile(rtsr | bit);

                let ftsr = exti::EXTI_FTSR.read_volatile();
                exti::EXTI_FTSR.write_volatile(ftsr & !bit);
            }
            TriggerEdge::Falling => {
                let ftsr = exti::EXTI_FTSR.read_volatile();
                exti::EXTI_FTSR.write_volatile(ftsr | bit);

                let rtsr = exti::EXTI_RTSR.read_volatile();
                exti::EXTI_RTSR.write_volatile(rtsr & !bit);
            }
            TriggerEdge::Both => {
                let rtsr = exti::EXTI_RTSR.read_volatile();
                exti::EXTI_RTSR.write_volatile(rtsr | bit);

                let ftsr = exti::EXTI_FTSR.read_volatile();
                exti::EXTI_FTSR.write_volatile(ftsr | bit);
            }
        }
    });
}

/// Internal EXTI line routed to PVD (Programmable Voltage Detector) on PY32F071 (`EXTI_IMR_IM16`).
pub const LINE_PVD: u8 = 16;

/// Internal EXTI line routed to LPTIM1 wakeup on PY32F071 (`EXTI_IMR_IM29`).
pub const LINE_LPTIM1: u8 = 29;

/// Enable multiple EXTI interrupt lines simultaneously without altering edge triggers.
/// Intended for internal peripheral wakeup lines (e.g. `LINE_LPTIM1`).
#[inline(always)]
pub fn enable_lines(mask: u32) {
    cortex_m::interrupt::free(|_| unsafe {
        let imr = exti::EXTI_IMR.read_volatile();
        exti::EXTI_IMR.write_volatile(imr | mask);
    });
}

pub fn disable_line(line: u8) {
    if line < 32 {
        disable_lines(1 << line);
    }
}

/// Mask multiple EXTI interrupt lines simultaneously.
#[inline(always)]
pub fn disable_lines(mask: u32) {
    cortex_m::interrupt::free(|_| unsafe {
        let imr = exti::EXTI_IMR.read_volatile();
        exti::EXTI_IMR.write_volatile(imr & !mask);
    });
}

pub fn is_pending(line: u8) -> bool {
    if line >= 32 {
        return false;
    }
    unsafe { (exti::EXTI_PR.read_volatile() & (1 << line)) != 0 }
}

pub fn clear_pending(line: u8) {
    if line < 32 {
        clear_pending_mask(1 << line);
    }
}

/// Acknowledge and clear all pending EXTI bits specified in `mask`.
#[inline(always)]
pub fn clear_pending_mask(mask: u32) {
    unsafe {
        exti::EXTI_PR.write_volatile(mask);
    }
}
