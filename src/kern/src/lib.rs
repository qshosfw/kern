//! # KERN Microkernel Architecture
//!
//! KERN is a high-assurance, zero-allocation microkernel designed for resource-constrained
//! ARM Cortex-M microcontrollers.
//!
//! ## Core Subsystems
//! - [`app`]: Application traits (`App`), dependency-injected system context (`SysContext`), and service interfaces.
//! - [`caps`]: Zero-Sized Capability Tokens (`HwCapCrypto`, `HwCapRadio`, etc.) enforcing peripheral ownership.
//! - [`canvas`]: Hardware-agnostic 2D graphics rendering interface.
//! - [`event`]: Hardware, IPC, and UI event routing taxonomy.
//! - [`ipc`]: Lock-free SPSC message queues for thread/task communication.
//! - [`sys`]: System metrics, stack introspection, and crash reporting.
//! - [`task`]: Zero-alloc coroutine task scheduler.

#![no_std]

pub mod app;
pub mod canvas;
pub mod caps;
pub mod dev;
pub mod event;
pub mod flight;
pub mod ipc;
pub mod macros;
pub mod power;
pub mod security;
pub mod service;
pub mod sys;
pub mod task;
