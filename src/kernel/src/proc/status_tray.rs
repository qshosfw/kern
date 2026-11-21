//! Microkernel Status Tray Registry
//!
//! Provides zero-alloc, capability-checked registration of dynamic status icons
//! displayed on the OS status bar for foreground apps, background daemons,
//! and system services.

use crate::proc::Pid;
use crate::ui::{Icon, StatusGlyph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraySlot {
    pub owner: Pid,
    pub glyph: StatusGlyph,
}

pub struct StatusTray<const N: usize = 4> {
    count: usize,
    owners: [Pid; N],
    glyphs: [StatusGlyph; N],
}

impl<const N: usize> StatusTray<N> {
    pub const fn new() -> Self {
        Self {
            count: 0,
            owners: [Pid(0); N],
            glyphs: [StatusGlyph::default_builtin(); N],
        }
    }

    /// Register or update a status glyph (built-in or custom 5x5 bitmap) for owner.
    pub fn set_glyph(&mut self, owner: Pid, glyph: StatusGlyph) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.owners[i] == owner {
                self.glyphs[i] = glyph;
                return true;
            }
            i += 1;
        }
        if self.count < N {
            self.owners[self.count] = owner;
            self.glyphs[self.count] = glyph;
            self.count += 1;
            return true;
        }
        false
    }

    /// Register or update a standard built-in status icon for the specified owner process.
    pub fn set_icon(&mut self, owner: Pid, icon: Icon) -> bool {
        self.set_glyph(owner, StatusGlyph::Builtin(icon))
    }

    /// Register or update a custom 5x5 monochromatic bitmap icon for the specified owner process.
    pub fn set_custom_icon(&mut self, owner: Pid, bmp: [u8; 5]) -> bool {
        self.set_glyph(owner, StatusGlyph::Custom(bmp))
    }

    /// Clear the tray icon for the specified owner process.
    pub fn clear_icon(&mut self, owner: Pid) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.owners[i] == owner {
                self.count -= 1;
                while i < self.count {
                    self.owners[i] = self.owners[i + 1];
                    self.glyphs[i] = self.glyphs[i + 1];
                    i += 1;
                }
                return true;
            }
            i += 1;
        }
        false
    }

    /// Get the active glyph for an owner process.
    pub fn get_glyph(&self, owner: Pid) -> Option<StatusGlyph> {
        let mut i = 0;
        while i < self.count {
            if self.owners[i] == owner {
                return Some(self.glyphs[i]);
            }
            i += 1;
        }
        None
    }

    /// Get the active built-in icon for an owner process, if it is a built-in icon.
    pub fn get_icon(&self, owner: Pid) -> Option<Icon> {
        self.get_glyph(owner).and_then(|g| match g {
            StatusGlyph::Builtin(ic) => Some(ic),
            StatusGlyph::Custom(_) => None,
        })
    }

    /// Returns a slice of all currently active tray glyphs.
    #[inline(always)]
    pub fn active_glyphs(&self) -> &[StatusGlyph] {
        &self.glyphs[..self.count]
    }

    /// Returns the number of currently active tray icons.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns true if the tray has no active icons.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clear all registered tray icons.
    pub fn clear_all(&mut self) {
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_tray_builtin_and_custom_glyphs() {
        let mut tray = StatusTray::<4>::new();
        assert!(tray.is_empty());

        // Process 1 (Daemon) registers builtin Beacon icon
        let pid1 = Pid(1);
        assert!(tray.set_icon(pid1, Icon::Beacon));
        assert_eq!(tray.len(), 1);
        assert_eq!(tray.get_icon(pid1), Some(Icon::Beacon));

        // Process 2 (Background Service) registers custom 5x5 icon
        let pid2 = Pid(2);
        let custom_bmp: [u8; 5] = [0x1F, 0x11, 0x11, 0x11, 0x1F];
        assert!(tray.set_custom_icon(pid2, custom_bmp));
        assert_eq!(tray.len(), 2);
        assert_eq!(tray.get_glyph(pid2), Some(StatusGlyph::Custom(custom_bmp)));
        assert_eq!(tray.get_icon(pid2), None); // Custom is not a builtin icon

        // Verify active glyphs list order and contents
        let active = tray.active_glyphs();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0], StatusGlyph::Builtin(Icon::Beacon));
        assert_eq!(active[1], StatusGlyph::Custom(custom_bmp));

        // Background update: Process 1 changes from Beacon to Broadcast
        assert!(tray.set_icon(pid1, Icon::Broadcast));
        assert_eq!(tray.len(), 2);
        assert_eq!(tray.get_icon(pid1), Some(Icon::Broadcast));

        // Process 1 finishes and clears icon
        assert!(tray.clear_icon(pid1));
        assert_eq!(tray.len(), 1);
        assert_eq!(tray.get_icon(pid1), None);
        // Process 2's custom icon remains intact!
        assert_eq!(tray.get_glyph(pid2), Some(StatusGlyph::Custom(custom_bmp)));
    }

    #[test]
    fn test_status_tray_saturation() {
        let mut tray = StatusTray::<2>::new();
        assert!(tray.set_icon(Pid(10), Icon::Scan));
        assert!(tray.set_icon(Pid(11), Icon::Lock));
        // Tray with N=2 should reject 3rd process
        assert!(!tray.set_icon(Pid(12), Icon::Antenna));
        assert_eq!(tray.len(), 2);
    }
}
