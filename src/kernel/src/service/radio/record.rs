//! 64-Byte Power-of-Two Aligned Canonical Channel Record (`CHAN`).
//!
//! Single unified channel schema across RAM (`RadioCoordinator` VFO slots),
//! KernFS (`/user/channels.ch`, `/user/vfo_*.vfo`), KRX syscalls, and RigWire v2.
//! Zero conversion glue, $O(1)$ seeking (`16 + (index << 6)`), zero unaligned access.

use super::channel::{RfPowerLevel, StepSetting};
use super::modulation::{Bandwidth, CompanderMode, ModulationMode, ScramblerMode};
use super::tone::{CssTailMode, SubaudibleTone};
use utils::units::{Frequency, Hertz};

/// FourCC tag identifying the declarative schema of the 8-byte channel extension block.
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

fn read_fixed_str(buf: &[u8]) -> &str {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    utils::format::ascii_str(&buf[..len]).unwrap_or("")
}

fn write_fixed_str(dst: &mut [u8], s: &str, max_len: usize) {
    dst.fill(0);
    let bytes = s.as_bytes();
    let copy_len = bytes.len().min(max_len).min(dst.len());
    dst[..copy_len].copy_from_slice(&bytes[..copy_len]);
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

    pub const FLAG_BUSY_LOCKOUT: u16 = 1 << 0;
    pub const FLAG_SCAN_SKIP: u16 = 1 << 1;
    pub const FLAG_REVERSE: u16 = 1 << 2;
    pub const FLAG_PRIORITY_1: u16 = 1 << 3;
    pub const FLAG_PRIORITY_2: u16 = 1 << 4;
    pub const FLAG_TX_INHIBIT: u16 = 1 << 5;
    pub const FLAG_TONE_BCLO: u16 = 1 << 6;
    pub const FLAG_PTT_ID_START: u16 = 1 << 7;
    pub const FLAG_PTT_ID_END: u16 = 1 << 8;
    pub const FLAG_TALKAROUND: u16 = 1 << 9;
    pub const FLAG_FAVORITE: u16 = 1 << 10;
    pub const FLAG_CALL_MATCH_MUTE: u16 = 1 << 11;
    pub const FLAG_FREQ_LOCK: u16 = 1 << 12;
    pub const FLAG_WIDE_FILTER: u16 = 1 << 13;
    pub const FLAG_LNA_BOOST: u16 = 1 << 14;
    pub const FLAG_VOX_ENABLE: u16 = 1 << 15;

    pub const SUBAUDIBLE_DCS_FLAG: u16 = 0x8000;
    pub const SUBAUDIBLE_DCS_INV_FLAG: u16 = 0x4000;
    pub const SUBAUDIBLE_VALUE_MASK: u16 = 0x3FFF;

    pub const fn empty() -> Self {
        Self {
            rx_frequency_hz: 0,
            tx_frequency_hz: 0,
            step_hz_x10: 1250,   // 12.5 kHz
            bandwidth_50hz: 250, // 12.5 kHz narrow
            modulation: 0,       // FM
            squelch_level: 4,
            rx_subaudible: 0,
            tx_subaudible: 0,
            zone_mask: 0x0001,
            flags: 0,
            power_mw: 5000, // High
            proto_ref: 0,
            uid: 0,
            proto: 0,
            scram_mode: 0,
            scram_param: 0,
            scram_hop: 0,
            compand: 0,
            mic_gain: 0,
            ste: CssTailMode::Tone55Hz as u8,
            roger: 0,
            tx_limit: 0,
            audio_flags: 0,
            name: [0; 16],
            user_tag: [0; 8],
            record_crc: 0,
        }
    }

    pub const fn new(rx_freq_hz: u32) -> Self {
        let mut r = Self::empty();
        r.rx_frequency_hz = rx_freq_hz;
        r
    }

    pub fn new_simplex(freq_hz: u32, name: &str) -> Self {
        let mut rec = Self::new(freq_hz);
        rec.set_name(name);
        rec
    }

    pub fn new_repeater(rx_hz: u32, tx_hz: u32, subaudible_tx: u16, name: &str) -> Self {
        let mut rec = Self::new(rx_hz);
        rec.tx_frequency_hz = tx_hz;
        rec.tx_subaudible = subaudible_tx;
        rec.set_name(name);
        rec
    }

    pub const fn encode_subaudible(tone: SubaudibleTone) -> u16 {
        match tone {
            SubaudibleTone::None => 0,
            SubaudibleTone::Ctcss(deci_hz) => deci_hz & Self::SUBAUDIBLE_VALUE_MASK,
            SubaudibleTone::DcsNormal(code) => Self::SUBAUDIBLE_DCS_FLAG | (code & 0x03FF),
            SubaudibleTone::DcsInverted(code) => {
                Self::SUBAUDIBLE_DCS_FLAG | Self::SUBAUDIBLE_DCS_INV_FLAG | (code & 0x03FF)
            }
        }
    }

    pub const fn decode_subaudible(raw: u16) -> SubaudibleTone {
        if raw == 0 {
            SubaudibleTone::None
        } else if (raw & Self::SUBAUDIBLE_DCS_FLAG) != 0 {
            let code = raw & 0x03FF;
            if (raw & Self::SUBAUDIBLE_DCS_INV_FLAG) != 0 {
                SubaudibleTone::DcsInverted(code)
            } else {
                SubaudibleTone::DcsNormal(code)
            }
        } else {
            SubaudibleTone::Ctcss(raw & Self::SUBAUDIBLE_VALUE_MASK)
        }
    }

    #[inline(always)]
    pub const fn rx_subaudible_tone(&self) -> SubaudibleTone {
        Self::decode_subaudible(self.rx_subaudible)
    }

    #[inline(always)]
    pub const fn tx_subaudible_tone(&self) -> SubaudibleTone {
        Self::decode_subaudible(self.tx_subaudible)
    }

    #[inline(always)]
    pub fn set_rx_subaudible_tone(&mut self, tone: SubaudibleTone) {
        self.rx_subaudible = Self::encode_subaudible(tone);
    }

    #[inline(always)]
    pub fn set_tx_subaudible_tone(&mut self, tone: SubaudibleTone) {
        self.tx_subaudible = Self::encode_subaudible(tone);
    }

    #[inline(always)]
    pub const fn has_flag(&self, bit: u16) -> bool {
        (self.flags & bit) != 0
    }

    #[inline(always)]
    pub fn set_flag(&mut self, bit: u16, on: bool) {
        if on {
            self.flags |= bit;
        } else {
            self.flags &= !bit;
        }
    }

    #[inline(always)]
    pub const fn reverse(&self) -> bool {
        self.has_flag(Self::FLAG_REVERSE)
    }
    #[inline(always)]
    pub fn set_reverse(&mut self, on: bool) {
        self.set_flag(Self::FLAG_REVERSE, on);
    }
    #[inline(always)]
    pub const fn busy_lock(&self) -> bool {
        self.has_flag(Self::FLAG_BUSY_LOCKOUT)
    }
    #[inline(always)]
    pub fn set_busy_lock(&mut self, on: bool) {
        self.set_flag(Self::FLAG_BUSY_LOCKOUT, on);
    }
    #[inline(always)]
    pub const fn talkaround(&self) -> bool {
        self.has_flag(Self::FLAG_TALKAROUND)
    }
    #[inline(always)]
    pub fn set_talkaround(&mut self, on: bool) {
        self.set_flag(Self::FLAG_TALKAROUND, on);
    }
    #[inline(always)]
    pub const fn tx_inhibit(&self) -> bool {
        self.has_flag(Self::FLAG_TX_INHIBIT)
    }
    #[inline(always)]
    pub fn set_tx_inhibit(&mut self, on: bool) {
        self.set_flag(Self::FLAG_TX_INHIBIT, on);
    }
    #[inline(always)]
    pub const fn tone_bclo(&self) -> bool {
        self.has_flag(Self::FLAG_TONE_BCLO)
    }
    #[inline(always)]
    pub const fn freq_lock(&self) -> bool {
        self.has_flag(Self::FLAG_FREQ_LOCK)
    }
    #[inline(always)]
    pub const fn call_match_mute(&self) -> bool {
        self.has_flag(Self::FLAG_CALL_MATCH_MUTE)
    }

    #[inline(always)]
    pub const fn step_setting(&self) -> StepSetting {
        StepSetting::from_hz_x10(self.step_hz_x10)
    }

    #[inline(always)]
    pub fn set_step_setting(&mut self, step: StepSetting) {
        self.step_hz_x10 = step.in_hz_x10();
    }

    #[inline(always)]
    pub const fn bandwidth(&self) -> Bandwidth {
        if self.bandwidth_50hz >= 400 || self.has_flag(Self::FLAG_WIDE_FILTER) {
            Bandwidth::Wide25k
        } else if self.bandwidth_50hz > 0 && self.bandwidth_50hz <= 150 {
            Bandwidth::Narrow6k25
        } else {
            Bandwidth::Narrow12k5
        }
    }

    #[inline(always)]
    pub fn set_bandwidth(&mut self, bw: Bandwidth) {
        const BW_50HZ: [u16; 3] = [500, 250, 125];
        self.bandwidth_50hz = BW_50HZ[bw as usize];
        self.set_flag(Self::FLAG_WIDE_FILTER, bw == Bandwidth::Wide25k);
    }

    #[inline(always)]
    pub const fn modulation_mode(&self) -> ModulationMode {
        ModulationMode::from_u8(self.modulation)
    }

    #[inline(always)]
    pub fn set_modulation_mode(&mut self, m: ModulationMode) {
        self.modulation = m.to_u8();
    }

    #[inline(always)]
    pub const fn power_level(&self) -> RfPowerLevel {
        if self.power_mw == 0 || self.power_mw >= 4000 {
            RfPowerLevel::High
        } else if self.power_mw >= 1500 {
            RfPowerLevel::Medium
        } else {
            RfPowerLevel::Low
        }
    }

    #[inline(always)]
    pub fn set_power_level(&mut self, p: RfPowerLevel) {
        const PWR_MW: [u16; 3] = [500, 2000, 5000];
        self.power_mw = PWR_MW[p as usize];
    }

    #[inline(always)]
    pub const fn compander(&self) -> CompanderMode {
        CompanderMode::from_u8(self.compand)
    }

    #[inline(always)]
    pub const fn scrambler(&self) -> ScramblerMode {
        if self.scram_mode == 0 {
            ScramblerMode::Off
        } else {
            let p = if self.scram_param > self.scram_mode {
                self.scram_param
            } else {
                self.scram_mode
            };
            ScramblerMode::InvertFrequency(p)
        }
    }

    #[inline(always)]
    pub fn set_scrambler(&mut self, s: ScramblerMode) {
        match s {
            ScramblerMode::Off => {
                self.scram_mode = 0;
                self.scram_param = 0;
            }
            ScramblerMode::InvertFrequency(code) => {
                self.scram_mode = 1;
                self.scram_param = code;
            }
        }
    }

    #[inline(always)]
    pub const fn ste_mode(&self) -> CssTailMode {
        CssTailMode::from_u8(self.ste)
    }

    #[inline(always)]
    const fn nominal_tx_hz(&self) -> u32 {
        if self.tx_frequency_hz != 0 {
            self.tx_frequency_hz
        } else {
            self.rx_frequency_hz
        }
    }

    #[inline(always)]
    pub const fn effective_rx_freq(&self) -> Frequency<Hertz> {
        if self.reverse() && !self.talkaround() {
            Frequency::new(self.nominal_tx_hz())
        } else {
            Frequency::new(self.rx_frequency_hz)
        }
    }

    #[inline(always)]
    pub const fn effective_tx_freq(&self) -> Frequency<Hertz> {
        if self.talkaround() || self.reverse() {
            Frequency::new(self.rx_frequency_hz)
        } else {
            Frequency::new(self.nominal_tx_hz())
        }
    }

    #[inline(always)]
    pub const fn effective_rx_subaudible(&self) -> SubaudibleTone {
        if self.reverse() && !self.talkaround() {
            self.tx_subaudible_tone()
        } else {
            self.rx_subaudible_tone()
        }
    }

    #[inline(always)]
    pub const fn effective_tx_subaudible(&self) -> SubaudibleTone {
        if self.reverse() && !self.talkaround() {
            self.rx_subaudible_tone()
        } else {
            self.tx_subaudible_tone()
        }
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

    #[inline(always)]
    pub fn is_programmed(&self) -> bool {
        self.rx_frequency_hz >= 18_000_000 && self.rx_frequency_hz <= 1_300_000_000
    }

    pub fn name_as_str(&self) -> &str {
        read_fixed_str(&self.name)
    }

    pub fn set_name(&mut self, s: &str) {
        write_fixed_str(&mut self.name, s, 15);
    }

    pub fn user_tag_as_str(&self) -> &str {
        read_fixed_str(&self.user_tag)
    }

    pub fn set_user_tag(&mut self, s: &str) {
        write_fixed_str(&mut self.user_tag, s, 7);
    }

    #[inline(always)]
    pub fn extension(&self) -> ChannelExtension {
        ChannelExtension::from_bytes(&self.user_tag)
    }

    #[inline(always)]
    pub fn set_extension(&mut self, ext: &ChannelExtension) {
        self.user_tag.copy_from_slice(ext.as_bytes());
    }

    pub fn compute_crc(&self) -> u16 {
        let bytes = self.as_bytes();
        crypto::crc::CrcEngine::crc16_ccitt(&bytes[..0x3E])
    }

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

    #[test]
    fn channel_record_subaudible_zone_and_reverse_roundtrip() {
        let mut ch = ChannelRecord::new_repeater(145_700_000, 145_100_000, 0, "R4 LOCAL");
        ch.set_rx_subaudible_tone(SubaudibleTone::Ctcss(885));
        ch.set_tx_subaudible_tone(SubaudibleTone::DcsInverted(0o023));
        ch.set_zone(3, true);
        ch.record_crc = ch.compute_crc();

        let dec = ChannelRecord::from_bytes(ch.as_bytes());
        assert_eq!(dec, ch);
        assert!(dec.is_crc_valid());
        assert_eq!(dec.effective_rx_freq().raw(), 145_700_000);
        assert_eq!(dec.effective_tx_freq().raw(), 145_100_000);

        let mut rev = dec;
        rev.set_reverse(true);
        assert_eq!(rev.effective_rx_freq().raw(), 145_100_000);
        assert_eq!(rev.effective_tx_freq().raw(), 145_700_000);
        assert_eq!(
            rev.effective_rx_subaudible(),
            SubaudibleTone::DcsInverted(0o023)
        );
        assert_eq!(rev.effective_tx_subaudible(), SubaudibleTone::Ctcss(885));
    }
}
