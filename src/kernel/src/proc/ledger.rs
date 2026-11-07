//! Process Resource Accounting & Diagnostics Ledger

/// Resource and execution telemetry tracked per process (KRX-ABI 2.5.0 Section 13.11).
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceLedger {
    /// Number of scheduling activations (run-to-completion invocations)
    pub cpu_ticks: u32,
    /// Number of IPC events dispatched to process
    pub ipc_events: u32,
    /// Maximum stack depth watermark observed in bytes
    pub stack_watermark: u16,
    /// Demand-paged overlay block faults
    pub page_faults: u16,
    /// Arena 16B slab cells currently used
    pub cells_used: u8,
    /// Swap hibernation count
    pub hibernations: u8,
    /// Soft fault / error count
    pub fault_count: u8,
}

impl ResourceLedger {
    pub const fn new() -> Self {
        Self {
            cpu_ticks: 0,
            ipc_events: 0,
            stack_watermark: 0,
            page_faults: 0,
            cells_used: 0,
            hibernations: 0,
            fault_count: 0,
        }
    }

    #[inline]
    pub fn record_activation(&mut self) {
        self.cpu_ticks = self.cpu_ticks.saturating_add(1);
    }

    #[inline]
    pub fn record_event(&mut self) {
        self.ipc_events = self.ipc_events.saturating_add(1);
    }

    #[inline]
    pub fn record_ipc(&mut self) {
        self.ipc_events = self.ipc_events.saturating_add(1);
    }

    #[inline]
    pub fn record_stack_depth(&mut self, depth: u16) {
        if depth > self.stack_watermark {
            self.stack_watermark = depth;
        }
    }

    #[inline]
    pub fn record_page_fault(&mut self) {
        self.page_faults = self.page_faults.saturating_add(1);
    }

    #[inline]
    pub fn record_hibernation(&mut self) {
        self.hibernations = self.hibernations.saturating_add(1);
    }

    #[inline]
    pub fn record_fault(&mut self) {
        self.fault_count = self.fault_count.saturating_add(1);
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}
