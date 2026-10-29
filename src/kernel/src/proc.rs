//! Microkernel Process Management & Control Blocks
//!
//! Implements lightweight process accounting, state tracking, and capability-checked
//! execution structures for static firmware apps and installable application bundles.

pub mod abi;
pub mod declare;
pub mod krx;
pub mod lease;
pub mod ledger;
pub mod memory;
pub mod paged;
pub mod primitives;
pub mod reloc;
pub mod service_registry;
#[cfg(feature = "krx_vm")]
pub mod vm;

pub use crate::boot::{BOOT_RECORD_MAGIC, BootRecord, BootSlot, ResetReason};
pub use crate::mem::{CellSlab, SlabCell};
pub use abi::*;
pub use krx::{
    BOOT_MAGIC, CALIB_MAGIC, FLAG_LEAF_PINNED, FLAG_LEAF_PREFETCH, FLAG_LEAF_RT, KERNFS_MAGIC,
    KRX_MAGIC, KrxClass, KrxExecType, KrxIndexEntry, KrxPreamble, KrxProfile, KrxSwapHeader,
    LeafContext, LeafContinuation, LeafEntry, PCB_MAGIC, SECTION_LEAF_MAP, SIG_MAGIC, SLAB_MAGIC,
    SWAP_FLAG_COMPRESSED_RLE, SWAP_MAGIC,
};
pub use lease::{LeaseError, LeaseKind, ResourceLease, ResourceLeaseManager};
pub use ledger::ResourceLedger;
pub use memory::{MemoryMapLayout, MemoryRegionDescriptor, RegionKind, region_flags};
pub use paged::{
    BLOCK_FLAG_HINT_NEXT, BLOCK_FLAG_PINNED, BLOCK_FLAG_RT, CacheSlotMeta, FixedWindowCache,
    PagedError, ReturnFrame, ShadowReturnStack, StructuredLeafManager,
};
pub use primitives::*;
pub use reloc::{RelocError, apply_relocs};
pub use service_registry::{MAX_REGISTERED_SERVICES, ServiceEntry, ServiceRegistry};
#[cfg(feature = "krx_vm")]
pub use vm::{Kvm16, VmError, VmStatus};

use crate::caps::CapabilitySet;

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
    /// Sandboxed bytecode or interpreted applet
    SandboxedBytecode,
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

/// Process Control Block (PCB)
///
/// Designed with fixed geometry and packed fields to fit within minimal SRAM footprint.
#[derive(Debug, Clone)]
pub struct Pcb {
    /// Ident verification tag (FourCC 'C0DE')
    pub ident: u16,
    /// Process ID
    pub pid: Pid,
    /// Human-readable process name
    pub name: heapless::String<16>,
    /// Associated App ID in AppRegistry or VTable catalog
    pub app_id: usize,
    /// Current execution state
    pub state: ProcState,
    /// Execution priority band
    pub priority: crate::sched::Priority,
    /// Execution classification
    pub exec_type: ExecType,
    /// Capability set granted to process
    pub caps: CapabilitySet,
    /// Static or dynamic memory usage watermark
    pub mem_bytes: u16,
    /// Size of hibernated compressed swap payload (0 when resident)
    pub compressed_bytes: u16,
    /// Restart attempts budget remaining before hard quarantine
    pub restart_budget: u8,
    /// Measured peak stack cost (in bytes)
    pub stack_cost: u16,
    /// Resource telemetry ledger
    pub ledger: ResourceLedger,
}

pub const PCB_IDENT: u16 = 0x06F9;

impl Pcb {
    pub const fn empty() -> Self {
        Self {
            ident: PCB_IDENT,
            pid: Pid(0),
            name: heapless::String::new(),
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
        let mut s = heapless::String::new();
        let _ = s.push_str(name);
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
pub struct ProcessTable<const N: usize = 10> {
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

    /// Allocate a PCB slot and assign a new Pid.
    pub fn spawn(
        &mut self,
        name: &str,
        app_id: usize,
        priority: crate::sched::Priority,
        exec_type: ExecType,
        caps: CapabilitySet,
    ) -> Result<Pid, ()> {
        for slot in self.slots.iter_mut() {
            if slot.state == ProcState::Dormant || slot.state == ProcState::Dead {
                let pid = Pid(self.next_pid);
                self.next_pid = self.next_pid.wrapping_add(1).max(1);
                *slot = Pcb::new(pid, name, app_id, priority, exec_type, caps);
                return Ok(pid);
            }
        }
        Err(())
    }

    /// Locate PCB by Pid.
    pub fn find(&self, pid: Pid) -> Option<&Pcb> {
        self.slots
            .iter()
            .find(|p| p.pid == pid && p.state != ProcState::Dormant)
    }

    /// Locate mutable PCB by Pid.
    pub fn find_mut(&mut self, pid: Pid) -> Option<&mut Pcb> {
        self.slots
            .iter_mut()
            .find(|p| p.pid == pid && p.state != ProcState::Dormant)
    }

    /// Locate PCB by app_id.
    pub fn find_by_app(&self, app_id: usize) -> Option<&Pcb> {
        self.slots
            .iter()
            .find(|p| p.app_id == app_id && p.state != ProcState::Dormant)
    }

    /// Locate mutable PCB by app_id.
    pub fn find_mut_by_app(&mut self, app_id: usize) -> Option<&mut Pcb> {
        self.slots
            .iter_mut()
            .find(|p| p.app_id == app_id && p.state != ProcState::Dormant)
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
        self.slots.iter().filter(|p| p.is_active()).count()
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
}
