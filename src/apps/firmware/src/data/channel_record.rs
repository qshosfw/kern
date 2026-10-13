//! 64-Byte Power-of-Two Aligned Modular Channel Record Format.
//!
//! Provides zero-heap, constant-stride ($O(1)$ random seek via `index << 6`) storage
//! for memory channels. Aligned to 64 bytes so exactly 64 records fit in a 4096-byte
//! flash sector without ever straddling sector boundaries.
//!
//! Features an embedded 16-byte declarative Tagged Extension block (`user_tag`) allowing
//! applications (Hermes, APRS, Scanners, Power Profilers) to store declarative,
//! channel-specific settings without schema migrations.

/// FourCC tag identifying the declarative schema of the 16-byte channel extension block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionTag(pub u32);

impl ExtensionTag {
    pub const NONE: Self = Self(0);
    pub const HERMES: Self = Self(u32::from_be_bytes(*b"HRMS"));
    pub const APRS: Self = Self(u32::from_be_bytes(*b"APRS"));
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

/// 16-byte declarative extension container embedded in `ChannelRecord.user_tag`.
///
/// Layout:
/// - `tag: u32` (4 bytes FourCC schema identifier)
/// - `payload: [u8; 12]` (12 bytes of typed or raw application payload)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct ChannelExtension {
    pub tag: u32,
    pub payload: [u8; 12],
}

impl ChannelExtension {
    pub const fn empty() -> Self {
        Self {
            tag: 0,
            payload: [0; 12],
        }
    }

    pub fn new(tag: ExtensionTag, payload: [u8; 12]) -> Self {
        Self {
            tag: tag.0,
            payload,
        }
    }

    #[inline(always)]
    pub fn tag(&self) -> ExtensionTag {
        ExtensionTag(self.tag)
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 16] {
        unsafe { &*(self as *const Self as *const [u8; 16]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 16]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

/// Declarative extension payload for Hermes Mesh Channels (`b"HRMS"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct HermesChannelExt {
    pub subnet_id: u16,
    pub key_slot: u8,
    pub node_role: u8,
    pub tx_power_dbm: i8,
    pub topic_hash: u32,
    pub reserved: [u8; 3],
}

const _: () = assert!(core::mem::size_of::<HermesChannelExt>() == 12);

/// Declarative extension payload for APRS Packet Channels (`b"APRS"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct AprsChannelExt {
    pub symbol: u8,
    pub table: u8,
    pub ssid: u8,
    pub path: [u8; 7],
    pub beacon_interval_s: u16,
}

const _: () = assert!(core::mem::size_of::<AprsChannelExt>() == 12);

/// Declarative extension payload for Channel Scanner Tuning (`b"SCAN"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct ScanChannelExt {
    pub dwell_ms: u16,
    pub priority_weight: u8,
    pub resume_delay_ms: u16,
    pub scan_group_mask: u16,
    pub reserved: [u8; 5],
}

const _: () = assert!(core::mem::size_of::<ScanChannelExt>() == 12);

/// Declarative extension payload for Calibrated PA Power Profiling (`b"POWR"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct PowerChannelExt {
    pub pa_bias_dac: u16,
    pub target_power_mw: u16,
    pub swr_limit_x10: u16,
    pub max_temp_c: u8,
    pub reserved: [u8; 5],
}

const _: () = assert!(core::mem::size_of::<PowerChannelExt>() == 12);

/// 64-byte packed, power-of-two aligned channel record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct ChannelRecord {
    pub rx_frequency_hz: u32,
    pub tx_frequency_hz: u32,
    pub step_khz_x100: u16,
    pub bandwidth: u8,
    pub modulation: u8,
    pub power_level: u8,
    pub squelch_level: u8,
    pub rx_subaudible: u16,
    pub tx_subaudible: u16,
    pub bank_mask: u16,
    pub flags: u16,
    pub scrambler_code: u8,
    pub signaling_type: u8,
    pub signaling_id: u16,
    pub roger_beep: u8,
    pub css_tail_mode: u8,
    pub name: [u8; 16],
    pub user_tag: [u8; 16],
    pub reserved: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<ChannelRecord>() == 64);

impl ChannelRecord {
    pub const SIZE: usize = 64;
    pub const STRIDE_SHIFT: u32 = 6; // 1 << 6 == 64 bytes

    // Channel Option Bitflags
    pub const FLAG_BUSY_LOCKOUT: u16 = 1 << 0;
    pub const FLAG_SCAN_SKIP: u16 = 1 << 1;
    pub const FLAG_REVERSE: u16 = 1 << 2;
    pub const FLAG_PRIORITY_1: u16 = 1 << 3;
    pub const FLAG_PRIORITY_2: u16 = 1 << 4;
    pub const FLAG_TX_INHIBIT: u16 = 1 << 5;
    pub const FLAG_COMPANDER: u16 = 1 << 6;
    pub const FLAG_PTT_ID_START: u16 = 1 << 7;
    pub const FLAG_PTT_ID_END: u16 = 1 << 8;
    pub const FLAG_TALKAROUND: u16 = 1 << 9;
    pub const FLAG_FAVORITE: u16 = 1 << 10;

    // Signaling Types
    pub const SIG_NONE: u8 = 0;
    pub const SIG_SELCALL: u8 = 1;
    pub const SIG_DTMF: u8 = 2;
    pub const SIG_MDC1200: u8 = 3;
    pub const SIG_M17: u8 = 4;
    pub const SIG_HERMES: u8 = 5;

    pub const fn empty() -> Self {
        Self {
            rx_frequency_hz: 0,
            tx_frequency_hz: 0,
            step_khz_x100: 1250, // 12.5 kHz
            bandwidth: 0,        // Narrow
            modulation: 0,       // FM
            power_level: 2,      // High
            squelch_level: 4,
            rx_subaudible: 0,
            tx_subaudible: 0,
            bank_mask: 0x0001, // Bank 1
            flags: 0,
            scrambler_code: 0,
            signaling_type: 0,
            signaling_id: 0,
            roger_beep: 0,
            css_tail_mode: 0,
            name: [0; 16],
            user_tag: [0; 16],
            reserved: [0; 4],
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
        let len = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..len]).unwrap_or("")
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 16];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(15);
        self.name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    pub fn user_tag_as_str(&self) -> &str {
        let len = self
            .user_tag
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.user_tag.len());
        core::str::from_utf8(&self.user_tag[..len]).unwrap_or("")
    }

    pub fn set_user_tag(&mut self, s: &str) {
        self.user_tag = [0; 16];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(15);
        self.user_tag[..copy_len].copy_from_slice(&bytes[..copy_len]);
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

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 64] {
        unsafe { &*(self as *const Self as *const [u8; 64]) }
    }

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 64] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 64]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 64]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}
