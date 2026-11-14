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
#[inline(always)]
pub fn read_mcu_uid() -> [u8; 12] {
    py32_hal::uid::SystemInfo::read_uid_12()
}

/// Reads the raw 16-byte CPU ID block used by DeltaFW for serial & KDF generation.
#[inline(always)]
pub fn read_cpu_id_16() -> [u8; 16] {
    py32_hal::uid::SystemInfo::read_uid_16()
}

/// Reads the complete 256-byte factory UID and system info space (0x1FFF3000..0x1FFF30FF).
#[inline(always)]
pub fn read_uid_256() -> [u8; 256] {
    py32_hal::uid::SystemInfo::read_uid_256()
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

/// Shared MurmurHash3 64-bit chunk-mixing accumulator over 8-byte little-endian blocks.
#[inline(never)]
fn murmur3_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15; // DeltaFW golden ratio seed
    let mut i = 0;
    let n = bytes.len() & !7;
    while i < n {
        let chunk = unsafe { u64::from_le_bytes(*bytes.as_ptr().add(i).cast::<[u8; 8]>()) };
        let mut k = chunk.wrapping_mul(0x87c37b91114253d5);
        k = k.rotate_left(31);
        k = k.wrapping_mul(0x4cf5ad432745937f);
        h ^= k;
        h = h.rotate_left(27);
        h = (h << 2).wrapping_add(h).wrapping_add(0x52dce729);
        i += 8;
    }
    h ^= bytes.len() as u64;
    fmix64(h)
}

/// Generates a deterministic 64-bit serial from the 16-byte CPU ID at 0x1FFF3000.
/// 100% bitwise identical to DeltaFW's `GetSerial()` in `identifier.c`.
pub fn get_hardware_serial64() -> u64 {
    let id_bytes = read_cpu_id_16();
    murmur3_64(&id_bytes)
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
    let hex = utils::format::HEX_CHARS;
    [
        hex[(mac[4] >> 4) as usize],
        hex[(mac[4] & 0x0F) as usize],
        hex[(mac[5] >> 4) as usize],
        hex[(mac[5] & 0x0F) as usize],
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

/// Encodes any 64-bit hardware identifier into a 15-byte null-terminated Crockford Base32
/// ASCII buffer (13 Crockford Base32 chars + 1 Mod-37 checksum + null).
#[inline(never)]
pub fn encode_crockford64(serial: u64) -> [u8; 15] {
    let mut val = serial;
    let mut out = [0u8; 15];

    // Encode 64 bits into 13 characters
    for i in (0..13).rev() {
        out[i] = CROCKFORD_ALPHABET[(val & 0x1F) as usize];
        val >>= 5;
    }

    let mut rem = 0u16;
    for shift in [48u8, 32, 16, 0] {
        rem = rem * 9 + utils::math::div37_u16((serial >> shift) as u16).1 as u16;
    }
    let mod37 = utils::math::div37_u16(rem).1 as usize;
    out[13] = unsafe { *CHECKSUM_ALPHABET.get_unchecked(mod37) };
    out[14] = b'\0';

    out
}

/// Generates a Crockford Base32 serial string with Mod-37 Checksum (e.g. "7K9MP3R2W8VFXC").
/// 100% bitwise identical to DeltaFW's `GetCrockfordSerial()`.
/// Returns a 15-byte null-terminated ASCII buffer (13 Crockford Base32 chars + 1 Mod-37 checksum + null).
#[inline(always)]
pub fn get_crockford_serial() -> [u8; 15] {
    encode_crockford64(get_hardware_serial64())
}

/// Computes a deterministic 64-bit hash over the 104-byte (`13 * 8` B) factory provisioning
/// certificate at `0x0800_2700..0x0800_2768` (80B UID-bound AES-128-CBC license + 24B factory stamp).
pub fn get_factory_cert_hash64() -> Option<u64> {
    py32_hal::uid::SystemInfo::factory_cert_slice().map(|cert| murmur3_64(cert))
}

/// Generates a 14-character Crockford Base32 + Mod-37 checksum serial number derived from the
/// 104-byte factory provisioning certificate at `0x0800_2700..0x0800_2768`.
#[inline(always)]
pub fn get_factory_serial() -> Option<[u8; 15]> {
    get_factory_cert_hash64().map(encode_crockford64)
}

/// Reads the live factory bootloader version string (e.g. `"7.02.02"`) from internal Flash.
#[inline(always)]
pub fn bootloader_version_str() -> &'static str {
    py32_hal::uid::SystemInfo::bootloader_version_str()
}

/// Hashes all 256 bytes of the UID memory space into a high-order 64-bit entropy fingerprint.
/// Ideal for cryptographic PRNG seeding and physical device fingerprinting.
pub fn get_hardware_fingerprint256() -> u64 {
    let raw = read_uid_256();
    murmur3_64(&raw)
}

/// Formats the 96-bit MCU UID into a null-terminated 25-byte Hex ASCII string buffer.
pub fn read_usb_serial_ascii() -> [u8; 25] {
    let hex = utils::format::HEX_CHARS;
    let uid = read_mcu_uid();
    let mut buf = [0u8; 25];

    for i in 0..12 {
        let b = uid[i];
        buf[i * 2] = hex[(b >> 4) as usize];
        buf[i * 2 + 1] = hex[(b & 0x0F) as usize];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmur3_64_matches_legacy_16_byte_mixing() {
        let input = *b"0123456789ABCDEF";
        let k1 = u64::from_le_bytes(input[0..8].try_into().unwrap());
        let k2 = u64::from_le_bytes(input[8..16].try_into().unwrap());
        let mut h: u64 = 0x9e3779b97f4a7c15;
        for &k in &[k1, k2] {
            let mut km = k.wrapping_mul(0x87c37b91114253d5);
            km = km.rotate_left(31);
            km = km.wrapping_mul(0x4cf5ad432745937f);
            h ^= km;
            h = h.rotate_left(27);
            h = h.wrapping_mul(5).wrapping_add(0x52dce729);
        }
        h ^= 16;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51afd7ed558ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
        h ^= h >> 33;
        assert_eq!(murmur3_64(&input), h);
    }

    #[test]
    fn factory_cert_serial_and_bootloader_version_are_deterministic() {
        let serial = get_factory_serial().expect("host fixture provides 104B factory cert");
        assert_eq!(serial[14], 0);
        assert!(serial[..14].iter().all(|b| b.is_ascii_graphic()));
        assert_ne!(serial, get_crockford_serial());
        assert_eq!(bootloader_version_str(), "7.02.02");
    }
}
