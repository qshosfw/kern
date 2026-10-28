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

use crate::caps::{Cap, CapabilitySet};

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
    pub caps: CapabilitySet,
    pub status: DeviceStatus,
}

impl DeviceDescriptor {
    pub const fn new(dev_type: DeviceType, name: &'static str, caps: CapabilitySet) -> Self {
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
    pub const fn active_capabilities() -> CapabilitySet {
        let mut caps = 0u32;

        #[cfg(feature = "radio")]
        {
            caps |= (Cap::RadioTx as u32) | (Cap::RadioRx as u32);
        }

        #[cfg(feature = "usb")]
        {
            caps |= Cap::UsbComm as u32;
        }

        #[cfg(feature = "secure_fs")]
        {
            caps |= Cap::CryptoAead as u32;
        }

        caps |= Cap::HardwareCrc as u32;
        caps |= Cap::StorageWrite as u32;

        CapabilitySet(caps)
    }
}
