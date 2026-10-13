//! External Interrupt Driver

use pac::exti;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TriggerEdge {
    Rising,
    Falling,
    Both,
}

pub fn enable_line(line: u8, trigger: TriggerEdge) {
    assert!(line < 32);
    unsafe {
        // Enable interrupt mask for this line
        let imr = exti::EXTI_IMR.read_volatile();
        exti::EXTI_IMR.write_volatile(imr | (1 << line));

        match trigger {
            TriggerEdge::Rising => {
                let rtsr = exti::EXTI_RTSR.read_volatile();
                exti::EXTI_RTSR.write_volatile(rtsr | (1 << line));

                let ftsr = exti::EXTI_FTSR.read_volatile();
                exti::EXTI_FTSR.write_volatile(ftsr & !(1 << line));
            }
            TriggerEdge::Falling => {
                let ftsr = exti::EXTI_FTSR.read_volatile();
                exti::EXTI_FTSR.write_volatile(ftsr | (1 << line));

                let rtsr = exti::EXTI_RTSR.read_volatile();
                exti::EXTI_RTSR.write_volatile(rtsr & !(1 << line));
            }
            TriggerEdge::Both => {
                let rtsr = exti::EXTI_RTSR.read_volatile();
                exti::EXTI_RTSR.write_volatile(rtsr | (1 << line));

                let ftsr = exti::EXTI_FTSR.read_volatile();
                exti::EXTI_FTSR.write_volatile(ftsr | (1 << line));
            }
        }
    }
}

pub fn disable_line(line: u8) {
    assert!(line < 32);
    unsafe {
        // Disable interrupt mask for this line
        let imr = exti::EXTI_IMR.read_volatile();
        exti::EXTI_IMR.write_volatile(imr & !(1 << line));
    }
}

pub fn is_pending(line: u8) -> bool {
    assert!(line < 32);
    unsafe { (exti::EXTI_PR.read_volatile() & (1 << line)) != 0 }
}

pub fn clear_pending(line: u8) {
    assert!(line < 32);
    unsafe {
        // Clear pending bit by writing 1
        exti::EXTI_PR.write_volatile(1 << line);
    }
}
