//! Structured Data, Documents, and Filesystem Formats for KERN OS Firmware.
//!
//! Zero-heap fixed-stride record formats (ORDS-01): channels, contacts, bandplans,
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
