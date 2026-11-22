//! Zero-cost `kernel::dev::Driver` trait implementations for hardware peripheral drivers.
//!
//! Enables declarative peripheral discovery and capability inspection without adding
//! runtime overhead or flash bloat when unused.

use kernel::caps::{Cap, CapabilitySet};
use kernel::dev::{BusType, DeviceDescriptor, DeviceType, Driver};

impl Driver for crate::keyboard::KeypadScanner {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(
            DeviceType::SystemInfo,
            "Keypad",
            CapabilitySet::NONE.with(Cap::Keypad),
        )
    }

    fn bus(&self) -> BusType {
        BusType::Internal
    }

    fn init(&mut self) -> Result<(), ()> {
        self.clear();
        Ok(())
    }
}

impl Driver for crate::battery::BatteryDriver {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(DeviceType::SystemInfo, "Battery", CapabilitySet::NONE)
    }

    fn bus(&self) -> BusType {
        BusType::Internal
    }

    fn init(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl<P: 'static> Driver for crate::audio::AudioAmplifier<P> {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(
            DeviceType::Audio,
            "AudioPA",
            CapabilitySet::NONE.with(Cap::AudioOutput),
        )
    }

    fn bus(&self) -> BusType {
        BusType::Internal
    }

    fn init(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(feature = "display")]
impl<B: 'static> Driver for crate::st7565::St7565<B> {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(
            DeviceType::Display,
            "ST7565",
            CapabilitySet::NONE.with(Cap::DisplayDirect),
        )
    }

    fn bus(&self) -> BusType {
        BusType::Spi { cs_pin: 0 }
    }

    fn init(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(feature = "filesystem")]
impl<B: 'static> Driver for crate::py25q16::Py25q16FlashDriver<B> {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(
            DeviceType::Storage,
            "PY25Q16",
            CapabilitySet::NONE
                .with(Cap::StorageRead)
                .with(Cap::StorageWrite),
        )
    }

    fn bus(&self) -> BusType {
        BusType::Spi { cs_pin: 1 }
    }

    fn init(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(feature = "radio")]
impl<B: 'static, State: 'static> Driver for crate::bk4829::Bk4829<'static, B, State> {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(
            DeviceType::Radio,
            "BK4829",
            CapabilitySet::NONE.with(Cap::RadioRx).with(Cap::RadioTx),
        )
    }

    fn bus(&self) -> BusType {
        BusType::I2c { address: 0x00 }
    }

    fn init(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(feature = "fm_radio")]
impl<I2C: 'static, D: 'static> Driver for crate::bk1080::Bk1080<I2C, D> {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(
            DeviceType::BroadBandReceiver,
            "BK1080",
            CapabilitySet::NONE.with(Cap::BroadBandRx),
        )
    }

    fn bus(&self) -> BusType {
        BusType::I2c { address: 0x40 }
    }

    fn init(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(feature = "usb")]
impl Driver for crate::usb::UsbDriver {
    fn descriptor(&self) -> DeviceDescriptor {
        DeviceDescriptor::new(
            DeviceType::Usb,
            "USB-FS",
            CapabilitySet::NONE.with(Cap::UsbComm).with(Cap::SerialComm),
        )
    }

    fn bus(&self) -> BusType {
        BusType::Usb
    }

    fn init(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_drivers_expose_descriptors_and_capabilities() {
        let kp = crate::keyboard::KeypadScanner::new();
        assert_eq!(kp.descriptor().name, "Keypad");
        assert!(kp.descriptor().caps.contains(Cap::Keypad));
        assert_eq!(kp.bus(), BusType::Internal);

        let bat = crate::battery::BatteryDriver::new();
        assert_eq!(bat.descriptor().name, "Battery");
    }
}
