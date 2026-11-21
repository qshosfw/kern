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

use ::abi::{Cap, CapabilitySet};

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

/// FourCC format identifier for `/sys/hw.cfg` (`"HWCF"`).
pub const HW_MANIFEST_IDENT: [u8; 4] = *b"HWCF";

/// Hardware tuner presence bitmask inside [`HwManifest::tuner_mask`].
pub mod tuner_flag {
    pub const BK4829: u16 = 1 << 0;
    pub const BK1080: u16 = 1 << 1;
    pub const SI4732: u16 = 1 << 2;
    pub const PA_10W: u16 = 1 << 3;
    pub const GNSS: u16 = 1 << 4;
    pub const BLE_UART: u16 = 1 << 5;
}

/// 32-byte ($2^5$) CRC32-protected hardware revision manifest stored in `/sys/hw.cfg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct HwManifest {
    pub ident: [u8; 4],
    pub board_id: u16,
    pub hw_rev: u16,
    pub flash_kb: u16,
    pub sram_kb: u16,
    pub tuner_mask: u16,
    pub bus_mask: u16,
    pub caps: u32,
    pub reserved: [u8; 8],
    pub crc32: u32,
}

const _: () = assert!(core::mem::size_of::<HwManifest>() == 32);

impl HwManifest {
    pub const fn new(
        board_id: u16,
        hw_rev: u16,
        flash_kb: u16,
        sram_kb: u16,
        tuner_mask: u16,
        bus_mask: u16,
        caps: CapabilitySet,
    ) -> Self {
        Self {
            ident: HW_MANIFEST_IDENT,
            board_id,
            hw_rev,
            flash_kb,
            sram_kb,
            tuner_mask,
            bus_mask,
            caps: caps.raw(),
            reserved: [0; 8],
            crc32: 0,
        }
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        unsafe { &*(self as *const Self as *const [u8; 32]) }
    }

    pub fn seal(&mut self) {
        self.ident = HW_MANIFEST_IDENT;
        self.crc32 = crypto::crc::CrcEngine::crc32(&self.as_bytes()[..28]);
    }

    pub fn is_valid(&self) -> bool {
        self.ident == HW_MANIFEST_IDENT
            && self.crc32 == crypto::crc::CrcEngine::crc32(&self.as_bytes()[..28])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hw_manifest_seals_and_detects_tamper() {
        let mut m = HwManifest::new(
            1,
            1,
            2048,
            16,
            tuner_flag::BK4829 | tuner_flag::BK1080,
            0x0F,
            DeviceRegistry::active_capabilities(),
        );
        assert!(!m.is_valid());
        m.seal();
        assert!(m.is_valid());
        m.tuner_mask |= tuner_flag::SI4732;
        assert!(!m.is_valid());
    }
}
