//! Microkernel Process Slot Table & Control Blocks (`32 B` Power-of-Two `Pcb`).
//!
//! Tracks real resident/foreground and Zone A background `.krx` actors using an exact
//! `32 B` (`2^5`) power-of-two slot layout (`lsls #5` indexing on Cortex-M0+, all fields
//! within `imm5` single-instruction offsets) and a compact `4 B` [`SlotHealth`] record.

pub mod abi;
pub mod lease;
pub mod ledger;
pub mod memory;
pub mod service_registry;
pub mod status_tray;

pub use crate::boot::{BOOT_RECORD_IDENT, BootRecord, BootSlot, ResetReason};
pub use abi::*;
pub use lease::{LeaseError, LeaseKind, ResourceLease, ResourceLeaseManager};
pub use ledger::SlotHealth;
pub use memory::{MemoryMapLayout, MemoryRegionDescriptor, RegionKind, region_flags};
pub use service_registry::{MAX_REGISTERED_SERVICES, ServiceEntry, ServiceRegistry};
pub use status_tray::StatusTray;

use ::abi::CapabilitySet;

/// Process Identifier (`0` = transient/unowned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Pid(pub u8);

/// Process Execution Class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExecType {
    /// Built-in static firmware app executing in-place from XIP flash
    StaticFirmware = 0,
    /// Native relocatable `.krx`/`.mod` executable loaded into SRAM arena
    RelocatableNative = 1,
}

/// Process Lifecycle State.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcState {
    Dormant = 0,
    Ready = 1,
    Running = 2,
    Suspended = 3,
    ServiceIdle = 4,
    Hibernated = 5,
    Stopped = 6,
    Dead = 7,
}

const PROC_STATE_NAMES: &[u8] = b"IDLE\0RDY\0RUN\0SUSP\0IDLE\0SWAP\0STOP\0DEAD\0";

impl ProcState {
    pub const fn as_short_str(&self) -> &'static str {
        utils::format::nth_cstr(PROC_STATE_NAMES, *self as u8)
    }
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

/// Exact 32-byte (`2^5`) Process Control Block (`Pcb`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Pcb {
    pub pid: Pid,
    pub state: ProcState,
    pub priority: crate::sched::Priority,
    pub exec_type: ExecType,
    pub app_id: u8,
    pub restart_budget: u8,
    pub mem_bytes: u16,
    pub compressed_bytes: u16,
    pub _pad: u16,
    pub caps: CapabilitySet,
    pub health: SlotHealth,
    pub name: PcbName,
}

const _: () = assert!(core::mem::size_of::<Pcb>() == 32);

impl Pcb {
    pub const fn empty() -> Self {
        Self {
            pid: Pid(0),
            state: ProcState::Dormant,
            priority: crate::sched::Priority::Ui,
            exec_type: ExecType::StaticFirmware,
            app_id: 0,
            restart_budget: 3,
            mem_bytes: 0,
            compressed_bytes: 0,
            _pad: 0,
            caps: CapabilitySet::empty(),
            health: SlotHealth::new(),
            name: PcbName::new(),
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
            pid,
            state: ProcState::Ready,
            priority,
            exec_type,
            app_id: app_id as u8,
            restart_budget: 3,
            mem_bytes: 0,
            compressed_bytes: 0,
            _pad: 0,
            caps,
            health: SlotHealth::new(),
            name: s,
        }
    }

    #[inline(always)]
    pub fn hibernate(&mut self, compressed_size: u16) {
        self.state = ProcState::Hibernated;
        self.compressed_bytes = compressed_size;
    }

    #[inline(always)]
    pub fn resume(&mut self) {
        self.state = ProcState::Suspended;
        self.compressed_bytes = 0;
    }

    #[inline(always)]
    pub fn is_hibernated(&self) -> bool {
        self.state == ProcState::Hibernated
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            ProcState::Ready | ProcState::Running | ProcState::Suspended | ProcState::ServiceIdle
        )
    }

    #[inline(always)]
    pub fn set_state(&mut self, state: ProcState) {
        self.state = state;
    }
}

/// Static, zero-allocation Process Slot Table (`N × 32 B`).
pub struct ProcessTable<const N: usize = 4> {
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

    pub fn init_in_place(dst: &mut core::mem::MaybeUninit<Self>) {
        let ptr = dst.as_mut_ptr();
        unsafe {
            core::ptr::write_bytes(ptr, 0, 1);
            let tbl = &mut *ptr;
            tbl.next_pid = 1;
            for slot in &mut tbl.slots {
                slot.priority = crate::sched::Priority::Ui;
                slot.restart_budget = 3;
            }
        }
    }

    fn spawn_inner(
        &mut self,
        name: &str,
        app_id: usize,
        priority: crate::sched::Priority,
        exec_type: ExecType,
        caps: CapabilitySet,
        allow_recycle: bool,
    ) -> Result<Pid, ()> {
        let aid = app_id as u8;
        let mut target_idx = if allow_recycle && aid < 0xF0 {
            self.slots.iter().position(|s| {
                s.state != ProcState::Dormant && s.state != ProcState::Dead && s.app_id == aid
            })
        } else {
            None
        };

        if target_idx.is_none() {
            target_idx = self
                .slots
                .iter()
                .position(|s| s.state == ProcState::Dormant || s.state == ProcState::Dead);
        }

        if target_idx.is_none() && allow_recycle {
            target_idx = self.slots.iter().position(|s| {
                (s.state == ProcState::Suspended || s.state == ProcState::Hibernated)
                    && s.app_id < 0xE0
                    && s.priority != crate::sched::Priority::Safety
            });
        }

        let Some(idx) = target_idx else {
            return Err(());
        };
        let pid = Pid(self.next_pid);
        self.next_pid = self.next_pid.wrapping_add(1).max(1);
        self.slots[idx] = Pcb::new(pid, name, app_id, priority, exec_type, caps);
        Ok(pid)
    }

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

    fn find_idx(&self, by_app: bool, id: u8) -> Option<usize> {
        for (i, p) in self.slots.iter().enumerate() {
            let hit = if by_app {
                p.app_id == id
            } else {
                p.pid.0 == id
            };
            if hit && p.state != ProcState::Dormant {
                return Some(i);
            }
        }
        None
    }

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

    pub fn find(&self, pid: Pid) -> Option<&Pcb> {
        self.find_idx(false, pid.0)
            .map(|i| unsafe { self.slots.get_unchecked(i) })
    }

    pub fn find_mut(&mut self, pid: Pid) -> Option<&mut Pcb> {
        self.find_idx(false, pid.0)
            .map(|i| unsafe { self.slots.get_unchecked_mut(i) })
    }

    pub fn find_by_app(&self, app_id: usize) -> Option<&Pcb> {
        self.find_idx(true, app_id as u8)
            .map(|i| unsafe { self.slots.get_unchecked(i) })
    }

    pub fn find_mut_by_app(&mut self, app_id: usize) -> Option<&mut Pcb> {
        self.find_idx(true, app_id as u8)
            .map(|i| unsafe { self.slots.get_unchecked_mut(i) })
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Pcb> {
        self.find_idx_name(name)
            .map(|i| unsafe { self.slots.get_unchecked(i) })
    }

    pub fn find_mut_by_name(&mut self, name: &str) -> Option<&mut Pcb> {
        self.find_idx_name(name)
            .map(|i| unsafe { self.slots.get_unchecked_mut(i) })
    }

    pub fn terminate(&mut self, pid: Pid) -> bool {
        if let Some(pcb) = self.find_mut(pid) {
            pcb.state = ProcState::Dead;
            true
        } else {
            false
        }
    }

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
    fn test_pcb_32b_geometry_and_hibernation_lifecycle() {
        assert_eq!(core::mem::size_of::<Pcb>(), 32);
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
        pcb.hibernate(42);
        assert!(pcb.is_hibernated());
        assert_eq!(pcb.compressed_bytes, 42);
        pcb.resume();
        assert_eq!(pcb.state, ProcState::Suspended);
        assert_eq!(pcb.compressed_bytes, 0);
    }

    #[test]
    fn test_process_table_slot_recycling_and_health() {
        let mut table = ProcessTable::<4>::new();
        let pid1 = table
            .spawn(
                "Home",
                0,
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
                ExecType::RelocatableNative,
                CapabilitySet::empty(),
            )
            .unwrap();
        let pid3 = table
            .spawn(
                "SvcA",
                0xE0,
                crate::sched::Priority::Radio,
                ExecType::RelocatableNative,
                CapabilitySet::empty(),
            )
            .unwrap();
        let pid4 = table
            .spawn(
                "SvcB",
                0xE1,
                crate::sched::Priority::Idle,
                ExecType::RelocatableNative,
                CapabilitySet::empty(),
            )
            .unwrap();

        table
            .find_mut(pid2)
            .unwrap()
            .set_state(ProcState::Suspended);
        table.find_mut(pid2).unwrap().health.record_page_fault();
        table.find_mut(pid2).unwrap().health.record_stack_depth(500);
        assert_eq!(table.find(pid2).unwrap().health.page_faults, 1);
        assert!(table.find(pid2).unwrap().health.stack_watermark_bytes() >= 500);

        assert!(
            table
                .spawn(
                    "AppB",
                    3,
                    crate::sched::Priority::Ui,
                    ExecType::RelocatableNative,
                    CapabilitySet::empty()
                )
                .is_err()
        );
        let pid_new = table
            .spawn_or_recycle(
                "AppB",
                3,
                crate::sched::Priority::Ui,
                ExecType::RelocatableNative,
                CapabilitySet::empty(),
            )
            .unwrap();
        assert!(table.find(pid1).is_some());
        assert!(table.find(pid2).is_none());
        assert!(table.find(pid3).is_some());
        assert!(table.find(pid4).is_some());
        assert_eq!(table.find(pid_new).unwrap().name.as_str(), "AppB");
    }
}
