//! Structured Data, Documents, and Filesystem Formats for KERN OS Firmware.
//!
//! Zero-heap fixed-stride record tables: channels, contacts, bandplans,
//! zones, scan hits, FM stations and VFO snapshots, plus the shared transient arena.
//! KRX package structures live in `kernel::runtime`.

pub mod arena;
pub mod channel_record;
pub mod contact_record;
pub mod file_header;
pub mod records;

pub use arena::*;
pub use channel_record::{ChannelExtension, ChannelRecord, ExtensionTag};
pub use contact_record::{AddressScheme, ContactKind, ContactRecord, TransportClass, fnv1a_hash64};
pub use file_header::{
    FILE_ID_APP_BUNDLE, FILE_ID_APP_LEGACY, FILE_ID_BMP, FILE_ID_KRX, FILE_ID_RLE_BITMAP,
    FILE_ID_VFO, FileType, TABLE_ID_BANDPLAN, TABLE_ID_CAPABILITIES, TABLE_ID_CHANNELS,
    TABLE_ID_CONTACTS, TABLE_ID_FM_STATION, TABLE_ID_HITS, TABLE_ID_ZONE, TableFileHeader,
};
pub use records::{
    BandPlanRecord, FmStationRecord, ScanHitRecord, VfoFileHeader, VfoOperatingMode,
    VfoSnapshotFile, ZoneRecord,
};

/// Read a NUL-terminated ASCII string from a fixed-size record field.
pub(crate) fn read_fixed_str(buf: &[u8]) -> &str {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    utils::format::ascii_str(&buf[..len]).unwrap_or("")
}

/// Zero-fill `dst` and copy up to `max_len` bytes of `s` into it.
pub(crate) fn write_fixed_str(dst: &mut [u8], s: &str, max_len: usize) {
    dst.fill(0);
    let bytes = s.as_bytes();
    let copy_len = bytes.len().min(max_len).min(dst.len());
    dst[..copy_len].copy_from_slice(&bytes[..copy_len]);
}
