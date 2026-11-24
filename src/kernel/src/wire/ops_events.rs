//! RigWire v2 Events Channel (`CH_EVENTS = 0x02`) Opcodes & Event IDs.

use abi::Status;

pub const OP_EVENTS_SUBSCRIBE: u8 = 0x01;
pub const OP_EVENTS_UNSUBSCRIBE: u8 = 0x02;
pub const OP_EVENTS_CREDIT: u8 = 0x03;
pub const OP_EVENTS_EMIT: u8 = 0x80;

pub const EVENT_SQUELCH_OPEN: u16 = 0x0001;
pub const EVENT_SQUELCH_CLOSE: u16 = 0x0002;
pub const EVENT_BATTERY_UPDATE: u16 = 0x0003;
pub const EVENT_KEY_PRESS: u16 = 0x0004;
pub const EVENT_APP_CHANGED: u16 = 0x0005;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventSubscribeReq {
    pub component_id: u8,
    pub event_mask: u32,
    pub initial_credits: u16,
}

impl EventSubscribeReq {
    pub fn parse(payload: &[u8]) -> Result<Self, Status> {
        if payload.len() < 7 {
            return Err(Status::BadLength);
        }
        Ok(Self {
            component_id: payload[0],
            event_mask: u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]),
            initial_credits: u16::from_le_bytes([payload[5], payload[6]]),
        })
    }
}
