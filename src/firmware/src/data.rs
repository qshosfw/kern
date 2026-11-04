//! Structured Data, Documents, and Filesystem Formats for KERN OS Firmware.
//!
//! Provides zero-heap record storage formats for Channels (`/channels.ch`),
//! Contacts (`/contacts.ct`), Codeplugs (`/codeplug.bin`), Application Bundles (`.app`),
//! and display frame delta encoding / compression for screen-sharing and capture.

pub mod app_bundle;
pub mod arena;
pub mod channel_record;
pub mod contact_record;
pub mod cursor;
pub mod file_header;
pub mod frame_delta;

pub use app_bundle::{
    KRX_MAGIC, KrxClass, KrxExecType, KrxPreamble, KrxProfile, SectionExecution, SectionIdentity,
    SectionType,
};
pub use arena::*;
pub use channel_record::{
    AprsChannelExt, ChannelExtension, ChannelRecord, ExtensionTag, HermesChannelExt,
    PowerChannelExt, ScanChannelExt,
};
pub use contact_record::ContactRecord;
pub use cursor::{RecordCursor, SliceRecordCursor};
pub use file_header::{
    FILE_ID_APP_BUNDLE, FILE_ID_APP_LEGACY, FILE_ID_BMP, FILE_ID_KRX, FILE_ID_RLE_BITMAP,
    FILE_ID_VFO, FileType, TABLE_ID_BANDPLAN, TABLE_ID_CAPABILITIES, TABLE_ID_CHANNELS,
    TABLE_ID_CONTACTS, TABLE_ID_FM_STATION, TABLE_ID_HITS, TABLE_ID_ZONE, TableFileHeader,
};
pub use frame_delta::{
    FrameDeltaDecoder, FrameDeltaEncoder, FrameDeltaHeader, PageDeltaEngine, RleCompressor,
};
