//! High-assurance, zero-allocation Microkernel Service Contracts.
//!
//! Defines the formal microkernel service traits and hardware abstraction interfaces.
//! Concrete implementations are provided by drivers or system subsystems, while
//! applications, framework components, and loadable modules consume these contracts.

pub mod radio;
pub use radio::*;

// ── Audio & Tone Services ───────────────────────────────────────────────────

/// Priority tier for the audio arbitration pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioPriority {
    Idle = 0,
    Mute = 1, // Tuning, scanning, or FSK digital packet RX
    BackgroundVoice = 2,
    RxVoice = 3,        // Active carrier RF voice
    UiTone = 4,         // Keypress clicks, menu hints
    TxRoger = 5,        // End-of-transmission tone burst
    EmergencyAlert = 6, // Alarm, low battery alert
}

/// Destination for routed audio output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioOutputTarget {
    Speaker,
    Earpiece,
    RfTxModulator,
}

/// Declarative interface for Audio Amplification & Tone Subsystem.
pub trait AudioService {
    fn set_volume(&mut self, level: u8);
    fn volume(&self) -> u8;
    fn enable(&mut self);
    fn disable(&mut self);
    fn is_enabled(&self) -> bool;
    fn is_active(&self) -> bool;
    fn hardware_gain(&self) -> (u8, u8);
}

/// Audio Session & Priority Arbitrator.
pub trait AudioSession {
    fn current_priority(&self) -> AudioPriority;
    fn request_session(&mut self, priority: AudioPriority, target: AudioOutputTarget) -> bool;
    fn release_session(&mut self, priority: AudioPriority);
    fn is_speaker_active(&self) -> bool;
}

/// Declarative interface for Audio ADC sampling & microphone capture.
pub trait AudioCaptureService {
    fn start_capture(&mut self, rate_hz: u16) -> Result<(), ()>;
    fn read_samples(&mut self, out: &mut [i16]) -> usize;
    fn stop_capture(&mut self);
    fn is_capturing(&self) -> bool;
}

// ── Power & Sensor Services ─────────────────────────────────────────────────

/// Declarative interface for Power, Battery and MCU Sensor services.
pub trait PowerService {
    fn battery_percentage(&self) -> u8;
    fn battery_voltage_mv(&self) -> u16;
    fn battery_current_ma(&self) -> u16;
    fn battery_type(&self) -> u8 {
        0
    }
    fn uptime_secs(&self) -> u32;
    fn raw_battery_adc(&self) -> u16 {
        0
    }
    fn mcu_temperature_deci_c(&self) -> i16 {
        250
    }
    fn vdda_voltage_mv(&self) -> u16 {
        3300
    }
    fn is_charging(&self) -> bool {
        false
    }
    fn is_low(&self) -> bool {
        false
    }
    fn is_critical(&self) -> bool {
        false
    }
    fn poll_charging_transition(&mut self) -> Option<bool> {
        None
    }
    fn poll_low_battery_transition(&mut self) -> bool {
        false
    }
    fn set_battery_type(&mut self, _bat_type: u8) {}
    fn force_refresh(&mut self) {}
    fn power_state(&self) -> crate::power::PowerState {
        crate::power::PowerState::Active
    }
    fn power_profile(&self) -> crate::power::PowerProfile {
        crate::power::PowerProfile::Balanced
    }
    fn inactivity_secs(&self) -> u32 {
        0
    }
}

// ── Display & Visual Feedback Services ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklightMode {
    Off,
    Auto(u16), // Milliseconds
    On,
}

/// Declarative interface for LCD Display & Backlight hardware control.
pub trait DisplayService {
    fn set_contrast(&mut self, contrast: u8);
    fn set_inverted(&mut self, inverted: bool);
    fn set_backlight_mode(&mut self, mode: BacklightMode);
    fn set_backlight_brightness(&mut self, max: u8, min: u8);
    fn set_keypad_light_mode(&mut self, mode: u8);
    fn wake(&mut self);
    fn set_shading_mode(&mut self, _mode: u8) {}
    fn show_progress(&mut self, title: &str, subtitle: &str, progress: u8);
}

/// Declarative interface for LED/Flashlight hardware control.
pub trait LedService {
    fn set_flashlight(&mut self, on: bool);
    fn toggle_flashlight(&mut self);
}

// ── Storage & VFS Services ──────────────────────────────────────────────────

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    NotFound = 0,
    IoError = 1,
    CorruptedData = 2,
    InvalidIdent = 3,
    CapacityFull = 4,
    AccessDenied = 5,
    EncryptedNoKey = 6,
    InvalidPath = 7,
}

pub const ATTR_READ_ONLY: u8 = 0x01;
pub const ATTR_SYSTEM: u8 = 0x02;
pub const ATTR_HIDDEN: u8 = 0x04;
pub const ATTR_ENCRYPTED: u8 = 0x08;
pub const ATTR_COMPRESSED: u8 = 0x10;
pub const ATTR_UNENCRYPTED: u8 = 0x00;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileAttributes {
    pub permission: u8,
}

// ── Serial & USB Communications ─────────────────────────────────────────────

/// Declarative interface for USB device connection and status.
pub trait UsbService {
    /// Whether the device is physically attached to an active USB host.
    fn is_connected(&self) -> bool;
    /// Whether a CDC terminal session is open (DTR asserted).
    fn is_dtr(&self) -> bool;
    /// Poll for hardware USB lifecycle events.
    fn poll_event(&mut self) -> Option<crate::event::UsbEvent> {
        None
    }
}

/// Logical serial port identifier for `SerialCore` multi-port routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PortId {
    UsbCdc = 0,
    Uart0 = 1,
    Uart1 = 2,
    Wireless = 3,
    Loopback = 4,
}

impl PortId {
    #[inline(always)]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::UsbCdc),
            1 => Some(Self::Uart0),
            2 => Some(Self::Uart1),
            3 => Some(Self::Wireless),
            4 => Some(Self::Loopback),
            _ => None,
        }
    }
}

/// Arbitrated operating mode for a logical serial port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PortMode {
    /// Kernel RigWire v2 COBS framer owns the port (unleased KRX writes are fenced).
    RigWire = 0,
    /// Exclusive raw byte stream leased to a runtime package (`owner_pid`).
    RawLease = 1,
    /// Dedicated GNSS NMEA/UBX parser feed.
    GnssFeed = 2,
    /// Hardware ring forwarder bridged to another `PortId`.
    Bridge = 3,
}

/// Compact 4-byte runtime arbitration state for one serial port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PortLeaseState {
    pub mode: PortMode,
    pub owner_pid: u8,
    pub peer_port: u8,
    pub flags: u8,
}

impl PortLeaseState {
    pub const fn rigwire() -> Self {
        Self {
            mode: PortMode::RigWire,
            owner_pid: 0,
            peer_port: 0,
            flags: 0,
        }
    }
}

/// Lightweight multi-port lease & write-fence router (`8 B` SRAM for 2 ports).
pub struct SerialRouter<const N: usize> {
    pub ports: [PortLeaseState; N],
}

impl<const N: usize> SerialRouter<N> {
    pub const fn new() -> Self {
        Self {
            ports: [PortLeaseState::rigwire(); N],
        }
    }

    pub fn acquire_raw(&mut self, port: usize, pid: u8) -> abi::Status {
        let Some(slot) = self.ports.get_mut(port) else {
            return abi::Status::InvalidArg;
        };
        if slot.mode == PortMode::RawLease && slot.owner_pid != pid {
            return abi::Status::Busy;
        }
        slot.mode = PortMode::RawLease;
        slot.owner_pid = pid;
        abi::Status::Ok
    }

    pub fn release_owner(&mut self, pid: u8) {
        for slot in &mut self.ports {
            if slot.mode == PortMode::RawLease && slot.owner_pid == pid {
                *slot = PortLeaseState::rigwire();
            }
        }
    }

    /// True if `pid` is permitted to perform raw serial reads/writes on `port`.
    /// When `rigwire_session_active` is true on a `RigWire`-mode port, unleased apps
    /// are write-fenced so COBS frames cannot be corrupted.
    #[inline(always)]
    pub fn can_raw_io(&self, port: usize, pid: u8, rigwire_session_active: bool) -> bool {
        let Some(slot) = self.ports.get(port) else {
            return false;
        };
        match slot.mode {
            PortMode::RawLease => slot.owner_pid == pid,
            PortMode::RigWire => !rigwire_session_active,
            PortMode::GnssFeed | PortMode::Bridge => false,
        }
    }
}

/// Unified streaming byte I/O contract for UART and USB CDC virtual COM ports.
pub trait SerialPort {
    fn write(&mut self, data: &[u8]);
    fn read(&mut self, buf: &mut [u8]) -> usize;
    /// Drain TX hardware FIFO / endpoint before baud changes, sleep, or system reset.
    fn flush(&mut self) {}
}

impl<T: SerialPort + ?Sized> SerialPort for &mut T {
    fn write(&mut self, data: &[u8]) {
        (**self).write(data);
    }
    fn read(&mut self, buf: &mut [u8]) -> usize {
        (**self).read(buf)
    }
    fn flush(&mut self) {
        (**self).flush();
    }
}

// ── User Input & Keypad State ───────────────────────────────────────────────

/// High-level semantic user input intents decoupled from raw hardware scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputIntent {
    StepUp,
    StepDown,
    FastStepUp,
    FastStepDown,
    ToggleVfo,
    ToggleReverse,
    ToggleDualWatch,
    StartScan,
    PttStart,
    PttEnd,
    OpenMenu,
    Back,
    Digit(u8),
    Custom(u8),
}

/// Declarative interface for user input state and keypad lock status.
pub trait InputService {
    fn is_keypad_locked(&self) -> bool;
    fn set_keypad_locked(&mut self, locked: bool);
    fn is_f_active(&self) -> bool;
    fn set_f_active(&mut self, active: bool);
}

/// Rich input subsystem with intent translation and chord recognition.
pub trait InputSubsystem: InputService {
    fn poll_intent(&mut self) -> Option<InputIntent> {
        None
    }
    fn is_ptt_pressed(&self) -> bool {
        false
    }
}
