//! RigWire Control & Telemetry Service Opcodes and Request Parsers.

use super::header::WireStatus;

pub const OP_CONTROL_STATE_GET: u8 = 0x01;
pub const OP_CONTROL_STATE_SET: u8 = 0x02;
pub const OP_CONTROL_ACTION_INVOKE: u8 = 0x03;
pub const OP_CONTROL_ACTION_CANCEL: u8 = 0x04;
pub const OP_CONTROL_APP_LAUNCH: u8 = 0x05;

// Radio Properties (Component 0x01)
pub const PROP_RADIO_RX_FREQ_HZ: u16 = 0x0001;
pub const PROP_RADIO_TX_FREQ_HZ: u16 = 0x0002;
pub const PROP_RADIO_MODULATION_MODE: u16 = 0x0003;
pub const PROP_RADIO_BANDWIDTH_HZ: u16 = 0x0004;
pub const PROP_RADIO_SELECTED_VFO: u16 = 0x0005;
pub const PROP_RADIO_SELECTED_CHANNEL: u16 = 0x0006;
pub const PROP_RADIO_POWER_LEVEL: u16 = 0x0007;
pub const PROP_RADIO_SQUELCH_LEVEL: u16 = 0x0008;
pub const PROP_RADIO_SCAN_STATE: u16 = 0x0009;
pub const PROP_RADIO_PTT_ACTIVE: u16 = 0x000A;
pub const PROP_RADIO_BATTERY_MV: u16 = 0x000B;
pub const PROP_RADIO_RSSI_DBM: u16 = 0x000C;
pub const PROP_RADIO_FINE_OFFSET_DHZ: u16 = 0x000D;

// System Properties (Component 0x00)
pub const PROP_SYS_UPTIME_MS: u16 = 0x0020;
pub const PROP_SYS_KEYPAD_LOCKED: u16 = 0x0021;
pub const PROP_SYS_FLASHLIGHT_ACTIVE: u16 = 0x0022;
pub const PROP_SYS_ACTIVE_APP: u16 = 0x0023;
pub const PROP_SYS_MCU_UID: u16 = 0x0024;
pub const PROP_SYS_SYSCLK_HZ: u16 = 0x0025;
pub const PROP_SYS_CALIB_VALID: u16 = 0x0026;

// Audio Properties (Component 0x03)
pub const PROP_AUDIO_VOLUME: u16 = 0x0030;
pub const PROP_AUDIO_KEY_BEEP: u16 = 0x0031;
pub const PROP_AUDIO_ROGER_BEEP: u16 = 0x0032;

// Standard Actions (§8.5 / §8.6)
pub const ACTION_FLASHLIGHT_TOGGLE: u16 = 0x0001;
pub const ACTION_VFO_TOGGLE: u16 = 0x0002;
pub const ACTION_PLAY_BEEP: u16 = 0x0003;
pub const ACTION_KEYPAD_LOCK_TOGGLE: u16 = 0x0004;
pub const ACTION_REBOOT: u16 = 0x0005;
pub const ACTION_SCAN_TOGGLE: u16 = 0x0006;
pub const ACTION_APP_LAUNCH: u16 = 0x0007;
pub const ACTION_PTT_TRANSMIT: u16 = 0x0008;

/// Control Service: Parsed STATE_GET request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateGetReq<'a> {
    pub component_id: u8,
    pub property_id: u16,
    pub query_params: &'a [u8],
}

impl<'a> StateGetReq<'a> {
    pub fn parse(payload: &'a [u8]) -> Result<Self, WireStatus> {
        if payload.len() < 3 {
            return Err(WireStatus::BadLength);
        }
        let component_id = payload[0];
        let property_id = u16::from_le_bytes([payload[1], payload[2]]);
        let query_params = &payload[3..];
        Ok(Self {
            component_id,
            property_id,
            query_params,
        })
    }
}

/// Control Service: Parsed STATE_SET request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateSetReq<'a> {
    pub component_id: u8,
    pub expected_revision: u32,
    pub is_dry_run: bool,
    pub property_id: u16,
    pub value_bytes: &'a [u8],
}

impl<'a> StateSetReq<'a> {
    pub fn parse(payload: &'a [u8]) -> Result<Self, WireStatus> {
        if payload.len() < 8 {
            return Err(WireStatus::BadLength);
        }
        let component_id = payload[0];
        let flags = payload[1];
        let is_dry_run = (flags & 0x01) != 0;
        let expected_revision =
            u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
        let property_id = u16::from_le_bytes([payload[6], payload[7]]);
        let value_bytes = &payload[8..];
        Ok(Self {
            component_id,
            expected_revision,
            is_dry_run,
            property_id,
            value_bytes,
        })
    }
}

/// Control Service: Parsed ACTION_INVOKE request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionInvokeReq<'a> {
    pub component_id: u8,
    pub action_id: u16,
    pub action_params: &'a [u8],
}

impl<'a> ActionInvokeReq<'a> {
    pub fn parse(payload: &'a [u8]) -> Result<Self, WireStatus> {
        if payload.len() < 3 {
            return Err(WireStatus::BadLength);
        }
        let component_id = payload[0];
        let action_id = u16::from_le_bytes([payload[1], payload[2]]);
        let action_params = &payload[3..];
        Ok(Self {
            component_id,
            action_id,
            action_params,
        })
    }
}
