//! Unified Kernel Device Registry & Capability Broker.
//!
//! Provides a type-safe abstraction harness for applications to discover,
//! acquire, and interact with the hardware peripherals of the PY32F071 MCU.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Radio,
    BroadBandReceiver, // e.g. Si4732 / Si4735 HF / SW / SSB
    Audio,
    Display,
    Storage,
    Serial,
    Usb,
    Gnss,     // GPS / Galileo / Beidou NMEA engine
    Wireless, // BLE NUS / Bluetooth SPP
    Crc,
    Rtc,
    Watchdog,
    InternalFlash,
    SystemInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceCapabilities(pub u32);

impl DeviceCapabilities {
    pub const CAP_NONE: u32 = 0;
    pub const CAP_RF_TX: u32 = 1 << 0;
    pub const CAP_RF_RX: u32 = 1 << 1;
    pub const CAP_HW_CRC: u32 = 1 << 2;
    pub const CAP_RTC: u32 = 1 << 3;
    pub const CAP_FLASH_PROGRAM: u32 = 1 << 4;
    pub const CAP_USB_CDC: u32 = 1 << 5;
    pub const CAP_USB_MSC: u32 = 1 << 6;
    pub const CAP_CRYPTO_AEAD: u32 = 1 << 7;
    pub const CAP_DMA_DISPLAY: u32 = 1 << 8;
    pub const CAP_BROADBAND_RX: u32 = 1 << 13;
    pub const CAP_GNSS: u32 = 1 << 14;
    pub const CAP_WIRELESS: u32 = 1 << 15;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(&self, cap: u32) -> bool {
        (self.0 & cap) == cap
    }

    pub fn insert(&mut self, cap: u32) {
        self.0 |= cap;
    }

    pub const fn to_capability_set(&self) -> crate::caps::CapabilitySet {
        crate::caps::CapabilitySet(self.0)
    }

    pub const fn from_capability_set(set: crate::caps::CapabilitySet) -> Self {
        Self(set.0)
    }
}

impl From<crate::caps::CapabilitySet> for DeviceCapabilities {
    fn from(set: crate::caps::CapabilitySet) -> Self {
        Self(set.0)
    }
}

impl From<DeviceCapabilities> for crate::caps::CapabilitySet {
    fn from(caps: DeviceCapabilities) -> Self {
        Self(caps.0)
    }
}

/// Device Status descriptor reported by hardware drivers to the kernel harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    Uninitialized,
    Ready,
    Busy,
    Error,
    Suspended,
}

/// Hardware communication bus used by concrete drivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    Internal,
    I2c { address: u8 },
    Spi { cs_pin: u8 },
    Uart { port: u8 },
    Usb,
}

/// Zero-cost declarative Driver contract.
pub trait Driver: 'static {
    fn descriptor(&self) -> DeviceDescriptor;
    fn bus(&self) -> BusType;
    fn init(&mut self) -> Result<(), ()>;
}

/// Information descriptor for registered hardware devices.
#[derive(Debug, Clone, Copy)]
pub struct DeviceDescriptor {
    pub dev_type: DeviceType,
    pub name: &'static str,
    pub caps: DeviceCapabilities,
    pub status: DeviceStatus,
}

impl DeviceDescriptor {
    pub const fn new(dev_type: DeviceType, name: &'static str, caps: DeviceCapabilities) -> Self {
        Self {
            dev_type,
            name,
            caps,
            status: DeviceStatus::Ready,
        }
    }
}

/// Compile-time & run-time device registry for the Kern microkernel.
pub struct DeviceRegistry;

impl DeviceRegistry {
    /// Returns the global capability bitmask supported by the active firmware build.
    pub const fn active_capabilities() -> DeviceCapabilities {
        let mut caps = DeviceCapabilities(0);

        #[cfg(feature = "radio")]
        {
            caps.0 |= DeviceCapabilities::CAP_RF_TX | DeviceCapabilities::CAP_RF_RX;
        }

        #[cfg(feature = "usb")]
        {
            caps.0 |= DeviceCapabilities::CAP_USB_CDC;
        }

        #[cfg(feature = "secure_vfs")]
        {
            caps.0 |= DeviceCapabilities::CAP_CRYPTO_AEAD;
        }

        caps.0 |= DeviceCapabilities::CAP_HW_CRC;
        caps.0 |= DeviceCapabilities::CAP_FLASH_PROGRAM;

        caps
    }
}
