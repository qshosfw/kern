//! # KERN Microkernel Architecture
//!
//! KERN is a high-assurance, zero-allocation microkernel designed for resource-constrained
//! ARM Cortex-M microcontrollers.
//!
//! ## Core Subsystems
//! - [`app`]: Application traits (`App`), dependency-injected system context (`SysContext`), and service interfaces.
//! - [`caps`]: Zero-Sized Capability Tokens enforcing peripheral ownership.
//! - [`canvas`]: Hardware-agnostic 2D graphics rendering interface.
//! - [`event`]: Hardware, IPC, and UI event routing taxonomy.
//! - [`ipc`]: Lock-free SPSC message queues for thread/task communication.
//! - [`power`]: Power states, profiles, and reset/bootloader lifecycle (`PowerControl`).
//! - [`sched`]: Zero-alloc cooperative run-to-completion actor and priority scheduler.
//! - [`sys`]: System metrics and stack introspection (`SystemDiagnostics`).

#![no_std]

pub mod app;
pub mod boot;
pub mod caps;
pub mod dev;
pub mod error;
pub mod event;
pub mod ipc;
pub mod macros;
pub mod mem;
pub mod nav;
pub mod power;
pub mod proc;
pub mod runtime;
pub mod sched;
pub mod security;
pub mod service;
pub mod store;
pub mod sys;
pub mod tag;
pub mod ui;
pub mod wire;
