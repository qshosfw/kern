//! RigWire Core Service Opcodes and Component Descriptors.

pub const OP_CORE_HELLO: u8 = 0x01;
pub const OP_CORE_GET_CAPS: u8 = 0x02;
pub const OP_CORE_PING: u8 = 0x03;
pub const OP_CORE_REBOOT: u8 = 0x04;
pub const OP_CORE_GET_STATUS: u8 = 0x05;
pub const OP_CORE_GET_RESULT: u8 = 0x06;

/// Standard Component Class IDs
pub const COMPONENT_CLASS_RADIO: u8 = 0x01;
pub const COMPONENT_CLASS_RF_CHAIN: u8 = 0x02;
pub const COMPONENT_CLASS_MCU: u8 = 0x03;
pub const COMPONENT_CLASS_DISPLAY: u8 = 0x04;
pub const COMPONENT_CLASS_GNSS: u8 = 0x05;
pub const COMPONENT_CLASS_AUDIO: u8 = 0x06;
pub const COMPONENT_CLASS_KEYPAD: u8 = 0x07;
pub const COMPONENT_CLASS_POWER: u8 = 0x08;
pub const COMPONENT_CLASS_SENSOR: u8 = 0x09;
pub const COMPONENT_CLASS_STORAGE: u8 = 0x0A;
pub const COMPONENT_CLASS_PERIPHERAL: u8 = 0x0B;
