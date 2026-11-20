//! Capability identifiers and the 32-bit capability set.

/// Declares `Cap` once: each capability owns one bit of a `CapabilitySet`.
/// Bits must be contiguous from 0 so `Cap::from_bit` stays a range check.
macro_rules! caps {
    ( $( $(#[$doc:meta])* $name:ident = $bit:literal ),* $(,)? ) => {
        /// Standard OS capability identifiers for hardware, subsystems, and APIs.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u32)]
        pub enum Cap {
            $( $(#[$doc])* $name = 1 << $bit, )*
        }

        impl Cap {
            /// Number of defined capabilities (bits `0..COUNT`).
            pub const COUNT: u8 = [$($bit),*].len() as u8;
        }

        const _: () = {
            let bits: &[u8] = &[$($bit),*];
            let mut i = 0;
            while i < bits.len() {
                assert!(bits[i] as usize == i, "capability bits must be contiguous from 0");
                i += 1;
            }
        };

        /// Rust identifiers of every capability, indexed by bit.
        #[cfg(feature = "describe")]
        pub const CAP_IDENTS: &[&str] = &[ $( stringify!($name) ),* ];
    };
}

caps! {
    RadioRx = 0,
    RadioTx = 1,
    StorageRead = 2,
    StorageWrite = 3,
    AudioOutput = 4,
    DisplayDirect = 5,
    SerialComm = 6,
    UsbComm = 7,
    HardwareCrc = 8,
    RealTimeClock = 9,
    PowerManage = 10,
    CryptoAead = 11,
    RawPeripheral = 12,
    /// e.g. Si4732 HF / SW / SSB reception.
    BroadBandRx = 13,
    /// GPS / Galileo / BeiDou position fix.
    GnssLocation = 14,
    /// BLE NUS / SPP serial wireless programming.
    WirelessLink = 15,
    /// Microphone sampling, ADC audio input, SSTV/APRS demodulation.
    AudioCapture = 16,
    /// BK1080 broadcast FM tuner (64-108 MHz).
    BroadcastFm = 17,
    /// Keypad matrix, multi-key chording & input events.
    Keypad = 18,
    /// Process management, background tasks, hibernation.
    TaskControl = 19,
    /// Microkernel IPC, event bus pub/sub, service RPC.
    ServiceRegistry = 20,
    /// Low-level I2C / SPI direct bus access.
    BusAccess = 21,
    /// Direct hardware GPIO pin manipulation.
    GpioAccess = 22,
    /// Full filesystem management (mkdir, rename, unlink, partition operations).
    StorageAdmin = 23,
    /// Protected factory calibration read/write access (/sys/calib.*).
    FactoryCalib = 24,
}

impl Cap {
    /// Bit index of this capability inside a `CapabilitySet`.
    #[inline(always)]
    pub const fn bit(self) -> u8 {
        (self as u32).trailing_zeros() as u8
    }

    /// Capability owning `bit`, if one is defined.
    #[inline(always)]
    pub const fn from_bit(bit: u8) -> Option<Self> {
        if bit < Self::COUNT {
            // SAFETY: `caps!` asserts bits `0..COUNT` are all defined, and each
            // variant's discriminant is exactly `1 << bit`.
            Some(unsafe { core::mem::transmute::<u32, Cap>(1u32 << bit) })
        } else {
            None
        }
    }
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

    /// Returns the first required capability token in this set, if any.
    pub const fn first_cap(&self) -> Option<Cap> {
        if self.0 == 0 {
            return None;
        }
        Cap::from_bit(self.0.trailing_zeros() as u8)
    }

    pub fn insert(&mut self, kind: Cap) {
        self.0 |= kind as u32;
    }

    pub fn remove(&mut self, kind: Cap) {
        self.0 &= !(kind as u32);
    }

    pub fn iter(&self) -> CapIter {
        CapIter {
            mask: self.0,
            bit: 0,
        }
    }
}

pub struct CapIter {
    mask: u32,
    bit: u8,
}

impl Iterator for CapIter {
    type Item = Cap;

    fn next(&mut self) -> Option<Self::Item> {
        while self.bit < Cap::COUNT {
            let current_bit = self.bit;
            self.bit += 1;
            let flag = 1u32 << current_bit;
            if (self.mask & flag) != 0 {
                return Cap::from_bit(current_bit);
            }
        }
        None
    }
}
