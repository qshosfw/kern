//! Application Package & Bundle Format Re-exports.
//!
//! Re-exports normative KRX package structures from the microkernel.

pub use super::file_header::{FILE_ID_APP_BUNDLE, FILE_ID_APP_LEGACY, FILE_ID_KRX};
pub use kernel::proc::krx::{
    KRX_IDENT, KrxClass, KrxExecType, KrxPreamble, KrxProfile, SECTION_EXECUTION, SECTION_IDENTITY,
    SECTION_TYPE_LIFECYCLE, SectionExecution, SectionIdentity, SectionType,
};
