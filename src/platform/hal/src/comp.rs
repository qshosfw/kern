//! Analog Comparators (COMP1 & COMP2) for PY32F071.
//!
//! Provides fast analog threshold detection (e.g. carrier sense, squelch,
//! power thresholding) without CPU or ADC sampling overhead.
//!
//! Base Addresses:
//! - COMP1: 0x4001_0200
//! - COMP2: 0x4001_0210

const COMP1_CSR: *mut u32 = 0x4001_0200 as *mut u32;
const COMP2_CSR: *mut u32 = 0x4001_0210 as *mut u32;

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
        let mut val = 1u32; // EN = 1
        val |= (inm as u32) << 4;
        val |= (hyst as u32) << 8;
        unsafe {
            COMP1_CSR.write_volatile(val);
        }
    }

    /// Disables COMP1.
    pub fn disable_comp1() {
        unsafe {
            COMP1_CSR.write_volatile(0);
        }
    }

    /// Reads the live output level of COMP1 (true = high, false = low).
    #[inline(always)]
    pub fn read_comp1() -> bool {
        unsafe { (COMP1_CSR.read_volatile() & (1 << 30)) != 0 }
    }

    /// Configures and enables COMP2.
    pub fn enable_comp2(inm: InvertingInput, hyst: ComparatorHysteresis) {
        let mut val = 1u32; // EN = 1
        val |= (inm as u32) << 4;
        val |= (hyst as u32) << 8;
        unsafe {
            COMP2_CSR.write_volatile(val);
        }
    }

    /// Disables COMP2.
    pub fn disable_comp2() {
        unsafe {
            COMP2_CSR.write_volatile(0);
        }
    }

    /// Reads the live output level of COMP2 (true = high, false = low).
    #[inline(always)]
    pub fn read_comp2() -> bool {
        unsafe { (COMP2_CSR.read_volatile() & (1 << 30)) != 0 }
    }
}
