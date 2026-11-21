//! Binary `.ton` file format parsing and serialization for tone definitions.

use super::types::MelodyBuffer;
use crate::services::SystemStorage;
use drivers::audio::ToneNote;
use heapless::Vec;

/// Format identifier for Kern Tone File (`.ton`): 2 bytes "KT"
pub const FILE_ID_TONE: [u8; 2] = *b"KT";
pub const TONE_VERSION: u8 = 1;
pub const MAX_TONE_NOTES: usize = 32;

/// Legacy format identifier for `.rgr` files
pub const FILE_ID_ROGER: [u8; 4] = *b"KRGR";
pub const MAX_ROGER_NOTES: usize = MAX_TONE_NOTES;

/// Metadata tag identifiers in `.ton` tail
pub const TAG_NAME: u8 = 0x01;
pub const MAX_NAME_LEN: usize = 16;

/// 4-byte header for `.ton` files
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneHeader {
    pub ident: [u8; 2], // "KT"
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
        } else if data.len() >= 4 && data[0..2] == FILE_ID_TONE && data[2] == TONE_VERSION {
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
                ident: FILE_ID_TONE,
                version: TONE_VERSION,
                note_count: notes.len() as u8,
            },
            notes,
            name,
        })
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
}
