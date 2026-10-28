//! Declarative Component Interface for RigWire.
//!
//! Enables microkernel services, hardware peripherals, and hosted applications
//! to expose their telemetry, configuration state, and actions declaratively
//! without hardcoding application or device specifics into wire transport.

use super::header::WireStatus;

pub const COMPONENT_SYSTEM: u8 = 0x00;
pub const COMPONENT_RADIO: u8 = 0x01;
pub const COMPONENT_POWER: u8 = 0x02;
pub const COMPONENT_AUDIO: u8 = 0x03;
pub const COMPONENT_APP_BASE: u8 = 0x10;

/// Declarative facet handler for a system component or hosted application.
pub trait WireComponent {
    /// Logical component identifier (§8.6.2).
    fn component_id(&self) -> u8;

    /// Read a component state property into `out`. Returns payload byte length on success.
    fn get_state(&self, property_id: u16, out: &mut [u8]) -> Result<usize, WireStatus>;

    /// Set a component state property from `value`.
    fn set_state(&mut self, property_id: u16, value: &[u8]) -> Result<(), WireStatus>;

    /// Invoke a component action with given `params`.
    fn invoke_action(&mut self, action_id: u16, params: &[u8]) -> Result<(), WireStatus>;
}
