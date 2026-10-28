//! 32-Byte Power-of-Two Universal Contact Record Format.
//!
//! Provides zero-heap, constant-stride ($O(1)$ random seek via `index << 5`) storage
//! for contacts and digital signaling identities. Aligned to 32 bytes so exactly 128
//! records fit in a 4096-byte flash sector without ever straddling sector boundaries.
//!
//! Universally supports:
//! - 48-bit (6-byte) MAC addresses & Hermes Link Node IDs
//! - M17 Base40 9-character callsigns
//! - DMR 24/32-bit Radio IDs & Talkgroups
//! - MDC1200 16-bit Unit IDs
//! - SelCall (CCIR/ZVEI) 5-tone sequences
//! - APRS callsigns & SSIDs
//! - Mesh neighbor telemetry (RSSI, battery, LQI, role)

/// 32-byte packed, power-of-two aligned contact record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct ContactRecord {
    pub address: [u8; 6],
    pub contact_type: u8,
    pub call_type: u8,
    pub name: [u8; 14],
    pub sub_id: u16,
    pub flags: u16,
    pub last_rssi_dbm: i8,
    pub battery_metric: u8,
    pub role_capabilities: u8,
    pub lqi: u8,
    pub reserved: [u8; 2],
}

const _: () = assert!(core::mem::size_of::<ContactRecord>() == 32);

impl ContactRecord {
    pub const SIZE: usize = 32;
    pub const STRIDE_SHIFT: u32 = 5; // 1 << 5 == 32 bytes

    // Contact Signaling Protocol Types
    pub const TYPE_GENERIC: u8 = 0;
    pub const TYPE_HERMES_MESH: u8 = 1;
    pub const TYPE_M17: u8 = 2;
    pub const TYPE_APRS: u8 = 3;
    pub const TYPE_DMR: u8 = 4;
    pub const TYPE_MDC1200: u8 = 5;
    pub const TYPE_SELCALL: u8 = 6;
    pub const TYPE_PHONE_DTMF: u8 = 7;

    // Call Types
    pub const CALL_PRIVATE: u8 = 0;
    pub const CALL_GROUP: u8 = 1;
    pub const CALL_BROADCAST: u8 = 2;
    pub const CALL_EMERGENCY: u8 = 3;

    // Option Bitflags
    pub const FLAG_MUTE_UNTIL_MATCH: u16 = 1 << 0;
    pub const FLAG_FAVORITE: u16 = 1 << 1;
    pub const FLAG_ACTIVE_NEIGHBOR: u16 = 1 << 2;
    pub const FLAG_ENCRYPTED: u16 = 1 << 3;
    pub const FLAG_AUTO_ACK: u16 = 1 << 4;

    // Node Roles (Hermes / Mesh)
    pub const ROLE_CLIENT: u8 = 0;
    pub const ROLE_ROUTER: u8 = 1;
    pub const ROLE_TRACKER: u8 = 2;
    pub const ROLE_GATEWAY: u8 = 3;

    pub const fn empty() -> Self {
        Self {
            address: [0; 6],
            contact_type: Self::TYPE_GENERIC,
            call_type: Self::CALL_PRIVATE,
            name: [0; 14],
            sub_id: 0,
            flags: 0,
            last_rssi_dbm: -128,
            battery_metric: 0,
            role_capabilities: 0,
            lqi: 0,
            reserved: [0; 2],
        }
    }

    /// Construct a contact with a 48-bit address (MAC, Hermes Node ID, or Base40).
    pub fn new_node(contact_type: u8, address: [u8; 6], name: &str) -> Self {
        let mut rec = Self::empty();
        rec.contact_type = contact_type;
        rec.address = address;
        rec.set_name(name);
        rec
    }

    /// Construct a legacy numeric contact (DMR, MDC1200, SelCall).
    pub fn new_numeric(contact_type: u8, id: u32, name: &str) -> Self {
        let mut rec = Self::empty();
        rec.contact_type = contact_type;
        rec.address[2..6].copy_from_slice(&id.to_be_bytes());
        rec.set_name(name);
        rec
    }

    #[inline(always)]
    pub fn numeric_id(&self) -> u32 {
        u32::from_be_bytes([
            self.address[2],
            self.address[3],
            self.address[4],
            self.address[5],
        ])
    }

    pub fn name_as_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        unsafe { core::str::from_utf8_unchecked(&self.name[..len]) }
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 14];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(13);
        self.name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    #[inline(always)]
    pub fn has_battery(&self) -> bool {
        (self.battery_metric & 0x80) != 0
    }

    #[inline(always)]
    pub fn battery_voltage_x10(&self) -> u8 {
        self.battery_metric & 0x7F
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        unsafe { &*(self as *const Self as *const [u8; 32]) }
    }

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 32] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 32]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}
