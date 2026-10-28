//! Hardware CRC32 Calculation Unit for PY32F071.
//!
//! Peripheral Base: 0x4002_3000 (AHB)
//! Polynomial: 0x04C11DB7 (standard Ethernet / ISO 3309 / MPEG-2 / ZIP)
//! Execution time: 4 AHB clock cycles per 32-bit word (instantaneous zero-CPU).

const CRC_BASE: usize = 0x4002_3000;
const CRC_DR: *mut u32 = CRC_BASE as *mut u32;
const CRC_IDR: *mut u8 = (CRC_BASE + 0x04) as *mut u8;
const CRC_CR: *mut u32 = (CRC_BASE + 0x08) as *mut u32;

pub struct HardwareCrc;

impl HardwareCrc {
    /// Resets the CRC data register to 0xFFFF_FFFF.
    #[inline(always)]
    pub fn reset() {
        unsafe {
            CRC_CR.write_volatile(1); // RESET bit
        }
    }

    /// Feeds a single 32-bit word into the hardware CRC accumulator and returns the new CRC.
    #[inline(always)]
    pub fn feed_word(word: u32) -> u32 {
        unsafe {
            CRC_DR.write_volatile(word);
            CRC_DR.read_volatile()
        }
    }

    /// Feeds a slice of 32-bit words into the hardware CRC accumulator.
    pub fn feed_slice(words: &[u32]) -> u32 {
        for &w in words {
            unsafe {
                CRC_DR.write_volatile(w);
            }
        }
        unsafe { CRC_DR.read_volatile() }
    }

    /// Feeds a byte buffer by packing 4 bytes per word into the CRC hardware.
    /// Trailing 1-3 bytes are padded into a final word with zeroes.
    pub fn feed_bytes(bytes: &[u8]) -> u32 {
        let mut chunks = bytes.chunks_exact(4);
        for chunk in chunks.by_ref() {
            let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            unsafe {
                CRC_DR.write_volatile(w);
            }
        }
        let rem = chunks.remainder();
        if !rem.is_empty() {
            let mut pad = [0u8; 4];
            pad[..rem.len()].copy_from_slice(rem);
            let w = u32::from_le_bytes(pad);
            unsafe {
                CRC_DR.write_volatile(w);
            }
        }
        unsafe { CRC_DR.read_volatile() }
    }

    /// Computes the complete CRC32 of a byte slice with automatic reset.
    pub fn compute_bytes(bytes: &[u8]) -> u32 {
        Self::reset();
        Self::feed_bytes(bytes)
    }

    /// Reads the current CRC value from the data register without mutating.
    #[inline(always)]
    pub fn read() -> u32 {
        unsafe { CRC_DR.read_volatile() }
    }

    /// Stores an 8-bit value into the independent data register (preserved during CRC reset).
    #[inline(always)]
    pub fn write_idr(val: u8) {
        unsafe {
            CRC_IDR.write_volatile(val);
        }
    }

    /// Reads the 8-bit value from the independent data register.
    #[inline(always)]
    pub fn read_idr() -> u8 {
        unsafe { CRC_IDR.read_volatile() }
    }
}
