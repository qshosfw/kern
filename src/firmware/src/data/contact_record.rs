//! 32-Byte Power-of-Two Universal Contact Record Format.
//!
//! Provides zero-heap, constant-stride ($O(1)$ random seek via `index << 5`) storage
//! for contacts and digital signaling identities. Aligned to 32 bytes so exactly 128
//! records fit in a 4096-byte flash sector without ever straddling sector boundaries.
//!
//! Universally supports:
//! - 48-bit (6-byte) MAC addresses & Mesh Node IDs
//! - M17 Base40 9-character callsigns
//! - DMR 24/32-bit Radio IDs & Talkgroups
//! - MDC1200 16-bit Unit IDs
//! - SelCall (CCIR/ZVEI) 5-tone sequences
//! - APRS callsigns & SSIDs
//! - Mesh neighbor telemetry (RSSI, battery, LQI, role)

/// 32-byte naturally-aligned, power-of-two contact record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
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

const _: () = {
    assert!(core::mem::size_of::<ContactRecord>() == 32);
    assert!(core::mem::align_of::<ContactRecord>() == 2);
};

impl ContactRecord {
    pub const SIZE: usize = 32;
    pub const STRIDE_SHIFT: u32 = 5; // 1 << 5 == 32 bytes

    // Contact Signaling Protocol Types
    pub const TYPE_GENERIC: u8 = 0;
    pub const TYPE_MESH_NODE: u8 = 1;
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

    // Node Roles (Digital / Mesh)
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

    /// Construct a contact with a 48-bit address (MAC, Mesh Node ID, or Base40).
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
        utils::format::ascii_str(&self.name[..len]).unwrap_or("")
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

    #[inline]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            address: [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]],
            contact_type: bytes[6],
            call_type: bytes[7],
            name: [
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15], bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21],
            ],
            sub_id: u16::from_le_bytes([bytes[22], bytes[23]]),
            flags: u16::from_le_bytes([bytes[24], bytes[25]]),
            last_rssi_dbm: bytes[26] as i8,
            battery_metric: bytes[27],
            role_capabilities: bytes[28],
            lqi: bytes[29],
            reserved: [bytes[30], bytes[31]],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_record_roundtrip() {
        let mut rec = ContactRecord::new_numeric(ContactRecord::TYPE_DMR, 0x0102_0304, "REPEATER");
        rec.sub_id = 0x1234;
        rec.flags = ContactRecord::FLAG_FAVORITE | ContactRecord::FLAG_AUTO_ACK;
        rec.last_rssi_dbm = -87;
        rec.battery_metric = 0x80 | 82;
        rec.role_capabilities = ContactRecord::ROLE_ROUTER;
        rec.lqi = 240;

        let decoded = ContactRecord::from_bytes(rec.as_bytes());
        assert_eq!(decoded, rec);
        assert_eq!(decoded.numeric_id(), 0x0102_0304);
        assert_eq!(decoded.name_as_str(), "REPEATER");
    }
}
