//! Explicit Executable Memory Regions & Memory Paradigm Descriptors (§6, §9 / KRX-ABI 3.2).
//!
//! Replaces ambiguous heap models with explicit, typed memory regions
//! (`ImmutableCode`, `VirtualRam`, `PersistentState`, `IoBuffer`, `OverlayWindow`),
//! supporting fine-grained permission enforcement and fixed VMA verification.

/// Categorization of memory regions in an executable's address space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RegionKind {
    /// Immutable executable machine code or bytecode (.text).
    ImmutableCode = 0,
    /// Transient application SRAM (.data, .bss, stack).
    VirtualRam = 1,
    /// Checked persistent state window backed by KernFS (/pref/ or /user/).
    PersistentState = 2,
    /// Shared packet or streaming I/O buffer (MBuf / Audio buffer).
    IoBuffer = 3,
    /// Fixed execution window for P0 leaf overlays.
    OverlayWindow = 4,
}

impl RegionKind {
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::ImmutableCode),
            1 => Some(Self::VirtualRam),
            2 => Some(Self::PersistentState),
            3 => Some(Self::IoBuffer),
            4 => Some(Self::OverlayWindow),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ImmutableCode => "Code",
            Self::VirtualRam => "RAM",
            Self::PersistentState => "Persistent",
            Self::IoBuffer => "I/O",
            Self::OverlayWindow => "Overlay",
        }
    }
}

/// Bitmask permission flags for memory regions.
pub mod region_flags {
    pub const READ: u8 = 1 << 0;
    pub const WRITE: u8 = 1 << 1;
    pub const EXEC: u8 = 1 << 2;
    pub const PINNED: u8 = 1 << 3;
}

/// Standard 16-byte Memory Region Descriptor ($O(1)$ power-of-two geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct MemoryRegionDescriptor {
    pub kind: u8,
    pub flags: u8,
    pub reserved: u16,
    pub base_vma: u32,
    pub size_bytes: u32,
    pub alignment: u16,
    pub pad: [u8; 2],
}

const _: () = assert!(core::mem::size_of::<MemoryRegionDescriptor>() == 16);

impl MemoryRegionDescriptor {
    pub const SIZE: usize = 16;

    pub const fn new(kind: RegionKind, flags: u8, base_vma: u32, size_bytes: u32) -> Self {
        Self {
            kind: kind as u8,
            flags,
            reserved: 0,
            base_vma,
            size_bytes,
            alignment: 4,
            pad: [0u8; 2],
        }
    }

    #[inline(always)]
    pub const fn kind(&self) -> Option<RegionKind> {
        RegionKind::from_u8(self.kind)
    }

    #[inline(always)]
    pub const fn can_read(&self) -> bool {
        (self.flags & region_flags::READ) != 0
    }

    #[inline(always)]
    pub const fn can_write(&self) -> bool {
        (self.flags & region_flags::WRITE) != 0
    }

    #[inline(always)]
    pub const fn can_execute(&self) -> bool {
        (self.flags & region_flags::EXEC) != 0
    }

    #[inline(always)]
    pub const fn is_pinned(&self) -> bool {
        (self.flags & region_flags::PINNED) != 0
    }

    #[inline(always)]
    pub fn contains_addr(&self, addr: u32) -> bool {
        addr >= self.base_vma && addr < self.base_vma.saturating_add(self.size_bytes)
    }

    #[inline(always)]
    pub fn contains_range(&self, start: u32, len: u32) -> bool {
        if len == 0 {
            return self.contains_addr(start);
        }
        let end = match start.checked_add(len) {
            Some(e) => e,
            None => return false,
        };
        start >= self.base_vma && end <= self.base_vma.saturating_add(self.size_bytes)
    }
}

/// Bounded collection of memory region descriptors defining an application memory layout.
#[derive(Debug, Clone)]
pub struct MemoryMapLayout<const MAX_REGIONS: usize = 8> {
    pub regions: [MemoryRegionDescriptor; MAX_REGIONS],
    pub region_count: u8,
}

impl<const MAX_REGIONS: usize> Default for MemoryMapLayout<MAX_REGIONS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_REGIONS: usize> MemoryMapLayout<MAX_REGIONS> {
    pub const fn new() -> Self {
        Self {
            regions: [MemoryRegionDescriptor {
                kind: 0,
                flags: 0,
                reserved: 0,
                base_vma: 0,
                size_bytes: 0,
                alignment: 0,
                pad: [0u8; 2],
            }; MAX_REGIONS],
            region_count: 0,
        }
    }

    pub fn add_region(&mut self, region: MemoryRegionDescriptor) -> Result<(), ()> {
        if (self.region_count as usize) < MAX_REGIONS {
            self.regions[self.region_count as usize] = region;
            self.region_count += 1;
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn reset(&mut self) {
        self.region_count = 0;
    }

    /// Configure memory layout in-place for P0 Structured Paged applications (§5.1).
    #[inline(never)]
    pub fn init_p0_structured(
        &mut self,
        code_vma: u32,
        code_size: u32,
        overlay_vma: u32,
        overlay_size: u32,
        ram_vma: u32,
        ram_size: u32,
    ) {
        self.reset();
        let _ = self.add_region(MemoryRegionDescriptor::new(
            RegionKind::ImmutableCode,
            region_flags::READ | region_flags::EXEC,
            code_vma,
            code_size,
        ));
        let _ = self.add_region(MemoryRegionDescriptor::new(
            RegionKind::OverlayWindow,
            region_flags::READ | region_flags::EXEC,
            overlay_vma,
            overlay_size,
        ));
        let _ = self.add_region(MemoryRegionDescriptor::new(
            RegionKind::VirtualRam,
            region_flags::READ | region_flags::WRITE,
            ram_vma,
            ram_size,
        ));
    }

    /// Construct a memory layout for P0 Structured Paged applications (§5.1).
    pub fn for_p0_structured(
        code_vma: u32,
        code_size: u32,
        overlay_vma: u32,
        overlay_size: u32,
        ram_vma: u32,
        ram_size: u32,
    ) -> Self {
        let mut layout = Self::new();
        layout.init_p0_structured(
            code_vma,
            code_size,
            overlay_vma,
            overlay_size,
            ram_vma,
            ram_size,
        );
        layout
    }

    /// Configure memory layout in-place for N1 Native PIC applications (§6.1).
    #[inline(never)]
    pub fn init_native_pic(&mut self, code_vma: u32, code_size: u32, ram_vma: u32, ram_size: u32) {
        self.reset();
        let _ = self.add_region(MemoryRegionDescriptor::new(
            RegionKind::ImmutableCode,
            region_flags::READ | region_flags::EXEC,
            code_vma,
            code_size,
        ));
        let _ = self.add_region(MemoryRegionDescriptor::new(
            RegionKind::VirtualRam,
            region_flags::READ | region_flags::WRITE,
            ram_vma,
            ram_size,
        ));
    }

    /// Construct a memory layout for N1 Native PIC applications (§6.1).
    pub fn for_native_pic(code_vma: u32, code_size: u32, ram_vma: u32, ram_size: u32) -> Self {
        let mut layout = Self::new();
        layout.init_native_pic(code_vma, code_size, ram_vma, ram_size);
        layout
    }

    /// Find the first memory region matching a specific kind.
    pub fn find_region_by_kind(&self, kind: RegionKind) -> Option<&MemoryRegionDescriptor> {
        let target_kind = kind as u8;
        for i in 0..(self.region_count as usize) {
            if self.regions[i].kind == target_kind {
                return Some(&self.regions[i]);
            }
        }
        None
    }

    /// Find the memory region containing the specified VMA address.
    pub fn find_region_containing(&self, addr: u32) -> Option<&MemoryRegionDescriptor> {
        for i in 0..(self.region_count as usize) {
            if self.regions[i].contains_addr(addr) {
                return Some(&self.regions[i]);
            }
        }
        None
    }

    /// Validates whether an access at `[addr .. addr + len]` is permitted by the layout.
    pub fn validate_access(
        &self,
        addr: u32,
        len: u32,
        require_write: bool,
        require_exec: bool,
    ) -> bool {
        for i in 0..(self.region_count as usize) {
            let r = &self.regions[i];
            if r.contains_range(addr, len) {
                if require_write && !r.can_write() {
                    return false;
                }
                if require_exec && !r.can_execute() {
                    return false;
                }
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_descriptor_geometry_and_permissions() {
        assert_eq!(core::mem::size_of::<MemoryRegionDescriptor>(), 16);

        let code_reg = MemoryRegionDescriptor::new(
            RegionKind::ImmutableCode,
            region_flags::READ | region_flags::EXEC,
            0x2000_0000,
            2048,
        );
        assert!(code_reg.can_read());
        assert!(code_reg.can_execute());
        assert!(!code_reg.can_write());
        assert!(code_reg.contains_range(0x2000_0000, 2048));
        assert!(!code_reg.contains_range(0x2000_0800, 10));

        let ram_reg = MemoryRegionDescriptor::new(
            RegionKind::VirtualRam,
            region_flags::READ | region_flags::WRITE,
            0x2000_0800,
            1024,
        );
        assert!(ram_reg.can_read());
        assert!(ram_reg.can_write());
        assert!(!ram_reg.can_execute());

        let mut layout: MemoryMapLayout<4> = MemoryMapLayout::new();
        assert!(layout.add_region(code_reg).is_ok());
        assert!(layout.add_region(ram_reg).is_ok());

        assert!(layout.validate_access(0x2000_0100, 32, false, true));
        assert!(!layout.validate_access(0x2000_0100, 32, true, false));
        assert!(layout.validate_access(0x2000_0810, 64, true, false));
        assert!(!layout.validate_access(0x2000_0810, 64, false, true));

        // Test P0 structured layout builder
        let p0_layout: MemoryMapLayout<4> = MemoryMapLayout::for_p0_structured(
            0x2000_0000,
            2048,
            0x2000_0800,
            1024,
            0x2000_0C00,
            1024,
        );
        assert_eq!(p0_layout.region_count, 3);
        let overlay = p0_layout
            .find_region_by_kind(RegionKind::OverlayWindow)
            .unwrap();
        let base = overlay.base_vma;
        assert_eq!(base, 0x2000_0800);
        assert!(overlay.can_execute());

        let containing = p0_layout.find_region_containing(0x2000_0850).unwrap();
        assert_eq!(containing.kind, RegionKind::OverlayWindow as u8);
    }
}
