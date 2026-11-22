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

utils::static_bytes!(
    static CHECKSUM_ALPHABET = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ*~$=U";
);

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

/// Shared MurmurHash3 64-bit chunk-mixing accumulator over aligned 32-bit little-endian word pairs.
#[inline(never)]
fn murmur3_64_words(words: &[u32]) -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15; // DeltaFW golden ratio seed
    let mut i = 0;
    let n = words.len() & !1;
    while i < n {
        let chunk = (words[i] as u64) | ((words[i + 1] as u64) << 32);
        let mut k = chunk.wrapping_mul(0x87c37b91114253d5);
        k = k.rotate_left(31);
        k = k.wrapping_mul(0x4cf5ad432745937f);
        h ^= k;
        h = h.rotate_left(27);
        h = (h << 2).wrapping_add(h).wrapping_add(0x52dce729);
        i += 2;
    }
    h ^= (words.len() * 4) as u64;
    fmix64(h)
}

#[cfg(test)]
fn murmur3_64(bytes: &[u8]) -> u64 {
    let mut words = [0u32; 64];
    let n = (bytes.len() / 4).min(64);
    for i in 0..n {
        words[i] = u32::from_le_bytes([
            bytes[i * 4],
            bytes[i * 4 + 1],
            bytes[i * 4 + 2],
            bytes[i * 4 + 3],
        ]);
    }
    murmur3_64_words(&words[..n])
}

/// Generates a deterministic 64-bit serial from the 16-byte CPU ID at 0x1FFF3000.
/// 100% bitwise identical to DeltaFW's `GetSerial()` in `identifier.c`.
#[inline(never)]
pub fn get_hardware_serial64() -> u64 {
    let id_bytes = read_cpu_id_16();
    let mut words = [0u32; 4];
    // SAFETY: both arrays are 16 bytes; copying into `[u32; 4]` guarantees 4-byte alignment on M0+.
    unsafe {
        core::ptr::copy_nonoverlapping(id_bytes.as_ptr(), words.as_mut_ptr().cast::<u8>(), 16);
    }
    #[cfg(target_endian = "big")]
    for w in &mut words {
        *w = w.swap_bytes();
    }
    murmur3_64_words(&words)
}

/// Derives an IEEE EUI-48 (6-byte) MAC address from a precomputed 64-bit hardware serial into `dst`.
#[inline(never)]
pub fn mac_from_serial64_into(serial: u64, dst: &mut [u8; 6]) {
    let bytes = serial.to_be_bytes();
    for i in 0..6 {
        dst[i] = bytes[i + 2];
    }
    dst[0] = (dst[0] | 0x02) & !0x01;
}

/// Derives an IEEE EUI-48 (6-byte) MAC address from a precomputed 64-bit hardware serial.
#[inline(always)]
pub fn mac_from_serial64(serial: u64) -> [u8; 6] {
    let mut mac = [0u8; 6];
    mac_from_serial64_into(serial, &mut mac);
    mac
}

/// Computes an IEEE EUI-48 (6-byte) MAC address derived from hardware serial.
/// 100% bitwise identical to DeltaFW's `GetMacAddress()`.
/// Format: 02:XX:XX:XX:XX:XX (Locally Administered, Unicast).
#[inline(always)]
pub fn read_mac_address() -> [u8; 6] {
    mac_from_serial64(get_hardware_serial64())
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

/// Encodes any 64-bit hardware identifier into `dst` (13 Crockford Base32 chars + 1 Mod-37 checksum + NUL).
#[inline(never)]
pub fn encode_crockford64_into(serial: u64, dst: &mut [u8; 15]) {
    let (mut lo, mut hi) = (serial as u32, (serial >> 32) as u32);
    let ptr = dst.as_mut_ptr();

    // Encode 64 bits into 13 characters using 32-bit shifts (avoids __aeabi_llsr on Cortex-M0+).
    for i in (0..13).rev() {
        unsafe { *ptr.add(i) = *CHECKSUM_ALPHABET.get_unchecked((lo & 0x1F) as usize) };
        lo = (lo >> 5) | (hi << 27);
        hi >>= 5;
    }

    let (s_lo, s_hi) = (serial as u32, (serial >> 32) as u32);
    let mut rem = 0u16;
    for limb in [
        (s_hi >> 16) as u16,
        s_hi as u16,
        (s_lo >> 16) as u16,
        s_lo as u16,
    ] {
        rem = rem * 9 + utils::math::div37_u16(limb).1 as u16;
    }
    let mod37 = utils::math::div37_u16(rem).1 as usize;
    unsafe {
        *ptr.add(13) = *CHECKSUM_ALPHABET.get_unchecked(mod37);
        *ptr.add(14) = b'\0';
    }
}

/// Encodes any 64-bit hardware identifier into a 15-byte null-terminated Crockford Base32
/// ASCII buffer (13 Crockford Base32 chars + 1 Mod-37 checksum + null).
#[inline(always)]
pub fn encode_crockford64(serial: u64) -> [u8; 15] {
    let mut out = core::mem::MaybeUninit::<[u8; 15]>::uninit();
    unsafe {
        encode_crockford64_into(serial, &mut *out.as_mut_ptr());
        out.assume_init()
    }
}

/// Generates a Crockford Base32 serial string with Mod-37 Checksum (e.g. "7K9MP3R2W8VFXC").
/// 100% bitwise identical to DeltaFW's `GetCrockfordSerial()`.
/// Returns a 15-byte null-terminated ASCII buffer (13 Crockford Base32 chars + 1 Mod-37 checksum + null).
#[inline(always)]
pub fn get_crockford_serial() -> [u8; 15] {
    encode_crockford64(get_hardware_serial64())
}

/// Status and optional raw ASCII serial extracted from the 104-byte factory provisioning
/// certificate at `0x0800_2700..0x0800_2768`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactoryCertStatus {
    /// Provisioning block erased (`0xFF`) or absent.
    Erased,
    /// Provisioning block present without a readable `1-sq` ASCII serial.
    Present,
    /// Valid `1-sq` block containing a 1..=16 byte ASCII serial in `cert[84..100]`.
    Qs1Ascii { buf: [u8; 16], len: u8 },
}

impl FactoryCertStatus {
    /// Returns the display string suitable for Settings UI (`<= 16` chars).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Erased => "None",
            Self::Present => "Present",
            Self::Qs1Ascii { buf, len } => {
                let n = (*len as usize).min(buf.len());
                utils::format::ascii_str(&buf[..n]).unwrap_or("Present")
            }
        }
    }
}

fn find_qs1_tag_end(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let v = u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        if (v | 0x2020_0000) == u32::from_le_bytes(*b"1-sq")
            || (v | 0x0000_2020) == u32::from_le_bytes(*b"qs-1")
        {
            return Some(i + 4);
        }
        i += 1;
    }
    None
}

/// Scans `bytes` for a `1-sq` / `qs-1` tag (case-insensitive) and returns a slice of up to 16
/// printable serial bytes, or falls back to an un-tagged `>= 8`-char ASCII run in `[80..]` if present.
#[inline(never)]
pub fn extract_qs1_slice(bytes: &[u8]) -> Option<&[u8]> {
    let tag_end = find_qs1_tag_end(bytes);
    let mut start = tag_end.unwrap_or(80);
    if start >= bytes.len() {
        return None;
    }

    while start < bytes.len() && matches!(bytes[start], 0x00 | 0xFF | b' ' | b':' | b'=') {
        start += 1;
    }

    let max = (bytes.len() - start).min(16);
    let mut len = 0usize;
    while len < max {
        let b = bytes[start + len];
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/') {
            len += 1;
        } else {
            break;
        }
    }

    let min_len = if tag_end.is_some() { 1 } else { 8 };
    (len >= min_len).then(|| &bytes[start..start + len])
}

/// Parses a 104-byte factory provisioning certificate into its UI status and raw `1-sq` ASCII S/N.
pub fn parse_factory_cert_status(cert: Option<&[u8; 104]>) -> FactoryCertStatus {
    let Some(cert) = cert else {
        return FactoryCertStatus::Erased;
    };
    if let Some(s) = extract_qs1_slice(cert) {
        let mut buf = [0u8; 16];
        buf[..s.len()].copy_from_slice(s);
        FactoryCertStatus::Qs1Ascii {
            buf,
            len: s.len() as u8,
        }
    } else {
        FactoryCertStatus::Present
    }
}

/// Reads and parses the 104-byte factory provisioning certificate at `0x0800_2700..0x0800_2768`.
#[inline(always)]
pub fn factory_cert_status() -> FactoryCertStatus {
    parse_factory_cert_status(py32_hal::uid::SystemInfo::factory_cert_slice())
}

/// Reads the live factory bootloader version string (e.g. `"7.02.02"`) from internal Flash.
#[inline(always)]
pub fn bootloader_version_str() -> &'static str {
    py32_hal::uid::SystemInfo::bootloader_version_str()
}

/// Reads the factory bootloader's USB VID, PID, and product descriptor name from Flash.
#[inline(always)]
pub fn bootloader_usb_info() -> py32_hal::uid::BootloaderUsbInfo {
    py32_hal::uid::SystemInfo::bootloader_usb_info()
}

/// Hashes all 256 bytes of the UID memory space into a high-order 64-bit entropy fingerprint.
/// Ideal for cryptographic PRNG seeding and physical device fingerprinting.
pub fn get_hardware_fingerprint256() -> u64 {
    let raw = read_uid_256();
    let mut words = [0u32; 64];
    unsafe {
        core::ptr::copy_nonoverlapping(raw.as_ptr(), words.as_mut_ptr().cast::<u8>(), 256);
    }
    #[cfg(target_endian = "big")]
    for w in &mut words {
        *w = w.swap_bytes();
    }
    murmur3_64_words(&words)
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
    fn factory_cert_status_parses_qs1_and_handles_edge_cases() {
        assert_eq!(bootloader_version_str(), "7.02.02");
        let status = factory_cert_status();
        assert_eq!(status.as_str(), "C111520222909161");

        // Verify `'@'` (`0x40`) at byte [100] right after the 16B serial is not included.
        assert_eq!(status.as_str().len(), 16);

        // Non-`1-sq` tail yields `Present`.
        let mut non_qs1 = [0xAAu8; 104];
        non_qs1[80..84].copy_from_slice(b"ABCD");
        assert_eq!(
            parse_factory_cert_status(Some(&non_qs1)),
            FactoryCertStatus::Present
        );
        assert_eq!(
            parse_factory_cert_status(Some(&non_qs1)).as_str(),
            "Present"
        );

        // `1-sq` without printable ASCII S/N yields `Present`.
        non_qs1[80..84].copy_from_slice(b"1-sq");
        non_qs1[84..100].fill(0xFF);
        assert_eq!(
            parse_factory_cert_status(Some(&non_qs1)),
            FactoryCertStatus::Present
        );

        // Erased block yields `Erased` -> `"None"`.
        assert_eq!(parse_factory_cert_status(None), FactoryCertStatus::Erased);
        assert_eq!(parse_factory_cert_status(None).as_str(), "None");

        // Shifted / uppercase / colon-delimited `QS-1` tags are extracted cleanly.
        let mut shifted = [0x00u8; 104];
        shifted[72..78].copy_from_slice(b"QS-1: ");
        shifted[78..88].copy_from_slice(b"K1-SN98765");
        assert_eq!(
            parse_factory_cert_status(Some(&shifted)).as_str(),
            "K1-SN98765"
        );

        // Un-tagged 8+ char ASCII serial at [80..] is also extracted.
        let mut untagged = [0x00u8; 104];
        untagged[80..92].copy_from_slice(b"BATCH2026A12");
        assert_eq!(
            parse_factory_cert_status(Some(&untagged)).as_str(),
            "BATCH2026A12"
        );
    }

    #[test]
    fn fuzz_qs1_extractor_matches_byte_oracle() {
        fn oracle_find_tag_end(bytes: &[u8]) -> Option<usize> {
            let mut i = 0;
            while i + 4 <= bytes.len() {
                let w = &bytes[i..i + 4];
                let m1 =
                    w[0] == b'1' && w[1] == b'-' && (w[2] | 0x20) == b's' && (w[3] | 0x20) == b'q';
                let m2 =
                    (w[0] | 0x20) == b'q' && (w[1] | 0x20) == b's' && w[2] == b'-' && w[3] == b'1';
                if m1 || m2 {
                    return Some(i + 4);
                }
                i += 1;
            }
            None
        }

        let mut rng = 0x5A17_BEEFu32;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };

        let tags: [&[u8; 4]; 8] = [
            b"1-sq", b"1-SQ", b"1-Sq", b"1-sQ", b"qs-1", b"QS-1", b"Qs-1", b"qS-1",
        ];
        let mut buf = [0u8; 128];

        for iter in 0..1000usize {
            let len = (next() as usize) % 129;
            for b in &mut buf[..len] {
                *b = next() as u8;
            }
            if (iter & 1) == 0 && len >= 8 {
                let pos = (next() as usize) % (len - 4);
                buf[pos..pos + 4].copy_from_slice(tags[iter & 7]);
            }
            let slice = &buf[..len];
            assert_eq!(find_qs1_tag_end(slice), oracle_find_tag_end(slice));
            if let Some(sn) = extract_qs1_slice(slice) {
                assert!(sn.len() <= 16);
                assert!(
                    sn.iter().all(
                        |b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/')
                    )
                );
            }
        }
    }
}
