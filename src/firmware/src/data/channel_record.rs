//! 64-Byte Power-of-Two Aligned Modular Channel Record Format.
//!
//! Provides zero-heap, constant-stride ($O(1)$ random seek via `index << 6`) storage
//! for memory channels. Aligned to 64 bytes so exactly 64 records fit in a 4096-byte
//! flash sector without ever straddling sector boundaries.
//!
//! Features an embedded 16-byte declarative Tagged Extension block (`user_tag`) allowing
//! applications (Mesh, APRS, Scanners, Power Profilers) to store declarative,
//! channel-specific settings without schema migrations.

/// FourCC tag identifying the declarative schema of the 16-byte channel extension block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionTag(pub u32);

impl ExtensionTag {
    pub const NONE: Self = Self(0);
    pub const MESH: Self = Self(u32::from_be_bytes(*b"MESH"));
    pub const PACKET: Self = Self(u32::from_be_bytes(*b"PKT "));
    pub const SCAN: Self = Self(u32::from_be_bytes(*b"SCAN"));
    pub const POWER: Self = Self(u32::from_be_bytes(*b"POWR"));
    pub const TEXT: Self = Self(u32::from_be_bytes(*b"TEXT"));

    #[inline(always)]
    pub const fn from_tag(tag: &[u8; 4]) -> Self {
        Self(u32::from_be_bytes(*tag))
    }

    #[inline(always)]
    pub const fn to_tag(&self) -> [u8; 4] {
        self.0.to_be_bytes()
    }
}

/// 8-byte declarative extension container embedded in `ChannelRecord.user_tag`.
///
/// Layout (Keel Section 3.2.6):
/// - `tag: u32` (4 bytes FourCC schema identifier)
/// - `version: u8` (app schema version)
/// - `payload: [u8; 3]` (3 bytes application payload)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ChannelExtension {
    pub tag: u32,
    pub version: u8,
    pub payload: [u8; 3],
}

const _: () = {
    assert!(core::mem::size_of::<ChannelExtension>() == 8);
    assert!(core::mem::align_of::<ChannelExtension>() == 4);
};

impl ChannelExtension {
    pub const fn empty() -> Self {
        Self {
            tag: 0,
            version: 0,
            payload: [0; 3],
        }
    }

    pub fn new(tag: ExtensionTag, version: u8, payload: [u8; 3]) -> Self {
        Self {
            tag: tag.0,
            version,
            payload,
        }
    }

    #[inline(always)]
    pub fn tag(&self) -> ExtensionTag {
        ExtensionTag(self.tag)
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 8] {
        unsafe { &*(self as *const Self as *const [u8; 8]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 8]) -> Self {
        Self {
            tag: u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            version: bytes[4],
            payload: [bytes[5], bytes[6], bytes[7]],
        }
    }
}

/// 64-byte naturally-aligned, power-of-two channel record (zero unaligned accesses on Cortex-M0+).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ChannelRecord {
    pub rx_frequency_hz: u32,
    pub tx_frequency_hz: u32,
    pub step_hz_x10: u16,
    pub bandwidth_50hz: u16,
    pub modulation: u8,
    pub squelch_level: u8,
    pub rx_subaudible: u16,
    pub tx_subaudible: u16,
    pub zone_mask: u16,
    pub flags: u16,
    pub power_mw: u16,
    pub proto_ref: u16,
    pub uid: u16,
    pub proto: u8,

    // Audio Block (9 bytes physical parameters matching BK4829 hardware registers)
    pub scram_mode: u8,
    pub scram_param: u8,
    pub scram_hop: u8,
    pub compand: u8,
    pub mic_gain: u8,
    pub ste: u8,
    pub roger: u8,
    pub tx_limit: u8,
    pub audio_flags: u8,

    pub name: [u8; 16],
    pub user_tag: [u8; 8],
    pub record_crc: u16,
}

const _: () = {
    assert!(core::mem::size_of::<ChannelRecord>() == 64);
    assert!(core::mem::align_of::<ChannelRecord>() == 4);
    assert!(core::mem::offset_of!(ChannelRecord, record_crc) == 0x3E);
};

impl ChannelRecord {
    pub const SIZE: usize = 64;
    pub const STRIDE_SHIFT: u32 = 6; // 1 << 6 == 64 bytes

    // Channel Option Bitflags (Keel Table 3.3 & Cross-Firmware Parity)
    pub const FLAG_BUSY_LOCKOUT: u16 = 1 << 0;
    pub const FLAG_SCAN_SKIP: u16 = 1 << 1;
    pub const FLAG_REVERSE: u16 = 1 << 2;
    pub const FLAG_PRIORITY_1: u16 = 1 << 3;
    pub const FLAG_PRIORITY_2: u16 = 1 << 4;
    pub const FLAG_TX_INHIBIT: u16 = 1 << 5;
    /// Busy Channel Lockout only blocks when carrier is present with mismatched subaudible tone.
    pub const FLAG_TONE_BCLO: u16 = 1 << 6;
    pub const FLAG_PTT_ID_START: u16 = 1 << 7;
    pub const FLAG_PTT_ID_END: u16 = 1 << 8;
    pub const FLAG_TALKAROUND: u16 = 1 << 9;
    pub const FLAG_FAVORITE: u16 = 1 << 10;
    /// Selective squelch: remain muted until incoming signaling matches `proto_ref` contact/group/topic.
    pub const FLAG_CALL_MATCH_MUTE: u16 = 1 << 11;
    /// Lock VFO frequency against accidental encoder tuning.
    pub const FLAG_FREQ_LOCK: u16 = 1 << 12;
    pub const FLAG_WIDE_FILTER: u16 = 1 << 13;
    /// Enable front-end LNA max gain boost for weak-signal reception.
    pub const FLAG_LNA_BOOST: u16 = 1 << 14;
    /// Enable hands-free VOX transmission on this channel.
    pub const FLAG_VOX_ENABLE: u16 = 1 << 15;

    // Canonical 16-bit Subaudible Tone Tag Masks
    pub const SUBAUDIBLE_DCS_FLAG: u16 = 0x8000;
    pub const SUBAUDIBLE_DCS_INV_FLAG: u16 = 0x4000;
    pub const SUBAUDIBLE_VALUE_MASK: u16 = 0x3FFF;

    /// Encode a `SubaudibleTone` into the compact 16-bit `rx_subaudible` / `tx_subaudible` word.
    #[inline(always)]
    pub const fn encode_subaudible(tone: kernel::service::radio::SubaudibleTone) -> u16 {
        match tone {
            kernel::service::radio::SubaudibleTone::None => 0,
            kernel::service::radio::SubaudibleTone::Ctcss(deci_hz) => {
                deci_hz & Self::SUBAUDIBLE_VALUE_MASK
            }
            kernel::service::radio::SubaudibleTone::DcsNormal(code) => {
                Self::SUBAUDIBLE_DCS_FLAG | (code & 0x03FF)
            }
            kernel::service::radio::SubaudibleTone::DcsInverted(code) => {
                Self::SUBAUDIBLE_DCS_FLAG | Self::SUBAUDIBLE_DCS_INV_FLAG | (code & 0x03FF)
            }
        }
    }

    /// Decode a 16-bit subaudible word back into a `SubaudibleTone`.
    #[inline(always)]
    pub const fn decode_subaudible(raw: u16) -> kernel::service::radio::SubaudibleTone {
        if raw == 0 {
            kernel::service::radio::SubaudibleTone::None
        } else if (raw & Self::SUBAUDIBLE_DCS_FLAG) != 0 {
            let code = raw & 0x03FF;
            if (raw & Self::SUBAUDIBLE_DCS_INV_FLAG) != 0 {
                kernel::service::radio::SubaudibleTone::DcsInverted(code)
            } else {
                kernel::service::radio::SubaudibleTone::DcsNormal(code)
            }
        } else {
            kernel::service::radio::SubaudibleTone::Ctcss(raw & Self::SUBAUDIBLE_VALUE_MASK)
        }
    }

    #[inline(always)]
    pub const fn rx_subaudible_tone(&self) -> kernel::service::radio::SubaudibleTone {
        Self::decode_subaudible(self.rx_subaudible)
    }

    #[inline(always)]
    pub const fn tx_subaudible_tone(&self) -> kernel::service::radio::SubaudibleTone {
        Self::decode_subaudible(self.tx_subaudible)
    }

    #[inline(always)]
    pub fn set_rx_subaudible_tone(&mut self, tone: kernel::service::radio::SubaudibleTone) {
        self.rx_subaudible = Self::encode_subaudible(tone);
    }

    #[inline(always)]
    pub fn set_tx_subaudible_tone(&mut self, tone: kernel::service::radio::SubaudibleTone) {
        self.tx_subaudible = Self::encode_subaudible(tone);
    }

    #[inline(always)]
    pub const fn in_zone(&self, zone_index: u8) -> bool {
        zone_index < 16 && (self.zone_mask & (1u16 << zone_index)) != 0
    }

    #[inline(always)]
    pub fn set_zone(&mut self, zone_index: u8, member: bool) {
        if zone_index < 16 {
            if member {
                self.zone_mask |= 1u16 << zone_index;
            } else {
                self.zone_mask &= !(1u16 << zone_index);
            }
        }
    }

    pub const fn empty() -> Self {
        Self {
            rx_frequency_hz: 0,
            tx_frequency_hz: 0,
            step_hz_x10: 1250, // 12.5 kHz
            bandwidth_50hz: 0, // Profile default (or 250 for 12.5k)
            modulation: 0,     // FM
            squelch_level: 4,
            rx_subaudible: 0,
            tx_subaudible: 0,
            zone_mask: 0x0001, // Zone 1
            flags: 0,
            power_mw: 0, // Profile default (high)
            proto: 0,    // Analog
            proto_ref: 0,
            uid: 0, // Unassigned
            scram_mode: 0,
            scram_param: 0,
            scram_hop: 0,
            compand: 0,
            mic_gain: 0,
            ste: 0,
            roger: 0,
            tx_limit: 0,
            audio_flags: 0,
            name: [0; 16],
            user_tag: [0; 8],
            record_crc: 0,
        }
    }

    pub fn new_simplex(freq_hz: u32, name: &str) -> Self {
        let mut rec = Self::empty();
        rec.rx_frequency_hz = freq_hz;
        rec.tx_frequency_hz = 0; // 0 = simplex
        rec.set_name(name);
        rec
    }

    pub fn new_repeater(rx_hz: u32, tx_hz: u32, subaudible_tx: u16, name: &str) -> Self {
        let mut rec = Self::empty();
        rec.rx_frequency_hz = rx_hz;
        rec.tx_frequency_hz = tx_hz;
        rec.tx_subaudible = subaudible_tx;
        rec.set_name(name);
        rec
    }

    #[inline(always)]
    pub fn is_programmed(&self) -> bool {
        self.rx_frequency_hz >= 18_000_000 && self.rx_frequency_hz <= 1_300_000_000
    }

    #[inline(always)]
    pub fn effective_tx_freq(&self) -> u32 {
        if self.tx_frequency_hz != 0 {
            self.tx_frequency_hz
        } else {
            self.rx_frequency_hz
        }
    }

    pub fn name_as_str(&self) -> &str {
        super::read_fixed_str(&self.name)
    }

    pub fn set_name(&mut self, s: &str) {
        super::write_fixed_str(&mut self.name, s, 15);
    }

    pub fn user_tag_as_str(&self) -> &str {
        super::read_fixed_str(&self.user_tag)
    }

    pub fn set_user_tag(&mut self, s: &str) {
        super::write_fixed_str(&mut self.user_tag, s, 7);
    }

    /// Read the declarative tagged extension block embedded in `user_tag`.
    #[inline(always)]
    pub fn extension(&self) -> ChannelExtension {
        ChannelExtension::from_bytes(&self.user_tag)
    }

    /// Write a declarative tagged extension block into `user_tag`.
    #[inline(always)]
    pub fn set_extension(&mut self, ext: &ChannelExtension) {
        self.user_tag.copy_from_slice(ext.as_bytes());
    }

    /// Compute CRC-16/CCITT-FALSE over bytes [0..0x3E) as required by Keel KEEL-C1-003 / KEEL-C1-004.
    pub fn compute_crc(&self) -> u16 {
        let bytes = self.as_bytes();
        crypto::crc::CrcEngine::crc16_ccitt(&bytes[..0x3E])
    }

    /// Verify whether `record_crc` matches calculated checksum.
    pub fn is_crc_valid(&self) -> bool {
        self.record_crc == self.compute_crc()
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 64] {
        unsafe { &*(self as *const Self as *const [u8; 64]) }
    }

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 64] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 64]) }
    }

    #[inline(never)]
    pub fn from_bytes(bytes: &[u8; 64]) -> Self {
        let mut out = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), out.as_mut_ptr() as *mut u8, 64);
            out.assume_init()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::service::radio::SubaudibleTone;

    #[test]
    fn channel_record_subaudible_and_zone_roundtrip() {
        let mut ch = ChannelRecord::new_repeater(145_700_000, 145_100_000, 0, "R4 LOCAL");
        ch.set_rx_subaudible_tone(SubaudibleTone::Ctcss(885));
        ch.set_tx_subaudible_tone(SubaudibleTone::DcsInverted(0o023));
        ch.set_zone(3, true);
        ch.record_crc = ch.compute_crc();

        let dec = ChannelRecord::from_bytes(ch.as_bytes());
        assert_eq!(dec, ch);
        assert!(dec.is_crc_valid());
        assert_eq!(dec.rx_subaudible_tone(), SubaudibleTone::Ctcss(885));
        assert_eq!(dec.tx_subaudible_tone(), SubaudibleTone::DcsInverted(0o023));
        assert!(dec.in_zone(0));
        assert!(dec.in_zone(3));
        assert!(!dec.in_zone(2));
    }
}
