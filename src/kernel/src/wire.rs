//! Universal, Firmware-Agnostic Transceiver Wire Protocol (RigWire v2).
//!
//! Provides naturally aligned 8-byte framing, terminator-only in-place COBS,
//! canonical `abi::Status` error reporting, declarative `SemanticCap` / `TableDesc`
//! discovery, universal Resource Handle RPC, and streaming storage transfers.

pub mod header;
pub mod ops_control;
pub mod ops_core;
pub mod ops_data;
pub mod ops_events;
pub mod session;

pub use abi::Status;
pub use header::*;
pub use ops_control::*;
pub use ops_core::*;
pub use ops_data::*;
pub use ops_events::*;
pub use session::*;
