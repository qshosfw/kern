//! Compact 4-byte KRX Slot Health & Diagnostics (`docs/kern-nextgen-architecture-and-silicon.md` §2.1 #3).

/// Compact 4-byte (`2^2`) execution health record per process slot.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlotHealth {
    /// Demand-paged overlay / virtual code cache faults.
    pub page_faults: u16,
    /// Peak observed MSP stack depth divided by 32 bytes (`0..255` -> up to `8,160 B`).
    pub stack_hwm_div32: u8,
    /// Contained guest / slice fault count.
    pub faults: u8,
}

const _: () = assert!(core::mem::size_of::<SlotHealth>() == 4);

impl SlotHealth {
    pub const fn new() -> Self {
        Self {
            page_faults: 0,
            stack_hwm_div32: 0,
            faults: 0,
        }
    }

    #[inline(always)]
    pub fn record_stack_depth(&mut self, depth_bytes: u16) {
        let q = ((depth_bytes + 31) >> 5).min(255) as u8;
        if q > self.stack_hwm_div32 {
            self.stack_hwm_div32 = q;
        }
    }

    #[inline(always)]
    pub const fn stack_watermark_bytes(&self) -> u16 {
        (self.stack_hwm_div32 as u16) << 5
    }

    #[inline(always)]
    pub fn record_page_fault(&mut self) {
        self.page_faults = self.page_faults.saturating_add(1);
    }

    #[inline(always)]
    pub fn record_fault(&mut self) {
        self.faults = self.faults.saturating_add(1);
    }
}
