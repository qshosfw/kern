//! Uniform Capability & Service Registry (KRX-ABI 2.5.0 Chapter 8).
//!
//! Implements the three-tier service and capability binding broker:
//! 1. Preferred provider (`PIN_PROVIDER`)
//! 2. Kernel primitive registry (`0x0001..=0x000D`)
//! 3. Dynamic package service registry (`0x0100+` published by `.svc`/`.dmn`/`.app` actors)

use super::Pid;

/// Maximum number of concurrently registered dynamic service interfaces.
pub const MAX_REGISTERED_SERVICES: usize = 8;

/// Published service interface entry in the kernel Service Registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceEntry {
    pub interface_id: u16,
    pub abi_version: u16,
    pub provider_pid: Pid,
    pub vtable_ptr: usize,
    pub pinned: bool,
    pub client_count: u8,
}

impl ServiceEntry {
    pub const fn empty() -> Self {
        Self {
            interface_id: 0,
            abi_version: 0,
            provider_pid: Pid(0),
            vtable_ptr: 0,
            pinned: false,
            client_count: 0,
        }
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.interface_id != 0 && self.vtable_ptr != 0
    }
}

/// Zero-allocation Service Registry for package-provided and pinned capability interfaces.
pub struct ServiceRegistry<const N: usize = MAX_REGISTERED_SERVICES> {
    pub entries: [ServiceEntry; N],
}

impl<const N: usize> Default for ServiceRegistry<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ServiceRegistry<N> {
    pub const fn new() -> Self {
        Self {
            entries: [const { ServiceEntry::empty() }; N],
        }
    }

    /// Publish or update a service interface vtable in the registry.
    pub fn register(
        &mut self,
        interface_id: u16,
        abi_version: u16,
        provider_pid: Pid,
        vtable_ptr: usize,
        pinned: bool,
    ) -> bool {
        if interface_id == 0 || vtable_ptr == 0 {
            return false;
        }
        // Update existing entry if matching interface_id (and not pinned by another provider unless pinned=true)
        for slot in self.entries.iter_mut() {
            if slot.interface_id == interface_id {
                if slot.pinned && !pinned && slot.provider_pid != provider_pid {
                    return false;
                }
                slot.abi_version = abi_version;
                slot.provider_pid = provider_pid;
                slot.vtable_ptr = vtable_ptr;
                slot.pinned = pinned;
                return true;
            }
        }
        // Allocate free slot
        for slot in self.entries.iter_mut() {
            if !slot.is_active() {
                *slot = ServiceEntry {
                    interface_id,
                    abi_version,
                    provider_pid,
                    vtable_ptr,
                    pinned,
                    client_count: 0,
                };
                return true;
            }
        }
        false
    }

    /// Resolve an interface by `interface_id` and minimum ABI level `min_abi`.
    /// Returns the raw vtable pointer (`usize`) if found and compatible.
    pub fn resolve(&mut self, interface_id: u16, min_abi: u16) -> Option<usize> {
        // 1. Prefer pinned provider first
        for slot in self.entries.iter_mut() {
            if slot.is_active()
                && slot.pinned
                && slot.interface_id == interface_id
                && slot.abi_version >= min_abi
            {
                slot.client_count = slot.client_count.saturating_add(1);
                return Some(slot.vtable_ptr);
            }
        }
        // 2. Any active provider
        for slot in self.entries.iter_mut() {
            if slot.is_active() && slot.interface_id == interface_id && slot.abi_version >= min_abi
            {
                slot.client_count = slot.client_count.saturating_add(1);
                return Some(slot.vtable_ptr);
            }
        }
        None
    }

    /// Unregister a service interface by `interface_id`.
    pub fn release(&mut self, interface_id: u16) -> bool {
        for slot in self.entries.iter_mut() {
            if slot.is_active() && slot.interface_id == interface_id {
                *slot = ServiceEntry::empty();
                return true;
            }
        }
        false
    }

    /// Unbind all services owned by a terminated/quarantined actor (`provider_pid`).
    /// Returns the number of unbound interfaces so the kernel can emit `SERVICE_DIED`.
    pub fn unbind_provider(&mut self, provider_pid: Pid) -> usize {
        let mut removed = 0;
        for slot in self.entries.iter_mut() {
            if slot.is_active() && slot.provider_pid == provider_pid {
                *slot = ServiceEntry::empty();
                removed += 1;
            }
        }
        removed
    }

    /// Count of active published services.
    pub fn active_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_active()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_registry_pin_provider_and_reclaim() {
        let mut reg = ServiceRegistry::<4>::new();
        assert!(reg.register(0x0100, 1, Pid(5), 0x2000_1000, false));
        assert_eq!(reg.resolve(0x0100, 1), Some(0x2000_1000));
        // Higher min_abi fails
        assert_eq!(reg.resolve(0x0100, 2), None);

        // Pinned provider overrides unpinned provider
        assert!(reg.register(0x0100, 2, Pid(6), 0x2000_2000, true));
        assert_eq!(reg.resolve(0x0100, 1), Some(0x2000_2000));

        // Unbind provider Pid(6)
        assert_eq!(reg.unbind_provider(Pid(6)), 1);
        assert_eq!(reg.resolve(0x0100, 1), None);
    }

    #[test]
    fn test_service_registry_release() {
        let mut reg = ServiceRegistry::<4>::new();
        assert!(reg.register(0x0105, 1, Pid(1), 0x2000_3000, false));
        assert_eq!(reg.resolve(0x0105, 1), Some(0x2000_3000));
        assert_eq!(reg.active_count(), 1);

        assert!(reg.release(0x0105));
        assert_eq!(reg.resolve(0x0105, 1), None);
        assert_eq!(reg.active_count(), 0);

        // Releasing already released interface returns false
        assert!(!reg.release(0x0105));
    }
}
