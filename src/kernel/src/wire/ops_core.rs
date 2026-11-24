//! RigWire v2 Core Channel (`0x00`) Opcodes & Firmware-Agnostic Discovery Descriptors.

pub const OP_CORE_HELLO: u8 = 0x01;
pub const OP_CORE_PING: u8 = 0x02;
pub const OP_CORE_TIME_SYNC: u8 = 0x03;
pub const OP_CORE_AUTH: u8 = 0x04;
pub const OP_CORE_LEASE: u8 = 0x05;
pub const OP_CORE_PORT_CTRL: u8 = 0x06;
pub const OP_CORE_RUNLEVEL: u8 = 0x07;
pub const OP_CORE_GET_TIME: u8 = 0x08;
pub const OP_CORE_CAPS: u8 = 0x09;
pub const OP_CORE_TABLES: u8 = 0x0A;

/// Self-describing 16-byte transceiver capability facet (`CORE::CAPS`).
///
/// Firmware-agnostic: hosts discover tuners, PA, audio, display, serial ports,
/// storage, and loadable runtime nodes without hardcoding board or firmware IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct SemanticCap {
    pub domain: [u8; 4],
    pub driver_tag: [u8; 4],
    pub instance: u8,
    pub cap_rev: u8,
    pub bound_object: u16,
    pub flags: u32,
}

const _: () = assert!(core::mem::size_of::<SemanticCap>() == 16);

impl SemanticCap {
    pub const fn new(
        domain: [u8; 4],
        driver_tag: [u8; 4],
        instance: u8,
        cap_rev: u8,
        bound_object: u16,
        flags: u32,
    ) -> Self {
        Self {
            domain,
            driver_tag,
            instance,
            cap_rev,
            bound_object,
            flags,
        }
    }

    pub fn write_le(&self, dst: &mut [u8]) {
        dst[0..4].copy_from_slice(&self.domain);
        dst[4..8].copy_from_slice(&self.driver_tag);
        dst[8] = self.instance;
        dst[9] = self.cap_rev;
        dst[10..12].copy_from_slice(&self.bound_object.to_le_bytes());
        dst[12..16].copy_from_slice(&self.flags.to_le_bytes());
    }
}

/// Self-describing 12-byte binary storage table descriptor (`CORE::TABLES`).
///
/// Exposes fixed-stride power-of-two record tables (`CHAN`, `CONT`, `BNDP`, `ZONE`,
/// `HITS`, `FMBC`) generically to any RigWire v2 programming client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TableDesc {
    pub tag: [u8; 4],
    pub record_size: u16,
    pub capacity: u16,
    pub schema_ver: u8,
    pub flags: u8,
    pub _pad: u16,
}

const _: () = assert!(core::mem::size_of::<TableDesc>() == 12);

impl TableDesc {
    pub const FLAG_WRITABLE: u8 = 1 << 0;
    pub const FLAG_HASHABLE: u8 = 1 << 1;

    pub const fn new(
        tag: [u8; 4],
        record_size: u16,
        capacity: u16,
        schema_ver: u8,
        flags: u8,
    ) -> Self {
        Self {
            tag,
            record_size,
            capacity,
            schema_ver,
            flags,
            _pad: 0,
        }
    }

    pub fn write_le(&self, dst: &mut [u8]) {
        dst[0..4].copy_from_slice(&self.tag);
        dst[4..6].copy_from_slice(&self.record_size.to_le_bytes());
        dst[6..8].copy_from_slice(&self.capacity.to_le_bytes());
        dst[8] = self.schema_ver;
        dst[9] = self.flags;
        dst[10] = 0;
        dst[11] = 0;
    }
}
