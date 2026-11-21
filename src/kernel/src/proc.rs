//! Microkernel Process Management & Control Blocks
//!
//! Implements lightweight process accounting, state tracking, and capability-checked
//! execution structures for static firmware apps and installable application bundles.

pub mod abi;
pub mod lease;
pub mod ledger;
pub mod memory;
pub mod service_registry;
pub mod status_tray;

pub use crate::boot::{BOOT_RECORD_IDENT, BootRecord, BootSlot, ResetReason};
pub use crate::mem::{CellSlab, SlabCell};
pub use abi::*;
pub use lease::{LeaseError, LeaseKind, ResourceLease, ResourceLeaseManager};
pub use ledger::ResourceLedger;
pub use memory::{MemoryMapLayout, MemoryRegionDescriptor, RegionKind, region_flags};
pub use service_registry::{MAX_REGISTERED_SERVICES, ServiceEntry, ServiceRegistry};
pub use status_tray::StatusTray;

use ::abi::CapabilitySet;

/// Process Identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u8);

/// Process Execution Class
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecType {
    /// Builtin static firmware app executing in-place from XIP flash
    StaticFirmware,
    /// Native relocatable ELF/KRX executable loaded in flash
    RelocatableNative,
}

/// Process Lifecycle State (§12.3 / KRX-ABI 2.5.0)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    /// Slot is unallocated / dormant
    Dormant,
    /// Ready to run on next scheduler cycle
    Ready,
    /// Currently executing on CPU
    Running,
    /// Suspended / Waiting for IPC, event, or hardware timer
    Suspended,
    /// Resident background service idle waiting for client requests
    ServiceIdle,
    /// Hibernated / swapped to flash storage
    Hibernated,
    /// Stopped due to faults or budget exhaustion
    Stopped,
    /// Terminated, awaiting reap or slot recycle
    Dead,
}

const PROC_STATE_NAMES: &[u8] = b"IDLE\0RDY\0RUN\0SUSP\0IDLE\0SWAP\0STOP\0DEAD\0";

impl ProcState {
    pub const fn as_short_str(&self) -> &'static str {
        utils::format::nth_cstr(PROC_STATE_NAMES, *self as u8)
    }
}

/// Process Control Block (PCB)
///
/// Designed with fixed geometry and packed fields to fit within minimal SRAM footprint.
/// Hot `u8`/`u16` scalar fields sit at offsets `0..16` for single-instruction Thumb-1 `imm5` access.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Pcb {
    /// Process ID
    pub pid: Pid,
    /// Current execution state
    pub state: ProcState,
    /// Execution priority band
    pub priority: crate::sched::Priority,
    /// Execution classification
    pub exec_type: ExecType,
    /// Restart attempts budget remaining before hard quarantine
    pub restart_budget: u8,
    /// Ident verification tag (FourCC 'C0DE')
    pub ident: u16,
    /// Static or dynamic memory usage watermark
    pub mem_bytes: u16,
    /// Size of hibernated compressed swap payload (0 when resident)
    pub compressed_bytes: u16,
    /// Measured peak stack cost (in bytes)
    pub stack_cost: u16,
    /// Associated App ID in AppRegistry or VTable catalog
    pub app_id: usize,
    /// Capability set granted to process
    pub caps: CapabilitySet,
    /// Resource telemetry ledger
    pub ledger: ResourceLedger,
    /// Human-readable process name (compact 12B POD to minimize PCB static RAM footprint).
    pub name: PcbName,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct PcbName {
    pub len: u8,
    pub buf: [u8; 11],
}

impl PcbName {
    pub const fn new() -> Self {
        Self {
            len: 0,
            buf: [0u8; 11],
        }
    }

    #[inline(always)]
    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len as usize]) }
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len as usize]
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn push_str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let avail = self.buf.len().saturating_sub(self.len as usize);
        let mut n = bytes.len().min(avail);
        while n > 0 && n < bytes.len() && (bytes[n] & 0xC0) == 0x80 {
            n -= 1;
        }
        let start = self.len as usize;
        self.buf[start..start + n].copy_from_slice(&bytes[..n]);
        self.len = (start + n) as u8;
    }
}

impl PartialEq<&str> for PcbName {
    #[inline(always)]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

pub const PCB_IDENT: u16 = 0x06F9;

impl Pcb {
    pub const fn empty() -> Self {
        Self {
            ident: PCB_IDENT,
            pid: Pid(0),
            name: PcbName::new(),
            app_id: 0,
            state: ProcState::Dormant,
            priority: crate::sched::Priority::Ui,
            exec_type: ExecType::StaticFirmware,
            caps: CapabilitySet::empty(),
            mem_bytes: 0,
            compressed_bytes: 0,
            restart_budget: 3,
            stack_cost: 0,
            ledger: ResourceLedger::new(),
        }
    }

    pub fn new(
        pid: Pid,
        name: &str,
        app_id: usize,
        priority: crate::sched::Priority,
        exec_type: ExecType,
        caps: CapabilitySet,
    ) -> Self {
        let mut s = PcbName::new();
        s.push_str(name);
        Self {
            ident: PCB_IDENT,
            pid,
            name: s,
            app_id,
            state: ProcState::Ready,
            priority,
            exec_type,
            caps,
            mem_bytes: 0,
            compressed_bytes: 0,
            restart_budget: 3,
            stack_cost: 0,
            ledger: ResourceLedger::new(),
        }
    }

    #[inline]
    pub fn hibernate(&mut self, compressed_size: u16) {
        self.state = ProcState::Hibernated;
        self.compressed_bytes = compressed_size;
    }

    #[inline]
    pub fn resume(&mut self) {
        self.state = ProcState::Suspended;
        self.compressed_bytes = 0;
    }

    #[inline]
    pub fn is_hibernated(&self) -> bool {
        self.state == ProcState::Hibernated
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            ProcState::Ready | ProcState::Running | ProcState::Suspended | ProcState::ServiceIdle
        )
    }

    #[inline]
    pub fn set_state(&mut self, state: ProcState) {
        self.state = state;
    }
}

/// Static, zero-allocation Process Table for up to N concurrent processes.
pub struct ProcessTable<const N: usize = 8> {
    pub slots: [Pcb; N],
    next_pid: u8,
}

impl<const N: usize> Default for ProcessTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ProcessTable<N> {
    pub const fn new() -> Self {
        Self {
            slots: [const { Pcb::empty() }; N],
            next_pid: 1,
        }
    }

    /// Zero-copy in-place initialization avoiding a ~484 B stack temporary on Cortex-M0+.
    #[inline(never)]
    pub fn init_in_place(dst: &mut core::mem::MaybeUninit<Self>) {
        let ptr = dst.as_mut_ptr();
        unsafe {
            core::ptr::write_bytes(ptr, 0, 1);
            let tbl = &mut *ptr;
            tbl.next_pid = 1;
            for slot in &mut tbl.slots {
                slot.ident = PCB_IDENT;
                slot.priority = crate::sched::Priority::Ui;
                slot.restart_budget = 3;
            }
        }
    }

    #[inline(never)]
    fn spawn_inner(
        &mut self,
        name: &str,
        app_id: usize,
        priority: crate::sched::Priority,
        exec_type: ExecType,
        caps: CapabilitySet,
        allow_recycle: bool,
    ) -> Result<Pid, ()> {
        let mut target_idx = self
            .slots
            .iter()
            .position(|s| s.state == ProcState::Dormant || s.state == ProcState::Dead);

        if target_idx.is_none() && allow_recycle {
            let mut min_ticks = u32::MAX;
            for (i, slot) in self.slots.iter().enumerate() {
                if (slot.state == ProcState::Suspended || slot.state == ProcState::Hibernated)
                    && slot.app_id < 0xF0
                    && slot.ledger.cpu_ticks < min_ticks
                {
                    min_ticks = slot.ledger.cpu_ticks;
                    target_idx = Some(i);
                }
            }
        }

        let Some(idx) = target_idx else {
            return Err(());
        };
        let pid = Pid(self.next_pid);
        self.next_pid = self.next_pid.wrapping_add(1).max(1);
        self.slots[idx] = Pcb::new(pid, name, app_id, priority, exec_type, caps);
        Ok(pid)
    }

    /// Allocate a PCB slot and assign a new Pid.
    pub fn spawn(
        &mut self,
        name: &str,
        app_id: usize,
        priority: crate::sched::Priority,
        exec_type: ExecType,
        caps: CapabilitySet,
    ) -> Result<Pid, ()> {
        self.spawn_inner(name, app_id, priority, exec_type, caps, false)
    }

    /// Allocate a PCB slot, recycling the least-recently-active suspended process if all slots are full.
    ///
    /// Never evicts system-critical daemons (PID <= 4 or Priority::Safety).
    pub fn spawn_or_recycle(
        &mut self,
        name: &str,
        app_id: usize,
        priority: crate::sched::Priority,
        exec_type: ExecType,
        caps: CapabilitySet,
    ) -> Result<Pid, ()> {
        self.spawn_inner(name, app_id, priority, exec_type, caps, true)
    }

    #[inline(never)]
    fn find_idx(&self, by_app: bool, id: usize) -> Option<usize> {
        for (i, p) in self.slots.iter().enumerate() {
            let hit = if by_app {
                p.app_id == id
            } else {
                p.pid.0 as usize == id
            };
            if hit && p.state != ProcState::Dormant {
                return Some(i);
            }
        }
        None
    }

    #[inline(never)]
    fn find_idx_name(&self, name: &str) -> Option<usize> {
        for (i, p) in self.slots.iter().enumerate() {
            if p.state != ProcState::Dormant
                && utils::format::ascii_eq_ignore_case(p.name.as_str(), name)
            {
                return Some(i);
            }
        }
        None
    }

    /// Locate PCB by Pid.
    pub fn find(&self, pid: Pid) -> Option<&Pcb> {
        self.find_idx(false, pid.0 as usize)
            .map(|i| unsafe { self.slots.get_unchecked(i) })
    }

    /// Locate mutable PCB by Pid.
    pub fn find_mut(&mut self, pid: Pid) -> Option<&mut Pcb> {
        self.find_idx(false, pid.0 as usize)
            .map(|i| unsafe { self.slots.get_unchecked_mut(i) })
    }

    /// Locate PCB by app_id.
    pub fn find_by_app(&self, app_id: usize) -> Option<&Pcb> {
        self.find_idx(true, app_id)
            .map(|i| unsafe { self.slots.get_unchecked(i) })
    }

    /// Locate mutable PCB by app_id.
    pub fn find_mut_by_app(&mut self, app_id: usize) -> Option<&mut Pcb> {
        self.find_idx(true, app_id)
            .map(|i| unsafe { self.slots.get_unchecked_mut(i) })
    }

    /// Locate PCB by process name (case-insensitive ASCII match).
    pub fn find_by_name(&self, name: &str) -> Option<&Pcb> {
        self.find_idx_name(name)
            .map(|i| unsafe { self.slots.get_unchecked(i) })
    }

    /// Locate mutable PCB by process name (case-insensitive ASCII match).
    pub fn find_mut_by_name(&mut self, name: &str) -> Option<&mut Pcb> {
        self.find_idx_name(name)
            .map(|i| unsafe { self.slots.get_unchecked_mut(i) })
    }

    /// Terminate process and recycle slot.
    pub fn terminate(&mut self, pid: Pid) -> bool {
        if let Some(pcb) = self.find_mut(pid) {
            pcb.state = ProcState::Dead;
            true
        } else {
            false
        }
    }

    /// Count of currently active (non-dormant, non-dead) processes.
    pub fn active_count(&self) -> usize {
        let mut count = 0;
        for p in &self.slots {
            if p.is_active() {
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcb_hibernation_lifecycle() {
        let mut pcb = Pcb::new(
            Pid(1),
            "TestApp",
            2,
            crate::sched::Priority::Ui,
            ExecType::StaticFirmware,
            CapabilitySet::empty(),
        );
        assert_eq!(pcb.state, ProcState::Ready);
        assert!(!pcb.is_hibernated());
        assert_eq!(pcb.compressed_bytes, 0);

        pcb.hibernate(42);
        assert_eq!(pcb.state, ProcState::Hibernated);
        assert!(pcb.is_hibernated());
        assert_eq!(pcb.compressed_bytes, 42);

        pcb.resume();
        assert_eq!(pcb.state, ProcState::Suspended);
        assert!(!pcb.is_hibernated());
        assert_eq!(pcb.compressed_bytes, 0);
    }

    #[test]
    fn test_process_table_daemon_accounting_and_multi_app_concurrency() {
        let mut table = ProcessTable::<8>::new();

        // 1. Spawn system daemons
        let pid_kernel = table
            .spawn(
                "Kernel",
                0xF0,
                crate::sched::Priority::Safety,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();
        let pid_radio = table
            .spawn(
                "Radio",
                0xF1,
                crate::sched::Priority::Radio,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();
        let pid_power = table
            .spawn(
                "Power",
                0xF2,
                crate::sched::Priority::Safety,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();

        // 2. Spawn foreground App 1 (VFO, app_id 0)
        let pid_vfo = table
            .spawn(
                "Radio",
                0,
                crate::sched::Priority::Ui,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();

        assert_eq!(pid_kernel, Pid(1));
        assert_eq!(pid_radio, Pid(2));
        assert_eq!(pid_power, Pid(3));
        assert_eq!(pid_vfo, Pid(4));

        // Mark radio daemon as ServiceIdle
        table
            .find_mut(pid_radio)
            .unwrap()
            .set_state(ProcState::ServiceIdle);

        // 3. Simulate 5 scheduler ticks: daemons receive background activations
        for _ in 0..5 {
            table
                .find_mut(pid_kernel)
                .unwrap()
                .ledger
                .record_activation();
            table
                .find_mut(pid_radio)
                .unwrap()
                .ledger
                .record_activation();
            table
                .find_mut(pid_power)
                .unwrap()
                .ledger
                .record_activation();
            table.find_mut(pid_vfo).unwrap().ledger.record_activation();
            table.find_mut(pid_vfo).unwrap().ledger.record_event();
        }

        assert_eq!(table.find(pid_kernel).unwrap().ledger.cpu_ticks, 5);
        assert_eq!(table.find(pid_radio).unwrap().ledger.cpu_ticks, 5);
        assert_eq!(table.find(pid_power).unwrap().ledger.cpu_ticks, 5);
        assert_eq!(table.find(pid_vfo).unwrap().ledger.cpu_ticks, 5);
        assert_eq!(table.find(pid_vfo).unwrap().ledger.ipc_events, 5);

        // 4. Switch foreground app from VFO (0) to Explorer (app_id 10)
        table
            .find_mut(pid_vfo)
            .unwrap()
            .set_state(ProcState::Suspended);
        let pid_explorer = table
            .spawn(
                "Explorer",
                10,
                crate::sched::Priority::Ui,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();
        table
            .find_mut(pid_explorer)
            .unwrap()
            .set_state(ProcState::Running);

        // 5. Simulate 3 more background ticks while Explorer is active
        for _ in 0..3 {
            table
                .find_mut(pid_kernel)
                .unwrap()
                .ledger
                .record_activation();
            table
                .find_mut(pid_radio)
                .unwrap()
                .ledger
                .record_activation();
            table
                .find_mut(pid_power)
                .unwrap()
                .ledger
                .record_activation();
            table
                .find_mut(pid_explorer)
                .unwrap()
                .ledger
                .record_activation();
        }

        // Background daemons continued running concurrently!
        assert_eq!(table.find(pid_radio).unwrap().ledger.cpu_ticks, 8);
        assert_eq!(table.find(pid_radio).unwrap().state, ProcState::ServiceIdle);
        assert_eq!(table.find(pid_kernel).unwrap().ledger.cpu_ticks, 8);
        // VFO state was preserved in Suspended state
        assert_eq!(table.find(pid_vfo).unwrap().state, ProcState::Suspended);
        assert_eq!(table.find(pid_vfo).unwrap().ledger.cpu_ticks, 5);
        // Explorer accumulated 3 activations
        assert_eq!(table.find(pid_explorer).unwrap().ledger.cpu_ticks, 3);
    }

    #[test]
    fn test_process_table_slot_recycling_under_saturation() {
        let mut table = ProcessTable::<4>::new();

        // Fill all 4 slots: 1 daemon + 3 user apps
        let pid1 = table
            .spawn(
                "Sys",
                1,
                crate::sched::Priority::Safety,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();
        let pid2 = table
            .spawn(
                "AppA",
                2,
                crate::sched::Priority::Ui,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();
        let pid3 = table
            .spawn(
                "AppB",
                3,
                crate::sched::Priority::Ui,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();
        let pid4 = table
            .spawn(
                "AppC",
                4,
                crate::sched::Priority::Ui,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();

        // Mark AppA, AppB, AppC suspended
        table
            .find_mut(pid2)
            .unwrap()
            .set_state(ProcState::Suspended);
        table.find_mut(pid2).unwrap().ledger.cpu_ticks = 10;
        table
            .find_mut(pid3)
            .unwrap()
            .set_state(ProcState::Suspended);
        table.find_mut(pid3).unwrap().ledger.cpu_ticks = 2; // Lowest ticks -> should be victim
        table
            .find_mut(pid4)
            .unwrap()
            .set_state(ProcState::Suspended);
        table.find_mut(pid4).unwrap().ledger.cpu_ticks = 8;

        // Normal spawn fails because all slots are occupied
        assert!(
            table
                .spawn(
                    "AppD",
                    5,
                    crate::sched::Priority::Ui,
                    ExecType::StaticFirmware,
                    CapabilitySet::empty()
                )
                .is_err()
        );

        // spawn_or_recycle succeeds by recycling AppB (lowest CPU ticks)
        let pid_new = table
            .spawn_or_recycle(
                "AppD",
                5,
                crate::sched::Priority::Ui,
                ExecType::StaticFirmware,
                CapabilitySet::empty(),
            )
            .unwrap();
        assert_eq!(pid_new, Pid(5));

        // Daemon and other apps preserved, AppB replaced by AppD
        assert!(table.find(pid1).is_some());
        assert!(table.find(pid2).is_some());
        assert!(table.find(pid3).is_none()); // Evicted
        assert!(table.find(pid4).is_some());
        assert_eq!(table.find(pid_new).unwrap().name.as_str(), "AppD");
        assert_eq!(table.find(pid_new).unwrap().app_id, 5);
    }
}
