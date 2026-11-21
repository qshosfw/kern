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

#[inline(never)]
fn read_kt_file(
    storage: &mut SystemStorage,
    path: &str,
    allow_krgr: bool,
    out: &mut [ToneNote; MAX_MELODY_NOTES],
) -> usize {
    if path.is_empty() {
        return 0;
    }
    let raw_buf = &mut crate::data::arena::staging_buf()[128..268];
    let Ok(n) = storage.read_file(path, raw_buf) else {
        return 0;
    };
    parse_kt_bytes(&raw_buf[..n], allow_krgr, out)
}

fn parse_kt_bytes(raw: &[u8], allow_krgr: bool, out: &mut [ToneNote; MAX_MELODY_NOTES]) -> usize {
    let n = raw.len();
    let (mut pos, count_raw) = if n >= 4 && &raw[0..2] == b"KT" && raw[2] == 1 {
        let count = if raw[3] > 0 {
            raw[3] as usize
        } else {
            (n - 4) >> 2
        };
        (4usize, count)
    } else if allow_krgr && n >= 16 && &raw[0..4] == b"KRGR" {
        (16usize, raw[5] as usize)
    } else {
        return 0;
    };
    let count = count_raw.min(MAX_MELODY_NOTES);
    let mut written = 0usize;
    while pos + 4 <= n && written < count {
        let freq = u16::from_be_bytes([raw[pos], raw[pos + 1]]);
        let dur = u16::from_be_bytes([raw[pos + 2], raw[pos + 3]]);
        out[written] = ToneNote::new(freq, dur);
        written += 1;
        pos += 4;
    }
    written
}

fn fill_notes(slice: &[ToneNote], out: &mut [ToneNote; MAX_MELODY_NOTES]) -> usize {
    let count = slice.len().min(MAX_MELODY_NOTES);
    let mut i = 0;
    while i < count {
        out[i] = slice[i];
        i += 1;
    }
    count
}

/// Load roger melody notes into `out` from storage path, with fallback to classic default.
#[inline(never)]
pub fn load_roger_notes(
    storage: &mut SystemStorage,
    path: &str,
    out: &mut [ToneNote; MAX_MELODY_NOTES],
) -> usize {
    let n = read_kt_file(storage, path, true, out);
    if n > 0 {
        return n;
    }
    fill_notes(pool_slice(RANGE_ROGER_CLASSIC), out)
}

/// Load jingle melody notes into `out` from storage path, or fallback to selected preset.
#[inline(never)]
pub fn load_jingle_notes(
    storage: &mut SystemStorage,
    boot_jingle_mode: u8,
    path: &str,
    out: &mut [ToneNote; MAX_MELODY_NOTES],
) -> usize {
    if boot_jingle_mode == 0 {
        return 0;
    }
    let n = read_kt_file(storage, path, false, out);
    if n > 0 {
        return n;
    }
    fill_notes(
        MelodyEngine::boot_jingle_notes(BootJingleType::from_u8(boot_jingle_mode)),
        out,
    )
}

#[cfg(all(
    feature = "filesystem",
    any(feature = "provision_rogers", feature = "boot_jingle")
))]
fn write_provision_ton(storage: &mut SystemStorage, path: &str, name: &str, notes: &[ToneNote]) {
    if !storage.exists(path) {
        let mut buf = [0u8; 160];
        let max_notes = ((buf.len() - 4) / 4).min(MAX_MELODY_NOTES);
        let note_count = notes.len().min(max_notes);
        buf[0] = b'K';
        buf[1] = b'T';
        buf[2] = 1;
        buf[3] = note_count as u8;
        let mut pos = 4;
        for note in &notes[..note_count] {
            buf[pos..pos + 2].copy_from_slice(&note.freq_hz.to_be_bytes());
            buf[pos + 2..pos + 4].copy_from_slice(&note.duration_ms.to_be_bytes());
            pos += 4;
        }
        let max_name = buf.len().saturating_sub(pos + 2);
        let name_len = name.len().min(max_name).min(u8::MAX as usize);
        if name_len > 0 {
            buf[pos] = TAG_NAME;
            buf[pos + 1] = name_len as u8;
            pos += 2;
            buf[pos..pos + name_len].copy_from_slice(&name.as_bytes()[..name_len]);
            pos += name_len;
        }
        let _ = storage.write_file(path, &buf[..pos]);
    }
}

#[cfg(all(
    feature = "filesystem",
    any(feature = "provision_rogers", feature = "boot_jingle")
))]
fn build_tone_path<'a>(
    buf: &'a mut [u8; 32],
    dir_prefix: &[u8],
    file_name: &str,
) -> Option<&'a str> {
    let fname = file_name.as_bytes();
    let total = dir_prefix.len().checked_add(fname.len())?;
    if total > buf.len() {
        return None;
    }
    buf[..dir_prefix.len()].copy_from_slice(dir_prefix);
    buf[dir_prefix.len()..total].copy_from_slice(fname);
    utils::format::ascii_str(&buf[..total])
}

#[cfg(not(all(feature = "filesystem", feature = "provision_rogers")))]
pub fn provision_rogers(_storage: &mut SystemStorage) {}

/// Provision default roger tone files into LittleFS (/user/rogers)
#[cfg(all(feature = "filesystem", feature = "provision_rogers"))]
pub fn provision_rogers(storage: &mut SystemStorage) {
    let _ = storage.create_dir("/user/rogers");
    let mut path_buf = [0u8; 32];

    for def in ROGER_DEFS {
        if let Some(path) = build_tone_path(&mut path_buf, b"/user/rogers/", def.file_name) {
            write_provision_ton(storage, path, def.name, def.notes);
        }
    }
}

#[cfg(not(all(feature = "filesystem", feature = "boot_jingle")))]
pub fn provision_jingles(_storage: &mut SystemStorage) {}

/// Provision default boot jingle tone files into LittleFS (/user/jingles)
#[cfg(all(feature = "filesystem", feature = "boot_jingle"))]
pub fn provision_jingles(storage: &mut SystemStorage) {
    let _ = storage.create_dir("/user/jingles");
    let mut path_buf = [0u8; 32];

    for def in JINGLE_DEFS {
        if let Some(path) = build_tone_path(&mut path_buf, b"/user/jingles/", def.file_name) {
            write_provision_ton(storage, path, def.name, def.notes);
        }
    }
}

pub struct MelodyEngine;

impl MelodyEngine {
    /// Return the const note slice for a built-in beep preset.
    pub fn beep_notes(beep: BeepType) -> &'static [ToneNote] {
        pool_slice(BEEP_RANGES[beep as usize])
    }

    /// Return the const note slice for a dialog/notification sound.
    pub fn dialog_notes(sound: DialogSoundType) -> &'static [ToneNote] {
        pool_slice(DIALOG_RANGES[sound as usize])
    }

    /// Return the const note slice for a built-in roger preset.
    pub fn roger_notes(roger: RogerType) -> &'static [ToneNote] {
        match roger {
            RogerType::Off => &[],
            RogerType::Tiny => pool_slice(RANGE_ROGER_TINY),
            #[cfg(feature = "extra_rogers")]
            other => {
                let idx = (other as usize).saturating_sub(3);
                EXTRA_ROGER_NOTES
                    .get(idx)
                    .copied()
                    .unwrap_or_else(|| pool_slice(RANGE_ROGER_CLASSIC))
            }
            #[cfg(not(feature = "extra_rogers"))]
            _ => pool_slice(RANGE_ROGER_CLASSIC),
        }
    }

    /// Return the const note slice for a boot jingle.
    pub fn boot_jingle_notes(jingle: BootJingleType) -> &'static [ToneNote] {
        match jingle {
            BootJingleType::Off => &[],
            #[cfg(feature = "boot_jingle")]
            BootJingleType::Nokia => JINGLE_NOKIA,
            #[cfg(feature = "boot_jingle")]
            BootJingleType::Chirp => JINGLE_CHIRP,
            #[cfg(feature = "boot_jingle")]
            BootJingleType::GameBoy => JINGLE_GAMEBOY,
            _ => pool_slice(RANGE_JINGLE_CHIME),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kt_bytes_supports_dynamic_and_explicit_counts() {
        // Explicit count = 2
        let explicit = [
            b'K', b'T', 1, 2, 0x03, 0xE8, 0x00, 0x32, 0x07, 0xD0, 0x00, 0x64,
        ];
        let mut out = [ToneNote::new(0, 0); MAX_MELODY_NOTES];
        assert_eq!(parse_kt_bytes(&explicit, false, &mut out), 2);
        assert_eq!(out[0], ToneNote::new(1000, 50));
        assert_eq!(out[1], ToneNote::new(2000, 100));

        // Dynamic count = 0 derives note count from remaining 4-byte pairs
        let dynamic = [
            b'K', b'T', 1, 0, 0x01, 0xB8, 0x00, 0x28, 0x02, 0x58, 0x00, 0x50,
        ];
        assert_eq!(parse_kt_bytes(&dynamic, false, &mut out), 2);
        assert_eq!(out[0], ToneNote::new(440, 40));
        assert_eq!(out[1], ToneNote::new(600, 80));
    }
}
