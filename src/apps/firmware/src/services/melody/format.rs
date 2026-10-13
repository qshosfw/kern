//! Binary `.ton` file format parsing and serialization for tone definitions.

use super::types::MelodyBuffer;
use crate::services::SystemStorage;
use drivers::audio::ToneNote;
use heapless::Vec;

/// Magic identifier for Kern Tone File (`.ton`): 2 bytes "KT"
pub const TONE_MAGIC: [u8; 2] = *b"KT";
pub const TONE_VERSION: u8 = 1;
pub const MAX_TONE_NOTES: usize = 32;

/// Legacy magic identifier for `.rgr` files
pub const ROGER_MAGIC: [u8; 4] = *b"KRGR";
pub const MAX_ROGER_NOTES: usize = MAX_TONE_NOTES;

/// Metadata tag identifiers in `.ton` tail
pub const TAG_NAME: u8 = 0x01;
pub const MAX_NAME_LEN: usize = 16;

/// 4-byte header for `.ton` files
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneHeader {
    pub magic: [u8; 2], // "KT"
    pub version: u8,    // 1
    pub note_count: u8, // 0 = dynamic length from file size, >0 = explicit count
}

/// Self-contained Tone definition for `.ton` file storage
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToneFile {
    pub header: ToneHeader,
    pub notes: Vec<ToneNote, MAX_TONE_NOTES>,
    pub name: Option<heapless::String<MAX_NAME_LEN>>,
}

impl ToneFile {
    pub fn new(name_str: &str) -> Self {
        let mut s = heapless::String::new();
        let bytes = name_str.as_bytes();
        let mut len = bytes.len().min(MAX_NAME_LEN);
        while len > 0 && (bytes[len - 1] & 0xC0) == 0x80 {
            len -= 1;
        }
        if len > 0 && (bytes[len - 1] & 0x80) != 0 {
            len -= 1;
        }
        let _ = s.push_str(unsafe { core::str::from_utf8_unchecked(&bytes[..len]) });

        Self {
            header: ToneHeader {
                magic: TONE_MAGIC,
                version: TONE_VERSION,
                note_count: 0,
            },
            notes: Vec::new(),
            name: if s.is_empty() { None } else { Some(s) },
        }
    }

    pub fn add_note(&mut self, freq_hz: u16, duration_ms: u16) -> bool {
        if self.notes.push(ToneNote::new(freq_hz, duration_ms)).is_ok() {
            self.header.note_count = self.notes.len() as u8;
            true
        } else {
            false
        }
    }

    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    pub fn display_name<'a>(&'a self, fallback_path: &'a str) -> &'a str {
        if let Some(ref name) = self.name {
            name.as_str()
        } else {
            Self::extract_basename(fallback_path)
        }
    }

    pub fn extract_basename(path: &str) -> &str {
        let name_start = gfx::text::rfind_byte(path, b'/').map_or(0, |i| i + 1);
        let file_part = &path[name_start..];
        if let Some(dot_idx) = gfx::text::rfind_byte(file_part, b'.') {
            &file_part[..dot_idx]
        } else {
            file_part
        }
    }

    /// Serialize into `.ton` binary format:
    /// - 4-byte header: `b"KT"`, version (1), note_count (u8)
    /// - `note_count * 4` bytes of `[freq_hz: u16 BE, duration_ms: u16 BE]`
    /// - Optional metadata tail: `[TAG_NAME (0x01), len, name_bytes...]`
    pub fn to_bytes(&self) -> Vec<u8, 160> {
        let mut out = Vec::new();
        let _ = out.push(self.header.magic[0]);
        let _ = out.push(self.header.magic[1]);
        let _ = out.push(self.header.version);
        let _ = out.push(self.notes.len() as u8);
        for note in &self.notes {
            let fb = note.freq_hz.to_be_bytes();
            let db = note.duration_ms.to_be_bytes();
            let _ = out.push(fb[0]);
            let _ = out.push(fb[1]);
            let _ = out.push(db[0]);
            let _ = out.push(db[1]);
        }
        if let Some(ref name) = self.name {
            let bytes = name.as_bytes();
            if !bytes.is_empty() {
                let _ = out.push(TAG_NAME);
                let _ = out.push(bytes.len() as u8);
                for &b in bytes {
                    let _ = out.push(b);
                }
            }
        }
        out
    }

    /// Parse `.ton` binary format or legacy 80-byte `.rgr` format
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let mut notes = Vec::new();
        let mut name: Option<heapless::String<MAX_NAME_LEN>> = None;

        let parse_ascii = |bytes: &[u8]| -> Option<heapless::String<MAX_NAME_LEN>> {
            let len = gfx::text::find_byte(bytes, 0)
                .unwrap_or(bytes.len())
                .min(MAX_NAME_LEN);
            if let Some(s) = gfx::text::from_ascii_bytes(&bytes[..len]) {
                if !s.is_empty() {
                    let mut out = heapless::String::new();
                    let _ = out.push_str(s);
                    return Some(out);
                }
            }
            None
        };

        let is_krgr = data.len() >= 16 && &data[0..4] == b"KRGR";
        let (mut offset, count) = if is_krgr {
            name = parse_ascii(&data[6..16]);
            (16, (data[5] as usize).min(MAX_TONE_NOTES))
        } else if data.len() >= 4 && data[0..2] == TONE_MAGIC && data[2] == TONE_VERSION {
            let raw = data[3] as usize;
            let c = if raw > 0 {
                raw.min(MAX_TONE_NOTES)
            } else {
                (data.len() - 4) >> 2
            };
            (4, c.min(MAX_TONE_NOTES))
        } else {
            return None;
        };

        while offset + 4 <= data.len() && notes.len() < count {
            let freq = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let dur = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            let _ = notes.push(ToneNote::new(freq, dur));
            offset += 4;
        }

        if !is_krgr {
            let raw_count = data[3] as usize;
            if raw_count > MAX_TONE_NOTES {
                offset += (raw_count - MAX_TONE_NOTES) << 2;
            }
            while offset + 2 <= data.len() {
                let tag = data[offset];
                let len = data[offset + 1] as usize;
                offset += 2;
                if offset + len <= data.len() {
                    if tag == TAG_NAME && name.is_none() {
                        name = parse_ascii(&data[offset..offset + len]);
                    }
                    offset += len;
                } else {
                    break;
                }
            }
        }

        Some(Self {
            header: ToneHeader {
                magic: TONE_MAGIC,
                version: TONE_VERSION,
                note_count: notes.len() as u8,
            },
            notes,
            name,
        })
    }

    /// Import from DeltaFW packed 16-bit format:
    /// upper 9 bits = frequency index (0=pause, else 250 + i*10 Hz), lower 7 bits = duration (*20ms)
    pub fn from_deltafw_packed(name_str: &str, entries: &[u16]) -> Self {
        let mut file = Self::new(name_str);
        for &entry in entries.iter().take(MAX_TONE_NOTES) {
            if entry == 0 || entry == 0xFFFF {
                break;
            }
            let freq_idx = entry >> 7;
            let dur_ms = (entry & 0x7F) * 20;
            let freq_hz = if freq_idx == 0 {
                0
            } else {
                250 + freq_idx * 10
            };
            file.add_note(freq_hz, dur_ms);
        }
        file
    }

    pub fn to_melody_buffer(&self) -> MelodyBuffer {
        let mut buf = MelodyBuffer::new();
        for note in &self.notes {
            let _ = buf.push(*note);
        }
        buf
    }

    pub fn load_from_storage(storage: &mut SystemStorage, path: &str) -> Option<Self> {
        let mut buf = [0u8; 160];
        if let Ok(n) = storage.read_file(path, &mut buf) {
            Self::from_bytes(&buf[..n])
        } else {
            None
        }
    }

    pub fn save_to_storage(
        &self,
        storage: &mut SystemStorage,
        path: &str,
    ) -> Result<(), crate::services::StorageError> {
        let bytes = self.to_bytes();
        storage.write_file(path, bytes.as_slice())
    }
}
