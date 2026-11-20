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

/// Hardware Capability: Storage and non-volatile flash peripheral.
pub struct CapStorage {
    _private: (),
}

impl CapStorage {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Audio playback, sampling and amplifier peripheral.
pub struct CapAudio {
    _private: (),
}

impl CapAudio {
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

/// Hardware Capability: Exclusive access to Serial communications (UART/USB CDC).
pub struct CapSerial {
    _private: (),
}

impl CapSerial {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: UI modals, status strip, and layout orchestration.
pub struct CapUi {
    _private: (),
}

impl CapUi {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Accelerated math (hardware divider, CRC, fixed-point helpers).
pub struct CapMath {
    _private: (),
}

impl CapMath {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: BK1080 / broadcast FM tuner peripheral.
pub struct CapFm {
    _private: (),
}

impl CapFm {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Kernel Meta-Capability: Service registry, event bus, and RPC invocation.
pub struct CapService {
    _private: (),
}

impl CapService {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Direct chip register access (BK4829, BK1080, MCU registers).
pub struct CapRegister {
    _private: (),
}

impl CapRegister {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Low-level bus master access (I2C, SPI).
pub struct CapBus {
    _private: (),
}

impl CapBus {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Direct GPIO pin manipulation and inspection.
pub struct CapGpio {
    _private: (),
}

impl CapGpio {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

/// Hardware Capability: Master raw peripheral access (Registers, Bus, and GPIO).
pub struct CapHardware {
    _private: (),
}

impl CapHardware {
    pub const unsafe fn new() -> Self {
        Self { _private: () }
    }
}

// ── Capability Presentation & Checking ──────────────────────────────────────

use abi::{Cap, CapabilitySet};

const CAP_NAMES: &[u8] = b"\
Radio RX\0\
Radio TX\0\
Storage Read\0\
Storage Write\0\
Audio Output\0\
Display\0\
Serial UART\0\
USB CDC\0\
Hardware CRC\0\
Real-Time Clock\0\
Power Management\0\
Crypto AEAD\0\
Raw MMIO\0\
Broadband HF RX\0\
GNSS Location\0\
Wireless Link\0\
Audio Capture\0\
Broadcast FM\0\
Keypad\0\
Task Control\0\
Service Registry\0\
Bus Access\0\
GPIO Access\0\
Storage Admin\0\
Factory Calibration\0";

/// Human-readable capability name for dialogs and settings pages.
pub const fn name(cap: Cap) -> &'static str {
    utils::format::nth_cstr(CAP_NAMES, cap.bit())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_tokens() {
        let _r = unsafe { CapRadio::new() };
        let _s = unsafe { CapStorage::new() };
        let _a = unsafe { CapAudio::new() };
        let _d = unsafe { CapDisplay::new() };
        assert_eq!(name(Cap::RadioRx), "Radio RX");
        assert_eq!(name(Cap::BroadBandRx), "Broadband HF RX");
        assert_eq!(name(Cap::CryptoAead), "Crypto AEAD");
        assert_eq!(name(Cap::StorageAdmin), "Storage Admin");
        assert_eq!(name(Cap::FactoryCalib), "Factory Calibration");
        let admin_set = CapabilitySet::empty().with(Cap::StorageAdmin);
        assert_eq!(admin_set.first_cap(), Some(Cap::StorageAdmin));
        let calib_set = CapabilitySet::empty().with(Cap::FactoryCalib);
        assert_eq!(calib_set.first_cap(), Some(Cap::FactoryCalib));
    }

    #[test]
    fn test_capability_set_bitwise_ops() {
        let mut set = CapabilitySet::empty();
        assert!(set.is_empty());
        assert!(!set.contains(Cap::RadioRx));

        set = set.with(Cap::RadioRx).with(Cap::DisplayDirect);
        assert!(!set.is_empty());
        assert!(set.contains(Cap::RadioRx));
        assert!(set.contains(Cap::DisplayDirect));
        assert!(!set.contains(Cap::RadioTx));

        let subset = CapabilitySet::empty().with(Cap::RadioRx);
        assert!(set.contains_all(subset));
        assert!(set.intersects(subset));

        let disjoint = CapabilitySet::empty().with(Cap::AudioCapture);
        assert!(!set.contains_all(disjoint));
        assert!(!set.intersects(disjoint));

        let union = set.union(disjoint);
        assert!(union.contains(Cap::RadioRx));
        assert!(union.contains(Cap::AudioCapture));

        let inter = union.intersection(set);
        assert_eq!(inter, set);

        let diff = union.without(set);
        assert_eq!(diff, disjoint);

        let mut mod_set = CapabilitySet::empty();
        mod_set.insert(Cap::StorageRead);
        assert!(mod_set.contains(Cap::StorageRead));
        mod_set.remove(Cap::StorageRead);
        assert!(!mod_set.contains(Cap::StorageRead));
    }

    #[test]
    fn test_capability_set_iterator() {
        let set = CapabilitySet::empty()
            .with(Cap::RadioRx)
            .with(Cap::RadioTx)
            .with(Cap::AudioOutput);

        let collected: heapless::Vec<Cap, 4> = set.iter().collect();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], Cap::RadioRx);
        assert_eq!(collected[1], Cap::RadioTx);
        assert_eq!(collected[2], Cap::AudioOutput);
    }

    #[test]
    fn test_first_cap_and_required_message() {
        let empty = CapabilitySet::empty();
        assert_eq!(empty.first_cap(), None);

        let set = CapabilitySet::empty()
            .with(Cap::BroadBandRx)
            .with(Cap::GnssLocation);
        assert_eq!(set.first_cap(), Some(Cap::BroadBandRx));
        assert_eq!(name(Cap::BroadBandRx), "Broadband HF RX");

        let single = CapabilitySet::empty().with(Cap::BroadcastFm);
        assert_eq!(single.first_cap(), Some(Cap::BroadcastFm));
        assert_eq!(name(Cap::BroadcastFm), "Broadcast FM");
    }

    struct MockSecurityContext {
        caps: CapabilitySet,
    }

    impl CapsCheck for MockSecurityContext {
        fn caps(&self) -> CapabilitySet {
            self.caps
        }
    }

    #[test]
    fn test_caps_check_contract() {
        let ctx = MockSecurityContext {
            caps: CapabilitySet::empty().with(Cap::SerialComm),
        };
        assert!(ctx.has(Cap::SerialComm));
        assert!(!ctx.has(Cap::UsbComm));
        assert_eq!(ctx.check(Cap::SerialComm), Ok(()));
        assert_eq!(
            ctx.check(Cap::UsbComm),
            Err(CapabilitySet::empty().with(Cap::UsbComm))
        );
    }
}
