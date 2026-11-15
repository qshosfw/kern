//! RigWire Microkernel Resource Handle & Facet Service Opcodes and Parsers.

use super::header::WireStatus;

// Opcodes (§8.5)
pub const OP_HANDLE_GET: u8 = 0x01; // Read state from handle
pub const OP_HANDLE_SET: u8 = 0x02; // Write state to handle
pub const OP_HANDLE_INVOKE: u8 = 0x03; // Invoke action on handle
pub const OP_HANDLE_STREAM: u8 = 0x04; // Stream data to/from handle

// Standard Resource Domains (§8.5)
pub const DOMAIN_SYSTEM: u8 = 0x00;
pub const DOMAIN_RADIO: u8 = 0x01;
pub const DOMAIN_UI: u8 = 0x02;
pub const DOMAIN_INPUT: u8 = 0x03;
pub const DOMAIN_CONFIG: u8 = 0x04;
pub const DOMAIN_APP: u8 = 0x05;
pub const DOMAIN_STORAGE: u8 = 0x06;

// ── DOMAIN_SYSTEM (0x00) ──────────────────────────────────────────────────────
pub const FACET_SYS_IDENTITY: u16 = 0x0001;
pub const FACET_SYS_METRICS: u16 = 0x0002;
pub const FACET_SYS_CALIBRATION: u16 = 0x0003;

pub const ACTION_SYS_REBOOT: u16 = 0x0001;
pub const ACTION_SYS_FLASHLIGHT: u16 = 0x0002;

// ── DOMAIN_RADIO (0x01) ───────────────────────────────────────────────────────
pub const RADIO_INSTANCE_ACTIVE: u8 = 0x00;
pub const RADIO_INSTANCE_VFO_A: u8 = 0x01;
pub const RADIO_INSTANCE_VFO_B: u8 = 0x02;
pub const RADIO_INSTANCE_TUNER_SEC: u8 = 0x03;

pub const FACET_RADIO_TUNING: u16 = 0x0001; // Freq Hz, Step
pub const FACET_RADIO_MODULATION: u16 = 0x0002; // Mode, Bandwidth
pub const FACET_RADIO_SQUELCH: u16 = 0x0003; // SQL, RSSI, Carrier
pub const FACET_RADIO_TRANSMIT: u16 = 0x0004; // PTT, Power
pub const FACET_RADIO_DIGITAL: u16 = 0x0005; // Vernier sub-Hz carrier / Continuous-phase FSK

pub const ACTION_RADIO_VFO_TOGGLE: u16 = 0x0001;

// ── DOMAIN_UI (0x02) ──────────────────────────────────────────────────────────
pub const UI_INSTANCE_PRIMARY: u8 = 0x00;

pub const FACET_UI_SURFACE: u16 = 0x0001; // Framebuffer chunk readback / contrast / invert
pub const FACET_UI_MODAL: u16 = 0x0002; // Active dialog inspection & dismissal
pub const FACET_UI_VIEW: u16 = 0x0003; // View navigation / active app name

pub const ACTION_MODAL_DISMISS: u16 = 0x0001;
pub const ACTION_MODAL_CONFIRM: u16 = 0x0002;
pub const ACTION_MODAL_CANCEL: u16 = 0x0003;
pub const ACTION_VIEW_LAUNCHER: u16 = 0x0004;

// ── DOMAIN_INPUT (0x03) ───────────────────────────────────────────────────────
pub const INPUT_INSTANCE_KEYPAD: u8 = 0x00;

pub const FACET_INPUT_KEY: u16 = 0x0001; // Inject KeyDown / KeyUp / KeyHold
pub const FACET_INPUT_LOCK: u16 = 0x0002; // Keypad lock state

pub const ACTION_INPUT_LOCK_TOGGLE: u16 = 0x0001;

// ── DOMAIN_CONFIG (0x04) ──────────────────────────────────────────────────────
pub const CONFIG_DOMAIN_DISPLAY: u8 = 0x00;
pub const CONFIG_DOMAIN_AUDIO: u8 = 0x01;
pub const CONFIG_DOMAIN_KEYS: u8 = 0x02;
pub const CONFIG_DOMAIN_SYSTEM: u8 = 0x03;
pub const CONFIG_DOMAIN_BATTERY: u8 = 0x04;
pub const CONFIG_DOMAIN_APPS: u8 = 0x05;

// ── DOMAIN_APP (0x05) ─────────────────────────────────────────────────────────
pub const FACET_APP_REGISTRY: u16 = 0x0001;
pub const FACET_APP_LIFECYCLE: u16 = 0x0002;

pub const ACTION_APP_LAUNCH: u16 = 0x0001;
pub const ACTION_APP_KILL: u16 = 0x0002;

/// Standard Microkernel Resource Handle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ResourceHandle {
    pub domain: u8,
    pub instance: u8,
    pub facet: u16,
}

impl ResourceHandle {
    pub const fn new(domain: u8, instance: u8, facet: u16) -> Self {
        Self {
            domain,
            instance,
            facet,
        }
    }

    pub fn parse_from(r: &mut utils::bytes::ByteReader<'_>) -> Option<Self> {
        Some(Self {
            domain: r.read_u8()?,
            instance: r.read_u8()?,
            facet: r.read_u16_le()?,
        })
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        Self::parse_from(&mut utils::bytes::ByteReader::new(bytes))
    }

    pub fn write_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < 4 {
            return 0;
        }
        out[0] = self.domain;
        out[1] = self.instance;
        out[2..4].copy_from_slice(&self.facet.to_le_bytes());
        4
    }
}

/// Parsed HANDLE_GET Request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleGetReq<'a> {
    pub handle: ResourceHandle,
    pub params: &'a [u8],
}

impl<'a> HandleGetReq<'a> {
    pub fn parse(payload: &'a [u8]) -> Result<Self, WireStatus> {
        let handle = ResourceHandle::parse(payload).ok_or(WireStatus::BadLength)?;
        let params = &payload[4..];
        Ok(Self { handle, params })
    }
}

/// Parsed HANDLE_SET Request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleSetReq<'a> {
    pub handle: ResourceHandle,
    pub value: &'a [u8],
}

impl<'a> HandleSetReq<'a> {
    pub fn parse(payload: &'a [u8]) -> Result<Self, WireStatus> {
        let handle = ResourceHandle::parse(payload).ok_or(WireStatus::BadLength)?;
        let value = &payload[4..];
        Ok(Self { handle, value })
    }
}

/// Parsed HANDLE_INVOKE Request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleInvokeReq<'a> {
    pub handle: ResourceHandle,
    pub action_id: u16,
    pub params: &'a [u8],
}

impl<'a> HandleInvokeReq<'a> {
    pub fn parse(payload: &'a [u8]) -> Result<Self, WireStatus> {
        let mut r = utils::bytes::ByteReader::new(payload);
        let handle = ResourceHandle::parse_from(&mut r).ok_or(WireStatus::BadLength)?;
        let action_id = r.read_u16_le().ok_or(WireStatus::BadLength)?;
        Ok(Self {
            handle,
            action_id,
            params: r.remaining_slice(),
        })
    }
}

/// Parsed HANDLE_STREAM Request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleStreamReq {
    pub handle: ResourceHandle,
    pub offset: u32,
    pub length: u16,
}

impl HandleStreamReq {
    pub fn parse(payload: &[u8]) -> Result<Self, WireStatus> {
        let mut r = utils::bytes::ByteReader::new(payload);
        let handle = ResourceHandle::parse_from(&mut r).ok_or(WireStatus::BadLength)?;
        let offset = r.read_u32_le().ok_or(WireStatus::BadLength)?;
        let length = r.read_u16_le().ok_or(WireStatus::BadLength)?;
        Ok(Self {
            handle,
            offset,
            length,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_handle_roundtrip() {
        let h = ResourceHandle::new(DOMAIN_RADIO, RADIO_INSTANCE_VFO_A, FACET_RADIO_TUNING);
        let mut buf = [0u8; 4];
        assert_eq!(h.write_bytes(&mut buf), 4);
        assert_eq!(buf, [DOMAIN_RADIO, RADIO_INSTANCE_VFO_A, 0x01, 0x00]);

        let parsed = ResourceHandle::parse(&buf).unwrap();
        assert_eq!(parsed, h);
    }

    #[test]
    fn test_handle_requests_parse() {
        let get_raw = [DOMAIN_UI, UI_INSTANCE_PRIMARY, 0x01, 0x00, 0xAA, 0xBB];
        let get_req = HandleGetReq::parse(&get_raw).unwrap();
        assert_eq!(get_req.handle.domain, DOMAIN_UI);
        assert_eq!(get_req.params, &[0xAA, 0xBB]);

        let set_raw = [
            DOMAIN_RADIO,
            RADIO_INSTANCE_ACTIVE,
            0x01,
            0x00,
            0x10,
            0x20,
            0x30,
            0x40,
        ];
        let set_req = HandleSetReq::parse(&set_raw).unwrap();
        assert_eq!(set_req.handle.domain, DOMAIN_RADIO);
        assert_eq!(set_req.value, &[0x10, 0x20, 0x30, 0x40]);

        let inv_raw = [
            DOMAIN_INPUT,
            INPUT_INSTANCE_KEYPAD,
            0x01,
            0x00,
            0x05,
            0x00,
            0xFF,
        ];
        let inv_req = HandleInvokeReq::parse(&inv_raw).unwrap();
        assert_eq!(inv_req.action_id, 5);
        assert_eq!(inv_req.params, &[0xFF]);
    }
}
