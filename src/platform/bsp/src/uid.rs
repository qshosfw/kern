//! Hardware Unique ID (256-byte UID Space at 0x1FFF_3000) Introspection & Serialization.
//!
//! Provides 100% bitwise parity with DeltaFW for:
//! - 64-bit hardware serial number (`GetSerial()`)
//! - IEEE EUI-48 MAC address derivation (`GetMacAddress()`)
//! - Crockford Base32 serial string with Mod-37 Checksum (`GetCrockfordSerial()`)
//!
//! Also provides full 256-byte system memory UID access (`0x1FFF_3000`..`0x1FFF_30FF`)
//! and 256-byte hardware entropy fingerprinting for cryptographic seeding.

pub const UID_BASE_ADDR: usize = 0x1FFF3000;
pub const UID_BYTES: usize = 12;
pub const UID_FULL_BYTES: usize = 256;

const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHECKSUM_ALPHABET: &[u8; 37] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ*~$=U";

/// Reads the raw 96-bit (12-byte) hardware Unique ID from system memory.
pub fn read_mcu_uid() -> [u8; 12] {
    let mut uid = [0u8; 12];
    let ptr = UID_BASE_ADDR as *const u8;
    for i in 0..12 {
        uid[i] = unsafe { core::ptr::read_volatile(ptr.add(i)) };
    }
    uid
}

/// Reads the raw 16-byte CPU ID block used by DeltaFW for serial & KDF generation.
pub fn read_cpu_id_16() -> [u8; 16] {
    let mut id = [0u8; 16];
    let ptr = UID_BASE_ADDR as *const u8;
    for i in 0..16 {
        id[i] = unsafe { core::ptr::read_volatile(ptr.add(i)) };
    }
    id
}

/// Reads the complete 256-byte factory UID and system info space (0x1FFF3000..0x1FFF30FF).
pub fn read_uid_256() -> [u8; 256] {
    let mut full = [0u8; 256];
    let ptr = UID_BASE_ADDR as *const u8;
    for i in 0..256 {
        full[i] = unsafe { core::ptr::read_volatile(ptr.add(i)) };
    }
    full
}

/// MurmurHash3 64-bit finalizer (`fmix64`).
#[inline(always)]
fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^= k >> 33;
    k
}

/// Generates a deterministic 64-bit serial from the 16-byte CPU ID at 0x1FFF3000.
/// 100% bitwise identical to DeltaFW's `GetSerial()` in `identifier.c`.
pub fn get_hardware_serial64() -> u64 {
    let id_bytes = read_cpu_id_16();
    let k1 = u64::from_le_bytes([
        id_bytes[0],
        id_bytes[1],
        id_bytes[2],
        id_bytes[3],
        id_bytes[4],
        id_bytes[5],
        id_bytes[6],
        id_bytes[7],
    ]);
    let k2 = u64::from_le_bytes([
        id_bytes[8],
        id_bytes[9],
        id_bytes[10],
        id_bytes[11],
        id_bytes[12],
        id_bytes[13],
        id_bytes[14],
        id_bytes[15],
    ]);

    let mut h: u64 = 0x9e3779b97f4a7c15; // DeltaFW golden ratio seed

    for &k in &[k1, k2] {
        let mut k_mixed = k.wrapping_mul(0x87c37b91114253d5);
        k_mixed = k_mixed.rotate_left(31);
        k_mixed = k_mixed.wrapping_mul(0x4cf5ad432745937f);
        h ^= k_mixed;
        h = h.rotate_left(27);
        h = h.wrapping_mul(5).wrapping_add(0x52dce729);
    }

    // Finalize with length 16
    h ^= 16;
    fmix64(h)
}

/// Computes an IEEE EUI-48 (6-byte) MAC address derived from hardware serial.
/// 100% bitwise identical to DeltaFW's `GetMacAddress()`.
/// Format: 02:XX:XX:XX:XX:XX (Locally Administered, Unicast).
pub fn read_mac_address() -> [u8; 6] {
    let serial = get_hardware_serial64();
    let mut mac = [
        ((serial >> 40) & 0xFF) as u8,
        ((serial >> 32) & 0xFF) as u8,
        ((serial >> 24) & 0xFF) as u8,
        ((serial >> 16) & 0xFF) as u8,
        ((serial >> 8) & 0xFF) as u8,
        (serial & 0xFF) as u8,
    ];
    mac[0] |= 0x02; // Locally Administered
    mac[0] &= !0x01; // Unicast
    mac
}

static mut MAC_TAG_CACHED: [u8; 4] = [0; 4];

/// Computes the default 4-character hex tag from the last 2 bytes of the MAC address.
pub fn get_mac_tag() -> [u8; 4] {
    let mac = read_mac_address();
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    [
        HEX[(mac[4] >> 4) as usize],
        HEX[(mac[4] & 0x0F) as usize],
        HEX[(mac[5] >> 4) as usize],
        HEX[(mac[5] & 0x0F) as usize],
    ]
}

/// Returns the default 4-character hex tag string derived from the hardware MAC address.
pub fn get_mac_tag_str() -> &'static str {
    unsafe {
        let ptr = core::ptr::addr_of_mut!(MAC_TAG_CACHED);
        if (*ptr)[0] == 0 {
            *ptr = get_mac_tag();
        }
        core::str::from_utf8_unchecked(&*ptr)
    }
}

/// Generates a Crockford Base32 serial string with Mod-37 Checksum (e.g. "7K9MP3R2W8VFXC").
/// 100% bitwise identical to DeltaFW's `GetCrockfordSerial()`.
/// Returns a 15-byte null-terminated ASCII buffer (13 Crockford Base32 chars + 1 Mod-37 checksum + null).
pub fn get_crockford_serial() -> [u8; 15] {
    let serial = get_hardware_serial64();
    let mut val = serial;
    let mut out = [0u8; 15];

    // Encode 64 bits into 13 characters
    for i in (0..13).rev() {
        out[i] = CROCKFORD_ALPHABET[(val & 0x1F) as usize];
        val >>= 5;
    }

    let hi = (serial >> 32) as u32;
    let lo = serial as u32;
    let r_hi = utils::math::rem37_u32(hi) as u16;
    let r_lo = utils::math::rem37_u32(lo) as u16;
    let mod37 = utils::math::div37_u16(r_hi * 7 + r_lo).1 as usize;
    out[13] = CHECKSUM_ALPHABET[mod37];
    out[14] = b'\0';

    out
}

/// Hashes all 256 bytes of the UID memory space into a high-order 64-bit entropy fingerprint.
/// Ideal for cryptographic PRNG seeding and physical device fingerprinting.
pub fn get_hardware_fingerprint256() -> u64 {
    let raw = read_uid_256();
    let mut h: u64 = 0x9e3779b97f4a7c15;

    let mut i = 0;
    while i < 256 {
        let chunk = u64::from_le_bytes([
            raw[i],
            raw[i + 1],
            raw[i + 2],
            raw[i + 3],
            raw[i + 4],
            raw[i + 5],
            raw[i + 6],
            raw[i + 7],
        ]);
        let mut k = chunk.wrapping_mul(0x87c37b91114253d5);
        k = k.rotate_left(31);
        k = k.wrapping_mul(0x4cf5ad432745937f);
        h ^= k;
        h = h.rotate_left(27);
        h = h.wrapping_mul(5).wrapping_add(0x52dce729);
        i += 8;
    }

    h ^= 256;
    fmix64(h)
}

/// Formats the 96-bit MCU UID into a null-terminated 25-byte Hex ASCII string buffer.
pub fn read_usb_serial_ascii() -> [u8; 25] {
    let uid = read_mcu_uid();
    let mut buf = [0u8; 25];

    for i in 0..12 {
        let b = uid[i];
        buf[i * 2] = CHECKSUM_ALPHABET[(b >> 4) as usize];
        buf[i * 2 + 1] = CHECKSUM_ALPHABET[(b & 0x0F) as usize];
    }
    buf[24] = 0;
    buf
}

/// Queries whether the MCU Flash Readout Protection (RDP) is active (Level 1).
#[inline(always)]
pub fn is_mcu_read_protected() -> bool {
    py32_hal::flash::InternalFlash::is_rdp_protected()
}

/// Returns the active MCU Flash Readout Protection (RDP) level.
#[inline(always)]
pub fn get_mcu_rdp_level() -> py32_hal::flash::RdpLevel {
    py32_hal::flash::InternalFlash::rdp_status()
}
