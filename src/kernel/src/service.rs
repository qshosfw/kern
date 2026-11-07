//! High-assurance, zero-allocation Microkernel Service Contracts.
//!
//! Defines the formal microkernel service traits and hardware abstraction interfaces.
//! Concrete implementations are provided by drivers or system subsystems, while
//! applications, framework components, and loadable modules consume these contracts.

use heapless::String;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    NotFound,
    IoError,
    CorruptedData,
    InvalidIdent,
    CapacityFull,
    AccessDenied,
    EncryptedNoKey,
    InvalidPath,
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

#[derive(Clone, Debug, Default)]
pub struct DirEntryInfo {
    pub path: String<64>,
    pub size: usize,
    pub is_dir: bool,
    pub permission: u8,
}

impl DirEntryInfo {
    pub const fn empty() -> Self {
        Self {
            path: String::new(),
            size: 0,
            is_dir: false,
            permission: 0,
        }
    }
}

/// Filesystem usage statistics descriptor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageStats {
    pub active_files: u16,
    pub used_sectors: u16,
    pub free_sectors: u16,
    pub deleted_sectors: u16,
    pub wear_cycles: u32,
    pub total_sectors: u16,
}

/// Declarative interface for non-volatile storage and filesystem access.
pub trait StorageService {
    fn exists(&mut self, path: &str) -> bool;
    fn file_size(&mut self, path: &str) -> Result<usize, StorageError>;
    fn remove(&mut self, path: &str) -> Result<(), StorageError>;
    fn create_dir(&mut self, path: &str) -> Result<(), StorageError>;

    fn read_file(&mut self, path: &str, buf: &mut [u8]) -> Result<usize, StorageError>;
    fn read_file_at(
        &mut self,
        path: &str,
        offset: usize,
        buf: &mut [u8],
    ) -> Result<usize, StorageError> {
        if offset == 0 {
            self.read_file(path, buf)
        } else {
            Err(StorageError::IoError)
        }
    }
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), StorageError>;

    fn get_attributes(&mut self, path: &str) -> Result<FileAttributes, StorageError>;
    fn set_attributes(&mut self, path: &str, attrs: FileAttributes) -> Result<(), StorageError>;

    fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), StorageError> {
        if let Ok(attrs) = self.get_attributes(old_path) {
            if (attrs.permission & (ATTR_SYSTEM | ATTR_READ_ONLY)) != 0
                || old_path.starts_with("/sys/calib.")
            {
                return Err(StorageError::AccessDenied);
            }
        }

        let mut buf = [0u8; 512];
        let len = self.read_file(old_path, &mut buf)?;
        self.write_file(new_path, &buf[..len])?;

        // Copy attributes
        if let Ok(attrs) = self.get_attributes(old_path) {
            let _ = self.set_attributes(new_path, attrs);
        }

        self.remove(old_path)?;
        Ok(())
    }

    fn list_dir(
        &mut self,
        _dir_path: &str,
        _out_entries: &mut [DirEntryInfo],
    ) -> Result<usize, StorageError> {
        Ok(0)
    }

    fn stats(&mut self) -> Option<StorageStats> {
        None
    }
}

pub trait StorageExt: StorageService {
    fn read_struct<T: Copy>(&mut self, path: &str, out: &mut T) -> Result<usize, StorageError> {
        let size = core::mem::size_of::<T>();
        let slice = unsafe { core::slice::from_raw_parts_mut(out as *mut T as *mut u8, size) };
        self.read_file(path, slice)
    }

    fn write_struct<T: Copy>(&mut self, path: &str, data: &T) -> Result<(), StorageError> {
        let size = core::mem::size_of::<T>();
        let slice = unsafe { core::slice::from_raw_parts(data as *const T as *const u8, size) };
        self.write_file(path, slice)
    }
}

impl<T: StorageService + ?Sized> StorageExt for T {}

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

/// Unified streaming byte I/O contract for UART and USB CDC virtual COM ports.
pub trait SerialPort {
    fn write(&mut self, data: &[u8]);
    fn read(&mut self, buf: &mut [u8]) -> usize;
}

/// Declarative interface for UART/Serial communications.
pub trait UartService: SerialPort {}

impl<T: SerialPort + ?Sized> SerialPort for &mut T {
    fn write(&mut self, data: &[u8]) {
        (**self).write(data);
    }
    fn read(&mut self, buf: &mut [u8]) -> usize {
        (**self).read(buf)
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
