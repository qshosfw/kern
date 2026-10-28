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
    let mut buf = MelodyBuffer::new();
    if !path.is_empty() {
        let mut raw = [0u8; 140];
        if let Ok(n) = storage.read_file(path, &mut raw) {
            if n >= 4 && &raw[0..2] == b"KT" && raw[2] == 1 {
                let count = (raw[3] as usize).min(MAX_MELODY_NOTES);
                let mut pos = 4;
                while pos + 4 <= n && buf.len() < count {
                    let freq = u16::from_be_bytes([raw[pos], raw[pos + 1]]);
                    let dur = u16::from_be_bytes([raw[pos + 2], raw[pos + 3]]);
                    let _ = buf.push(ToneNote::new(freq, dur));
                    pos += 4;
                }
                if !buf.is_empty() {
                    return buf;
                }
            } else if n >= 16 && &raw[0..4] == b"KRGR" {
                let count = (raw[5] as usize).min(MAX_MELODY_NOTES);
                let mut pos = 16;
                while pos + 4 <= n && buf.len() < count {
                    let freq = u16::from_be_bytes([raw[pos], raw[pos + 1]]);
                    let dur = u16::from_be_bytes([raw[pos + 2], raw[pos + 3]]);
                    let _ = buf.push(ToneNote::new(freq, dur));
                    pos += 4;
                }
                if !buf.is_empty() {
                    return buf;
                }
            }
        }
    }
    // Resilient fallback to default Classic roger
    for note in ROGER_CLASSIC_FALLBACK {
        let _ = buf.push(*note);
    }
    buf
}

/// Load jingle melody from storage path, or fallback to selected preset
pub fn load_jingle_melody(
    storage: &mut SystemStorage,
    boot_jingle_mode: u8,
    path: &str,
) -> MelodyBuffer {
    let mut buf = MelodyBuffer::new();
    if boot_jingle_mode == 0 {
        return buf;
    }
    if !path.is_empty() {
        let mut raw = [0u8; 140];
        if let Ok(n) = storage.read_file(path, &mut raw) {
            if n >= 4 && &raw[0..2] == b"KT" && raw[2] == 1 {
                let count = (raw[3] as usize).min(MAX_MELODY_NOTES);
                let mut pos = 4;
                while pos + 4 <= n && buf.len() < count {
                    let freq = u16::from_be_bytes([raw[pos], raw[pos + 1]]);
                    let dur = u16::from_be_bytes([raw[pos + 2], raw[pos + 3]]);
                    let _ = buf.push(ToneNote::new(freq, dur));
                    pos += 4;
                }
                if !buf.is_empty() {
                    return buf;
                }
            }
        }
    }
    // Fallback to configured preset
    let jtype = BootJingleType::from_u8(boot_jingle_mode);
    for note in MelodyEngine::boot_jingle_notes(jtype) {
        let _ = buf.push(*note);
    }
    buf
}

/// Fast zero-allocation, zero-I/O display name extractor for roger files
pub fn get_roger_name(path: &str) -> gfx::gui::modules::menu::ShortString {
    use gfx::gui::modules::menu::ShortString;
    let name_start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
    let file_part = &path[name_start..];
    let base = if let Some(dot_idx) = gfx::text::rfind_byte(file_part, b'.') {
        &file_part[..dot_idx]
    } else {
        file_part
    };
    if base.starts_with("01_classic") {
        ShortString::from_str("Classic")
    } else if base.starts_with("02_tiny") {
        ShortString::from_str("Tiny")
    } else if base.starts_with("03_mototrbo") {
        ShortString::from_str("MotoTRBO")
    } else if base.starts_with("04_apx6000") {
        ShortString::from_str("APX6000")
    } else if base.starts_with("05_t40") {
        ShortString::from_str("T40")
    } else if base.starts_with("06_tlkrt80") {
        ShortString::from_str("TLKR T80")
    } else if base.starts_with("07_cobra") {
        ShortString::from_str("Cobra")
    } else {
        let clean = if base.len() > 3
            && base.as_bytes()[0].is_ascii_digit()
            && base.as_bytes()[1].is_ascii_digit()
            && base.as_bytes()[2] == b'_'
        {
            &base[3..]
        } else {
            base
        };
        ShortString::from_str(clean)
    }
}

/// Fast zero-allocation, zero-I/O display name extractor for jingle files
pub fn get_jingle_name(path: &str) -> gfx::gui::modules::menu::ShortString {
    use gfx::gui::modules::menu::ShortString;
    let name_start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
    let file_part = &path[name_start..];
    let base = if let Some(dot_idx) = gfx::text::rfind_byte(file_part, b'.') {
        &file_part[..dot_idx]
    } else {
        file_part
    };
    if base.starts_with("01_chime") {
        ShortString::from_str("Chime")
    } else if base.starts_with("02_nokia") {
        ShortString::from_str("Nokia")
    } else if base.starts_with("03_chirp") {
        ShortString::from_str("Chirp")
    } else if base.starts_with("04_gameboy") {
        ShortString::from_str("GameBoy")
    } else {
        let clean = if base.len() > 3
            && base.as_bytes()[0].is_ascii_digit()
            && base.as_bytes()[1].is_ascii_digit()
            && base.as_bytes()[2] == b'_'
        {
            &base[3..]
        } else {
            base
        };
        ShortString::from_str(clean)
    }
}

#[cfg(all(
    feature = "filesystem",
    any(feature = "provision_rogers", feature = "boot_jingle")
))]
fn write_provision_ton(storage: &mut SystemStorage, path: &str, name: &str, notes: &[ToneNote]) {
    if !storage.exists(path) {
        let mut buf = [0u8; 160];
        buf[0] = b'K';
        buf[1] = b'T';
        buf[2] = 1;
        buf[3] = notes.len() as u8;
        let mut pos = 4;
        for note in notes {
            buf[pos..pos + 2].copy_from_slice(&note.freq_hz.to_be_bytes());
            buf[pos + 2..pos + 4].copy_from_slice(&note.duration_ms.to_be_bytes());
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

#[cfg(not(all(feature = "filesystem", feature = "provision_rogers")))]
pub fn provision_rogers(_storage: &mut SystemStorage) {}

/// Provision default roger tone files into LittleFS (/user/rogers)
#[cfg(all(feature = "filesystem", feature = "provision_rogers"))]
pub fn provision_rogers(storage: &mut SystemStorage) {
    let _ = storage.create_dir("/user/rogers");
    let mut path_buf = [0u8; 32];
    path_buf[..13].copy_from_slice(b"/user/rogers/");

    for def in ROGER_DEFS {
        let flen = def.file_name.len();
        path_buf[13..13 + flen].copy_from_slice(def.file_name.as_bytes());
        let path = unsafe { core::str::from_utf8_unchecked(&path_buf[..13 + flen]) };
        write_provision_ton(storage, path, def.name, def.notes);
    }
}

#[cfg(not(all(feature = "filesystem", feature = "boot_jingle")))]
pub fn provision_jingles(_storage: &mut SystemStorage) {}

/// Provision default boot jingle tone files into LittleFS (/user/jingles)
#[cfg(all(feature = "filesystem", feature = "boot_jingle"))]
pub fn provision_jingles(storage: &mut SystemStorage) {
    let _ = storage.create_dir("/user/jingles");
    let mut path_buf = [0u8; 32];
    path_buf[..14].copy_from_slice(b"/user/jingles/");

    for def in JINGLE_DEFS {
        let flen = def.file_name.len();
        path_buf[14..14 + flen].copy_from_slice(def.file_name.as_bytes());
        let path = unsafe { core::str::from_utf8_unchecked(&path_buf[..14 + flen]) };
        write_provision_ton(storage, path, def.name, def.notes);
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
