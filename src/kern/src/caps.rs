//! ZST Capability Tokens for high-assurance hardware access control.

/// Hardware Capability: Exclusive access to the Radio transceiver peripheral.
pub struct CapRadio {
    _private: (),
}

impl CapRadio {
    /// Safety: This must only be called ONCE during kernel initialization.
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Exclusive access to the SPI Flash peripheral.
pub struct CapSpiFlash {
    _private: (),
}

impl CapSpiFlash {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Exclusive access to the LCD Display peripheral.
pub struct CapDisplay {
    _private: (),
}

impl CapDisplay {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Exclusive access to the GPIO Keypad matrix.
pub struct CapKeypad {
    _private: (),
}

impl CapKeypad {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Exclusive access to the core System Timer (SysTick/RTC).
pub struct CapTimer {
    _private: (),
}

impl CapTimer {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Exclusive access to the Power Management Unit (PMU) and ADC.
pub struct CapPower {
    _private: (),
}

impl CapPower {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Exclusive access to Serial communications.
pub struct CapSerial {
    _private: (),
}

impl CapSerial {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

// ── Dynamic & Declarative Capability Set ────────────────────────────────────

/// Standard OS Capability identifiers for hardware, subsystems, and APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Cap {
    RadioRx = 1 << 0,
    RadioTx = 1 << 1,
    StorageRead = 1 << 2,
    StorageWrite = 1 << 3,
    AudioOutput = 1 << 4,
    DisplayDirect = 1 << 5,
    SerialComm = 1 << 6,
    UsbComm = 1 << 7,
    HardwareCrc = 1 << 8,
    RealTimeClock = 1 << 9,
    PowerManage = 1 << 10,
    CryptoAead = 1 << 11,
    RawPeripheral = 1 << 12,
    BroadBandRx = 1 << 13,  // e.g. Si4732 HF / SW / SSB reception
    GnssLocation = 1 << 14, // GPS / Galileo / BeiDou position fix
    WirelessLink = 1 << 15, // BLE NUS / SPP serial wireless programming
}

/// Compact 32-bit capability token set with zero-alloc O(1) bitwise operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapabilitySet(pub u32);

impl CapabilitySet {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(0xFFFF_FFFF);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_raw(mask: u32) -> Self {
        Self(mask)
    }

    pub const fn raw(&self) -> u32 {
        self.0
    }

    pub const fn with(self, kind: Cap) -> Self {
        Self(self.0 | (kind as u32))
    }

    pub const fn contains(&self, kind: Cap) -> bool {
        (self.0 & (kind as u32)) == (kind as u32)
    }

    pub const fn contains_mask(&self, mask: u32) -> bool {
        (self.0 & mask) == mask
    }

    pub const fn contains_all(&self, other: CapabilitySet) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn intersects(&self, other: CapabilitySet) -> bool {
        (self.0 & other.0) != 0
    }

    pub const fn union(self, other: CapabilitySet) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersection(self, other: CapabilitySet) -> Self {
        Self(self.0 & other.0)
    }

    pub const fn without(self, other: CapabilitySet) -> Self {
        Self(self.0 & !other.0)
    }

    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn insert(&mut self, kind: Cap) {
        self.0 |= kind as u32;
    }

    pub fn remove(&mut self, kind: Cap) {
        self.0 &= !(kind as u32);
    }
}

/// Minimalist capability check contract.
pub trait CapsCheck {
    fn caps(&self) -> CapabilitySet;

    #[inline(always)]
    fn has(&self, cap: Cap) -> bool {
        self.caps().contains(cap)
    }

    #[inline(always)]
    fn check(&self, cap: Cap) -> Result<(), CapabilitySet> {
        if self.has(cap) {
            Ok(())
        } else {
            Err(CapabilitySet::empty().with(cap))
        }
    }
}
