//! High-Assurance Serial & Control Protocol (RigWire Core 0.1).
//!
//! Provides bounded, language-neutral framing, request/response tracking,
//! sequence validation, capability discovery, and chunked streaming Data transfers.

pub mod component;
pub mod header;
pub mod ops_control;
pub mod ops_core;
pub mod ops_data;
pub mod ops_events;
pub mod session;

pub use component::*;
pub use header::*;
pub use ops_control::*;
pub use ops_core::*;
pub use ops_data::*;
pub use ops_events::*;
pub use session::*;
