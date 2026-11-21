//! Analog Comparators (COMP1 & COMP2) for PY32F071.
//!
//! Provides fast analog threshold detection (e.g. carrier sense, squelch,
//! power thresholding) without CPU or ADC sampling overhead.

use pac::comp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparatorHysteresis {
    None = 0,
    Low = 1,
    Medium = 2,
    High = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvertingInput {
    Vref1_4 = 0,
    Vref1_2 = 1,
    Vref3_4 = 2,
    VrefInt = 3,
}

pub struct AnalogComparator;

impl AnalogComparator {
    /// Configures and enables COMP1.
    pub fn enable_comp1(inm: InvertingInput, hyst: ComparatorHysteresis) {
        crate::rcc::enable_syscfg();
        let val = comp::CSR_EN | ((inm as u32) << 4) | ((hyst as u32) << 8);
        unsafe {
            comp::COMP1_CSR.write_volatile(val);
        }
    }

    /// Disables COMP1.
    pub fn disable_comp1() {
        unsafe {
            comp::COMP1_CSR.write_volatile(0);
        }
    }

    /// Reads the live output level of COMP1 (true = high, false = low).
    #[inline(always)]
    pub fn read_comp1() -> bool {
        unsafe { (comp::COMP1_CSR.read_volatile() & comp::CSR_COMP_OUT) != 0 }
    }

    /// Configures and enables COMP2.
    pub fn enable_comp2(inm: InvertingInput, hyst: ComparatorHysteresis) {
        crate::rcc::enable_syscfg();
        let val = comp::CSR_EN | ((inm as u32) << 4) | ((hyst as u32) << 8);
        unsafe {
            comp::COMP2_CSR.write_volatile(val);
        }
    }

    /// Disables COMP2.
    pub fn disable_comp2() {
        unsafe {
            comp::COMP2_CSR.write_volatile(0);
        }
    }

    /// Reads the live output level of COMP2 (true = high, false = low).
    #[inline(always)]
    pub fn read_comp2() -> bool {
        unsafe { (comp::COMP2_CSR.read_volatile() & comp::CSR_COMP_OUT) != 0 }
    }
}
