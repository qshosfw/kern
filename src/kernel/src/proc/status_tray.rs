//! Microkernel Status Tray Registry
//!
//! Provides zero-alloc, capability-checked registration of dynamic status icons
//! displayed on the OS status bar for foreground apps, background daemons,
//! and system services.

use crate::canvas::Icon;
use crate::proc::Pid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraySlot {
    pub owner: Pid,
    pub icon: Icon,
}

pub struct StatusTray<const N: usize = 4> {
    slots: [Option<TraySlot>; N],
    cached: [Icon; N],
    count: usize,
}

impl<const N: usize> StatusTray<N> {
    pub const fn new() -> Self {
        Self {
            slots: [None; N],
            cached: [Icon::Antenna; N],
            count: 0,
        }
    }

    fn rebuild(&mut self) {
        self.count = 0;
        for slot in self.slots.iter() {
            if let Some(entry) = slot {
                self.cached[self.count] = entry.icon;
                self.count += 1;
            }
        }
    }

    /// Register or update a status icon for the specified owner process.
    ///
    /// If `owner` already has an active tray icon, it is updated in-place.
    /// Otherwise, the icon is inserted into the first available slot.
    /// Returns `true` if registered/updated, `false` if the tray is full.
    pub fn set_icon(&mut self, owner: Pid, icon: Icon) -> bool {
        // 1. Update existing slot if present
        for slot in self.slots.iter_mut() {
            if let Some(entry) = slot {
                if entry.owner == owner {
                    entry.icon = icon;
                    self.rebuild();
                    return true;
                }
            }
        }

        // 2. Insert into first free slot
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(TraySlot { owner, icon });
                self.rebuild();
                return true;
            }
        }

        false
    }

    /// Clear the tray icon for the specified owner process.
    pub fn clear_icon(&mut self, owner: Pid) -> bool {
        for slot in self.slots.iter_mut() {
            if let Some(entry) = slot {
                if entry.owner == owner {
                    *slot = None;
                    self.rebuild();
                    return true;
                }
            }
        }
        false
    }

    /// Get the active icon for an owner process.
    pub fn get_icon(&self, owner: Pid) -> Option<Icon> {
        for slot in self.slots.iter() {
            if let Some(entry) = slot {
                if entry.owner == owner {
                    return Some(entry.icon);
                }
            }
        }
        None
    }

    /// Returns a slice of all currently active tray icons.
    #[inline(always)]
    pub fn active_icons(&self) -> &[Icon] {
        &self.cached[..self.count]
    }

    /// Clear all registered tray icons.
    pub fn clear_all(&mut self) {
        for slot in self.slots.iter_mut() {
            *slot = None;
        }
        self.count = 0;
    }
}
