//! Dynamic Resource Lease Management (§9 / KRX-ABI 3.2).
//!
//! Replaces ambiguous peripheral ownership with explicit, timed resource leases
//! (`RadioRx`, `RadioTx`, `AudioCapture`, `AudioOutput`, `DisplayCanvas`, `SpiBus`, `DmaChannel`, `OverlayWindow`, `StorageTransaction`).
//! Leases are deterministically revoked when an actor stops, yields, or faults.

use super::Pid;

/// Typed hardware or subsystem resource available for lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LeaseKind {
    RadioRx = 1,
    RadioTx = 2,
    AudioCapture = 3,
    AudioOutput = 4,
    DisplayCanvas = 5,
    SpiBus = 6,
    DmaChannel = 7,
    OverlayWindow = 8,
    StorageTransaction = 9,
}

impl LeaseKind {
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(Self::RadioRx),
            2 => Some(Self::RadioTx),
            3 => Some(Self::AudioCapture),
            4 => Some(Self::AudioOutput),
            5 => Some(Self::DisplayCanvas),
            6 => Some(Self::SpiBus),
            7 => Some(Self::DmaChannel),
            8 => Some(Self::OverlayWindow),
            9 => Some(Self::StorageTransaction),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::RadioRx => "RadioRx",
            Self::RadioTx => "RadioTx",
            Self::AudioCapture => "AudioCapture",
            Self::AudioOutput => "AudioOutput",
            Self::DisplayCanvas => "DisplayCanvas",
            Self::SpiBus => "SpiBus",
            Self::DmaChannel => "DmaChannel",
            Self::OverlayWindow => "OverlayWindow",
            Self::StorageTransaction => "StorageTxn",
        }
    }
}

/// Lease acquisition error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    AlreadyLeased { holder: Pid },
    NotLeased,
    NotOwner,
    LeaseTableFull,
    InvalidLeaseKind,
}

/// An active timed resource lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLease {
    pub kind: LeaseKind,
    pub owner: Pid,
    pub acquired_tick: u32,
    pub timeout_ms: u32,
}

/// Zero-allocation bounded resource lease manager.
#[derive(Debug, Clone)]
pub struct ResourceLeaseManager<const MAX_LEASES: usize = 12> {
    pub leases: [Option<ResourceLease>; MAX_LEASES],
}

impl<const MAX_LEASES: usize> Default for ResourceLeaseManager<MAX_LEASES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_LEASES: usize> ResourceLeaseManager<MAX_LEASES> {
    pub const fn new() -> Self {
        Self {
            leases: [None; MAX_LEASES],
        }
    }

    /// Finds the holder of a specific resource lease.
    pub fn holder(&self, kind: LeaseKind) -> Option<Pid> {
        for slot in self.leases.iter().flatten() {
            if slot.kind == kind {
                return Some(slot.owner);
            }
        }
        None
    }

    /// Check if a specific resource is currently leased.
    #[inline(always)]
    pub fn is_leased(&self, kind: LeaseKind) -> bool {
        self.holder(kind).is_some()
    }

    /// Acquires an exclusive timed lease on a resource.
    pub fn acquire(
        &mut self,
        owner: Pid,
        kind: LeaseKind,
        timeout_ms: u32,
        current_tick: u32,
    ) -> Result<(), LeaseError> {
        if let Some(holder) = self.holder(kind) {
            if holder == owner {
                // Refresh existing lease
                for slot in self.leases.iter_mut().flatten() {
                    if slot.kind == kind && slot.owner == owner {
                        slot.acquired_tick = current_tick;
                        slot.timeout_ms = timeout_ms;
                        return Ok(());
                    }
                }
            }
            return Err(LeaseError::AlreadyLeased { holder });
        }

        // Allocate empty lease slot
        for slot in self.leases.iter_mut() {
            if slot.is_none() {
                *slot = Some(ResourceLease {
                    kind,
                    owner,
                    acquired_tick: current_tick,
                    timeout_ms,
                });
                return Ok(());
            }
        }

        Err(LeaseError::LeaseTableFull)
    }

    /// Releases a previously acquired lease.
    pub fn release(&mut self, owner: Pid, kind: LeaseKind) -> Result<(), LeaseError> {
        for slot in self.leases.iter_mut() {
            if let Some(l) = slot {
                if l.kind == kind {
                    if l.owner == owner {
                        *slot = None;
                        return Ok(());
                    } else {
                        return Err(LeaseError::NotOwner);
                    }
                }
            }
        }
        Err(LeaseError::NotLeased)
    }

    /// Forcibly revokes all active leases held by `owner` upon process exit or fault.
    /// Returns the number of leases revoked.
    pub fn revoke_all_for_owner(&mut self, owner: Pid) -> u8 {
        let mut count = 0u8;
        for slot in self.leases.iter_mut() {
            if let Some(l) = slot {
                if l.owner == owner {
                    *slot = None;
                    count += 1;
                }
            }
        }
        count
    }

    /// Checks active leases against timeouts, invoking callback for expired leases.
    pub fn check_timeouts(
        &mut self,
        current_tick: u32,
        mut on_timeout: impl FnMut(LeaseKind, Pid),
    ) {
        for slot in self.leases.iter_mut() {
            if let Some(l) = slot {
                if l.timeout_ms > 0 {
                    let elapsed = current_tick.saturating_sub(l.acquired_tick);
                    if elapsed >= l.timeout_ms {
                        let kind = l.kind;
                        let owner = l.owner;
                        *slot = None;
                        on_timeout(kind, owner);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lease_acquire_and_release() {
        let mut mgr: ResourceLeaseManager<4> = ResourceLeaseManager::new();
        let pid1 = Pid(10);
        let pid2 = Pid(20);

        assert_eq!(mgr.holder(LeaseKind::RadioRx), None);
        assert!(mgr.acquire(pid1, LeaseKind::RadioRx, 1000, 50).is_ok());
        assert_eq!(mgr.holder(LeaseKind::RadioRx), Some(pid1));

        // pid2 cannot acquire already leased resource
        match mgr.acquire(pid2, LeaseKind::RadioRx, 1000, 60) {
            Err(LeaseError::AlreadyLeased { holder }) => assert_eq!(holder, pid1),
            _ => panic!("Expected AlreadyLeased error"),
        }

        // pid2 cannot release pid1's lease
        assert_eq!(
            mgr.release(pid2, LeaseKind::RadioRx),
            Err(LeaseError::NotOwner)
        );

        // pid1 releases
        assert!(mgr.release(pid1, LeaseKind::RadioRx).is_ok());
        assert_eq!(mgr.holder(LeaseKind::RadioRx), None);
    }

    #[test]
    fn test_lease_revocation_on_owner_exit() {
        let mut mgr: ResourceLeaseManager<4> = ResourceLeaseManager::new();
        let pid = Pid(5);

        assert!(mgr.acquire(pid, LeaseKind::RadioRx, 5000, 0).is_ok());
        assert!(mgr.acquire(pid, LeaseKind::DisplayCanvas, 5000, 0).is_ok());
        assert_eq!(mgr.revoke_all_for_owner(pid), 2);
        assert_eq!(mgr.holder(LeaseKind::RadioRx), None);
        assert_eq!(mgr.holder(LeaseKind::DisplayCanvas), None);
    }
}
