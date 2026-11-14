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
    BroadBandRx = 1 << 13,     // e.g. Si4732 HF / SW / SSB reception
    GnssLocation = 1 << 14,    // GPS / Galileo / BeiDou position fix
    WirelessLink = 1 << 15,    // BLE NUS / SPP serial wireless programming
    AudioCapture = 1 << 16,    // Microphone sampling, ADC audio input, SSTV/APRS demodulation
    BroadcastFm = 1 << 17,     // BK1080 broadcast FM tuner (64-108 MHz)
    Keypad = 1 << 18,          // Keypad matrix, multi-key chording & input events
    TaskControl = 1 << 19,     // Process management, background tasks, hibernation
    ServiceRegistry = 1 << 20, // Microkernel IPC, event bus pub/sub, service RPC
    BusAccess = 1 << 21,       // Low-level I2C / SPI direct bus access
    GpioAccess = 1 << 22,      // Direct hardware GPIO pin manipulation
    StorageAdmin = 1 << 23, // Full filesystem management (mkdir, rename, unlink, partition operations)
    FactoryCalib = 1 << 24, // Protected factory calibration read/write access (/sys/calib.*)
}

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

impl Cap {
    pub const fn name(&self) -> &'static str {
        utils::format::nth_cstr(CAP_NAMES, (*self as u32).trailing_zeros() as u8)
    }

    pub const fn required_message(&self) -> &'static str {
        self.name()
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
        let tz = self.0.trailing_zeros();
        match 1u32 << tz {
            1 => Some(Cap::RadioRx),
            2 => Some(Cap::RadioTx),
            4 => Some(Cap::StorageRead),
            8 => Some(Cap::StorageWrite),
            16 => Some(Cap::AudioOutput),
            32 => Some(Cap::DisplayDirect),
            64 => Some(Cap::SerialComm),
            128 => Some(Cap::UsbComm),
            256 => Some(Cap::HardwareCrc),
            512 => Some(Cap::RealTimeClock),
            1024 => Some(Cap::PowerManage),
            2048 => Some(Cap::CryptoAead),
            4096 => Some(Cap::RawPeripheral),
            8192 => Some(Cap::BroadBandRx),
            16384 => Some(Cap::GnssLocation),
            32768 => Some(Cap::WirelessLink),
            65536 => Some(Cap::AudioCapture),
            131072 => Some(Cap::BroadcastFm),
            262144 => Some(Cap::Keypad),
            524288 => Some(Cap::TaskControl),
            1048576 => Some(Cap::ServiceRegistry),
            2097152 => Some(Cap::BusAccess),
            4194304 => Some(Cap::GpioAccess),
            8388608 => Some(Cap::StorageAdmin),
            16777216 => Some(Cap::FactoryCalib),
            _ => None,
        }
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
        while self.bit < 25 {
            let current_bit = self.bit;
            self.bit += 1;
            let flag = 1u32 << current_bit;
            if (self.mask & flag) != 0 {
                return match flag {
                    1 => Some(Cap::RadioRx),
                    2 => Some(Cap::RadioTx),
                    4 => Some(Cap::StorageRead),
                    8 => Some(Cap::StorageWrite),
                    16 => Some(Cap::AudioOutput),
                    32 => Some(Cap::DisplayDirect),
                    64 => Some(Cap::SerialComm),
                    128 => Some(Cap::UsbComm),
                    256 => Some(Cap::HardwareCrc),
                    512 => Some(Cap::RealTimeClock),
                    1024 => Some(Cap::PowerManage),
                    2048 => Some(Cap::CryptoAead),
                    4096 => Some(Cap::RawPeripheral),
                    8192 => Some(Cap::BroadBandRx),
                    16384 => Some(Cap::GnssLocation),
                    32768 => Some(Cap::WirelessLink),
                    65536 => Some(Cap::AudioCapture),
                    131072 => Some(Cap::BroadcastFm),
                    262144 => Some(Cap::Keypad),
                    524288 => Some(Cap::TaskControl),
                    1048576 => Some(Cap::ServiceRegistry),
                    2097152 => Some(Cap::BusAccess),
                    4194304 => Some(Cap::GpioAccess),
                    8388608 => Some(Cap::StorageAdmin),
                    16777216 => Some(Cap::FactoryCalib),
                    _ => None,
                };
            }
        }
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_tokens() {
        let _r = unsafe { CapRadio::new() };
        let _s = unsafe { CapStorage::new() };
        let _a = unsafe { CapAudio::new() };
        let _d = unsafe { CapDisplay::new() };
        assert_eq!(Cap::RadioRx.name(), "Radio RX");
        assert_eq!(Cap::BroadBandRx.name(), "Broadband HF RX");
        assert_eq!(Cap::CryptoAead.name(), "Crypto AEAD");
        assert_eq!(Cap::StorageAdmin.name(), "Storage Admin");
        assert_eq!(Cap::FactoryCalib.name(), "Factory Calibration");
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
        assert_eq!(Cap::BroadBandRx.required_message(), "Broadband HF RX");

        let single = CapabilitySet::empty().with(Cap::BroadcastFm);
        assert_eq!(single.first_cap(), Some(Cap::BroadcastFm));
        assert_eq!(Cap::BroadcastFm.required_message(), "Broadcast FM");
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
