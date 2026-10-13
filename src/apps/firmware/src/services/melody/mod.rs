//! Modular Tone & Audio Sequence Composition Engine.
//!
//! Provides zero-alloc dynamic tone sequencing, rich presets (Roger beeps, keypress beeps,
//! alerts), and custom user-configurable tones decoupled from the hardware driver layer.

pub mod format;
pub mod presets;
pub mod types;

pub use format::*;
pub use presets::*;
pub use types::*;

use crate::services::SystemStorage;

/// Load roger melody from storage path, with automatic fallback to classic default
pub fn load_roger_melody(storage: &mut SystemStorage, path: &str) -> MelodyBuffer {
    if !path.is_empty() {
        if let Some(file) = ToneFile::load_from_storage(storage, path) {
            let buf = file.to_melody_buffer();
            if !buf.is_empty() {
                return buf;
            }
        }
    }
    // Resilient fallback to default Classic roger
    let mut buf = MelodyBuffer::new();
    for note in ROGER_CLASSIC_FALLBACK {
        let _ = buf.push(*note);
    }
    buf
}

pub fn get_roger_name(
    storage: &mut SystemStorage,
    path: &str,
) -> gfx::gui::modules::menu::ShortString {
    use gfx::gui::modules::menu::ShortString;
    if let Some(tf) = ToneFile::load_from_storage(storage, path) {
        ShortString::from_str(tf.display_name(path))
    } else {
        let base = ToneFile::extract_basename(path);
        if base.is_empty() {
            ShortString::from_str("Active")
        } else {
            ShortString::from_str(base)
        }
    }
}

#[cfg(feature = "filesystem")]
fn write_provision_roger(
    storage: &mut SystemStorage,
    path: &str,
    name: &str,
    notes: &[(u16, u16)],
) {
    if !storage.exists(path) {
        let mut buf = [0u8; 160];
        buf[0] = b'K';
        buf[1] = b'T';
        buf[2] = 1;
        buf[3] = notes.len() as u8;
        let mut pos = 4;
        for &(freq, dur) in notes {
            buf[pos..pos + 2].copy_from_slice(&freq.to_be_bytes());
            buf[pos + 2..pos + 4].copy_from_slice(&dur.to_be_bytes());
            pos += 4;
        }
        let name_bytes = name.as_bytes();
        if !name_bytes.is_empty() {
            buf[pos] = TAG_NAME;
            buf[pos + 1] = name_bytes.len() as u8;
            pos += 2;
            buf[pos..pos + name_bytes.len()].copy_from_slice(name_bytes);
            pos += name_bytes.len();
        }
        let _ = storage.write_file(path, &buf[..pos]);
    }
}

#[cfg(not(feature = "filesystem"))]
pub fn provision_rogers(_storage: &mut SystemStorage) {}

/// Provision default roger tone files into LittleFS (/user/rogers)
#[cfg(feature = "filesystem")]
pub fn provision_rogers(storage: &mut SystemStorage) {
    let mut path_buf = [0u8; 32];
    path_buf[..13].copy_from_slice(b"/user/rogers/");

    for def in ROGER_DEFS {
        let flen = def.file_name.len();
        path_buf[13..13 + flen].copy_from_slice(def.file_name.as_bytes());
        let path = unsafe { core::str::from_utf8_unchecked(&path_buf[..13 + flen]) };
        write_provision_roger(storage, path, def.name, def.notes);
    }
}

pub struct MelodyEngine;

impl MelodyEngine {
    /// Return the const note slice for a built-in beep preset.
    pub fn beep_notes(beep: BeepType) -> &'static [ToneNote] {
        match beep {
            BeepType::Off => &[],
            BeepType::Tone => BEEP_TONE,
            BeepType::Click => BEEP_CLICK,
            BeepType::Double => BEEP_DOUBLE,
            BeepType::Warning => BEEP_WARNING,
            BeepType::Scroll => BEEP_SCROLL,
            BeepType::Custom => BEEP_TONE, // fallback; real custom handled by beep_melody
        }
    }

    /// Generate a Keypress Beep melody. For built-in types this copies from const .rodata;
    /// for Custom it uses caller-supplied parameters.
    pub fn beep_melody(
        beep: BeepType,
        custom_freq_hz: u16,
        custom_duration_ms: u16,
    ) -> MelodyBuffer {
        let mut buf = MelodyBuffer::new();

        if beep == BeepType::Custom {
            let freq = if custom_freq_hz == 0 {
                700
            } else {
                custom_freq_hz
            };
            let dur = if custom_duration_ms == 0 {
                50
            } else {
                custom_duration_ms
            };
            let _ = buf.push(ToneNote::new(freq, dur));
        } else {
            for note in Self::beep_notes(beep) {
                let _ = buf.push(*note);
            }
        }

        buf
    }

    /// Return the const note slice for a dialog/notification sound.
    pub fn dialog_notes(sound: DialogSoundType) -> &'static [ToneNote] {
        match sound {
            DialogSoundType::Lock => DIALOG_LOCK,
            DialogSoundType::Unlock => DIALOG_UNLOCK,
            DialogSoundType::Prompt => DIALOG_PROMPT,
            DialogSoundType::Confirm => DIALOG_CONFIRM,
            DialogSoundType::Success => DIALOG_SUCCESS,
            DialogSoundType::Warning => DIALOG_WARNING,
            DialogSoundType::Error => DIALOG_ERROR,
            DialogSoundType::Charging => CHIME_CHARGING,
            DialogSoundType::UsbConnect => CHIME_USB_CONNECT,
            DialogSoundType::UsbDisconnect => CHIME_USB_DISCONNECT,
        }
    }

    /// Return the const note slice for a boot jingle.
    pub fn boot_jingle_notes(jingle: BootJingleType) -> &'static [ToneNote] {
        match jingle {
            BootJingleType::Off => &[],
            BootJingleType::Chime => JINGLE_CHIME,
            BootJingleType::Nokia => JINGLE_NOKIA,
            BootJingleType::Chirp => JINGLE_CHIRP,
            BootJingleType::GameBoy => JINGLE_GAMEBOY,
        }
    }
}
