//! Kern application binary interface: the one contract shared by the kernel,
//! firmware, RigWire and the C SDK.
//!
//! This crate has no dependencies. Every identifier here is declared exactly once;
//! `./kern abi` links this crate on the host and prints `sdk/c/krx_ops.h`
//! from the same tables the firmware dispatches on.
#![no_std]

/// ABI major version. Bump on any renumbered or retired operation; the loader
/// admits only packages built against this exact version.
pub const VERSION: u16 = 4;

mod cap;
mod ops;
mod pkg;
pub mod prop;
pub mod types;

pub use cap::{Cap, CapIter, CapabilitySet};
pub use ops::{ArgKind, OP_TABLE, Op, OpDesc, Sig, domain, idx, missing_caps, sub};
pub use pkg::{icon, scope};
pub use types::*;

#[cfg(feature = "describe")]
pub mod describe {
    //! Names behind the numeric contract, for host-side generators only.
    pub use crate::cap::CAP_IDENTS;
    pub use crate::ops::{DOMAINS, DomainInfo, OpInfo, SUB_GROUPS, SubGroupInfo};
}
