//! 32-Byte Power-of-Two Protocol-Agnostic Universal Contact, Group & Topic Record Format.
//!
//! Provides zero-heap, constant-stride ($O(1)$ random seek via `16 + (index << 5)`) storage
//! for addressable endpoints, multicast groups, pub/sub topics, relays, and services.
//! Aligned to 32 bytes ($2^5$) so exactly 128 records fit in a 4096-byte flash sector
//! without ever straddling flash erase sector boundaries.
//!
//! Design Philosophy:
//! - **Protocol-Agnostic**: Decouples *what* is being addressed (`ContactKind`: Endpoint, Group,
//!   Topic, Relay, Service) and *how* its 64-bit identity is encoded (`AddressScheme`: Numeric,
//!   Ascii, Mac48, Eui64, Base40, TopicHash, ToneSeq) from any specific physical/link protocol.
//! - **Hierarchical Groups & Topics**: Any record can reference a `parent_group` (1-based index
//!   or logical group ID), enabling nested pub/sub topic trees, fleet/talkgroup membership,
//!   and selective squelch filtering ("unmute if frame matches contact OR its subscribed group/topic").
//! - **64-Bit Universal Identity (`id: [u8; 8]`)**:
//!   - `AddressScheme::Numeric`: 32-bit/64-bit integer ID (radio IDs, talkgroups, unit numbers)
//!   - `AddressScheme::Ascii`: Up to 8 ASCII chars (callsigns with SSID `"IU2AAA-9"`, short tags `"#WX-EU"`)
//!   - `AddressScheme::Mac48`: 48-bit MAC/node address (`id[0..6]`) + 16-bit port/sub-ID (`id[6..8]`)
//!   - `AddressScheme::Eui64`: Full 64-bit hardware/network identifier
//!   - `AddressScheme::Base40`: 48-bit Base40 callsign (`id[0..6]`) + 16-bit stream/module (`id[6..8]`)
//!   - `AddressScheme::TopicHash`: 64-bit FNV-1a deterministic hash of arbitrary hierarchical topic URIs
//!     (e.g. `"mesh/eu/wx/alerts"` or `"aprs/sos"`) for $O(1)$ fixed-stride topic matching
//!   - `AddressScheme::ToneSeq`: Packed 4-bit nibble sequence encoding up to 16 DTMF / 5-Tone SelCall
//!     digits (`0..9, A..D, *, #`) directly in `id[0..8]`

/// Protocol-agnostic semantic entity kind of a contact record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ContactKind {
    /// Individual peer station, node, operator, or device endpoint.
    Endpoint = 0,
    /// Multicast talkgroup, net, crew, or fleet.
    Group = 1,
    /// Pub/Sub topic, bulletin stream, chat room, or telemetry channel.
    Topic = 2,
    /// Repeater, digipeater, mesh router, or cross-band gateway.
    Relay = 3,
    /// Automated service, beacon target, or local macro alias.
    Service = 4,
}

impl ContactKind {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Group,
            2 => Self::Topic,
            3 => Self::Relay,
            4 => Self::Service,
            _ => Self::Endpoint,
        }
    }
}

/// Protocol-agnostic encoding scheme for the 64-bit `id: [u8; 8]` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AddressScheme {
    /// Unassigned / label-only entry.
    None = 0,
    /// Big-endian 64-bit (or 32-bit namespace + 32-bit unit) integer identifier.
    Numeric = 1,
    /// Up to 8 ASCII characters (callsign+SSID, topic slug, short alias).
    Ascii = 2,
    /// 48-bit MAC / Node ID in `id[0..6]` + 16-bit big-endian port/endpoint in `id[6..8]`.
    Mac48 = 3,
    /// 64-bit EUI-64 / hardware network identifier.
    Eui64 = 4,
    /// 48-bit Base40 encoded callsign in `id[0..6]` + 16-bit module/sub-id in `id[6..8]`.
    Base40 = 5,
    /// 64-bit FNV-1a deterministic hash of an arbitrary hierarchical topic URI string.
    TopicHash = 6,
    /// Packed 4-bit nibbles encoding up to 16 DTMF / SelCall tone symbols (`0..=15`, `0xF` = end/pad).
    ToneSeq = 7,
}

impl AddressScheme {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Numeric,
            2 => Self::Ascii,
            3 => Self::Mac48,
            4 => Self::Eui64,
            5 => Self::Base40,
            6 => Self::TopicHash,
            7 => Self::ToneSeq,
            _ => Self::None,
        }
    }
}

/// Optional physical/transport layer class hint for routing and modulation selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportClass {
    /// Any active transport or channel default.
    Any = 0,
    /// Audio/subaudible tone signaling (DTMF, 5-Tone SelCall, CTCSS/DCS paging).
    ToneSignaling = 1,
    /// Direct baseband FSK / FFSK packet modem (1200..9600 bps).
    BasebandFsk = 2,
    /// Audio subcarrier AFSK / Bell 202 / SAME.
    AudioAfsk = 3,
    /// Digital voice / codec stream (M17, DMR, FreeDV).
    DigitalVoice = 4,
    /// Sub-Hz Fractional-N Vernier narrowband digital (FT8, WSPR, JS8, CW).
    VernierSubHz = 5,
}

/// Deterministic 64-bit FNV-1a hash for hierarchical topic paths and URI strings.
#[inline]
pub const fn fnv1a_hash64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(PRIME);
        i += 1;
    }
    hash
}

/// 32-byte naturally-aligned, power-of-two universal contact, group, and topic record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ContactRecord {
    /// 64-bit protocol-agnostic universal identifier (interpreted via `scheme`).
    pub id: [u8; 8],
    /// Parent group or topic identifier (0 = root/ungrouped).
    pub parent_group: u16,
    /// Behavioral, subscription, selective-squelch, and routing flags.
    pub flags: u16,
    /// Entity kind (`ContactKind`: Endpoint, Group, Topic, Relay, Service).
    pub kind: u8,
    /// Identity encoding scheme (`AddressScheme`).
    pub scheme: u8,
    /// Preferred physical/link transport class hint (`TransportClass` or app-defined `0x10..=0xFF`).
    pub transport: u8,
    /// Packed routing policy: bits `[7:4]` = QoS priority (`0..15`), bits `[3:0]` = max hops / TTL (`0..15`).
    pub routing_policy: u8,
    /// Last observed RSSI in dBm (`-128` = unknown).
    pub last_rssi_dbm: i8,
    /// Link telemetry metric: LQI (`0..255`), SNR, or battery voltage (`0x80 | v_x10`).
    pub link_metric: u8,
    /// 14-byte human-readable UTF-8 / ASCII display name.
    pub name: [u8; 14],
}

const _: () = {
    assert!(core::mem::size_of::<ContactRecord>() == 32);
    assert!(core::mem::align_of::<ContactRecord>() == 2);
};

impl ContactRecord {
    pub const SIZE: usize = 32;
    pub const STRIDE_SHIFT: u32 = 5; // 1 << 5 == 32 bytes

    // Behavioral & Subscription Bitflags
    pub const FLAG_MUTE_UNTIL_MATCH: u16 = 1 << 0;
    pub const FLAG_FAVORITE: u16 = 1 << 1;
    pub const FLAG_ACTIVE_NEIGHBOR: u16 = 1 << 2;
    pub const FLAG_ENCRYPTED: u16 = 1 << 3;
    pub const FLAG_AUTO_ACK: u16 = 1 << 4;
    pub const FLAG_SUBSCRIBED: u16 = 1 << 5;
    pub const FLAG_TX_PRESENCE: u16 = 1 << 6;
    pub const FLAG_RELAY_ALLOW: u16 = 1 << 7;
    pub const FLAG_PRIORITY_ALERT: u16 = 1 << 8;
    pub const FLAG_BEACON_TARGET: u16 = 1 << 9;
    pub const FLAG_SYSTEM_LOCKED: u16 = 1 << 10;

    pub const fn empty() -> Self {
        Self {
            id: [0; 8],
            parent_group: 0,
            flags: 0,
            kind: ContactKind::Endpoint as u8,
            scheme: AddressScheme::None as u8,
            transport: TransportClass::Any as u8,
            routing_policy: 0x03, // Default TTL = 3 hops, QoS = 0
            last_rssi_dbm: -128,
            link_metric: 0,
            name: [0; 14],
        }
    }

    /// Construct an endpoint or relay with a 48-bit MAC/node address and optional 16-bit port/sub-ID.
    pub fn new_mac48(kind: ContactKind, mac: [u8; 6], sub_port: u16, name: &str) -> Self {
        let mut rec = Self::empty();
        rec.kind = kind as u8;
        rec.scheme = AddressScheme::Mac48 as u8;
        rec.id[0..6].copy_from_slice(&mac);
        rec.id[6..8].copy_from_slice(&sub_port.to_be_bytes());
        rec.set_name(name);
        rec
    }

    /// Construct a numeric endpoint or group (e.g., radio ID, unit ID, talkgroup ID).
    pub fn new_numeric(kind: ContactKind, id: u32, name: &str) -> Self {
        let mut rec = Self::empty();
        rec.kind = kind as u8;
        rec.scheme = AddressScheme::Numeric as u8;
        rec.id[4..8].copy_from_slice(&id.to_be_bytes());
        rec.set_name(name);
        rec
    }

    /// Construct a multicast group record (`ContactKind::Group`).
    pub fn new_group(group_id: u32, name: &str) -> Self {
        let mut rec = Self::new_numeric(ContactKind::Group, group_id, name);
        rec.flags = Self::FLAG_SUBSCRIBED;
        rec
    }

    /// Construct a pub/sub topic record (`ContactKind::Topic`) from an arbitrary topic URI path.
    ///
    /// If `topic_uri` fits in 8 ASCII bytes (e.g. `"#WX-EU"`), uses `AddressScheme::Ascii`;
    /// otherwise stores its deterministic 64-bit FNV-1a hash via `AddressScheme::TopicHash`.
    pub fn new_topic(topic_uri: &str, display_name: &str) -> Self {
        let mut rec = Self::empty();
        rec.kind = ContactKind::Topic as u8;
        rec.flags = Self::FLAG_SUBSCRIBED;
        let uri_bytes = topic_uri.as_bytes();
        if !uri_bytes.is_empty() && uri_bytes.len() <= 8 {
            rec.scheme = AddressScheme::Ascii as u8;
            rec.id[..uri_bytes.len()].copy_from_slice(uri_bytes);
        } else {
            rec.scheme = AddressScheme::TopicHash as u8;
            rec.id = fnv1a_hash64(uri_bytes).to_be_bytes();
        }
        rec.set_name(display_name);
        rec
    }

    /// Construct an ASCII-identified station or callsign contact (e.g. `"IU2AAA-9"`).
    pub fn new_ascii(kind: ContactKind, ascii_id: &str, name: &str) -> Self {
        let mut rec = Self::empty();
        rec.kind = kind as u8;
        rec.scheme = AddressScheme::Ascii as u8;
        let bytes = ascii_id.as_bytes();
        let len = bytes.len().min(8);
        rec.id[..len].copy_from_slice(&bytes[..len]);
        rec.set_name(name);
        rec
    }

    /// Construct a tone-sequence contact (up to 16 DTMF or 5-Tone SelCall digits `0..=14`, padded with `0xF`).
    pub fn new_tone_seq(digits: &[u8], name: &str) -> Self {
        let mut rec = Self::empty();
        rec.kind = ContactKind::Endpoint as u8;
        rec.scheme = AddressScheme::ToneSeq as u8;
        rec.transport = TransportClass::ToneSignaling as u8;
        rec.id = [0xFF; 8];
        let count = digits.len().min(16);
        let mut i = 0;
        while i < count {
            let nib = digits[i] & 0x0F;
            let byte_idx = i >> 1;
            if (i & 1) == 0 {
                rec.id[byte_idx] = (nib << 4) | 0x0F;
            } else {
                rec.id[byte_idx] = (rec.id[byte_idx] & 0xF0) | nib;
            }
            i += 1;
        }
        rec.set_name(name);
        rec
    }

    /// Decode a packed `AddressScheme::ToneSeq` contact into up to 16 tone digits (`0..=14`).
    pub fn decode_tone_seq(&self, out: &mut [u8; 16]) -> usize {
        if self.address_scheme() != AddressScheme::ToneSeq {
            return 0;
        }
        let mut count = 0;
        let mut i = 0;
        while i < 8 {
            let hi = (self.id[i] >> 4) & 0x0F;
            if hi == 0x0F {
                break;
            }
            out[count] = hi;
            count += 1;
            let lo = self.id[i] & 0x0F;
            if lo == 0x0F {
                break;
            }
            out[count] = lo;
            count += 1;
            i += 1;
        }
        count
    }

    #[inline(always)]
    pub const fn contact_kind(&self) -> ContactKind {
        ContactKind::from_u8(self.kind)
    }

    #[inline(always)]
    pub const fn address_scheme(&self) -> AddressScheme {
        AddressScheme::from_u8(self.scheme)
    }

    #[inline(always)]
    pub const fn is_occupied(&self) -> bool {
        self.scheme != (AddressScheme::None as u8) || self.name[0] != 0
    }

    #[inline(always)]
    pub const fn is_group_or_topic(&self) -> bool {
        matches!(self.contact_kind(), ContactKind::Group | ContactKind::Topic)
    }

    #[inline(always)]
    pub const fn is_subscribed(&self) -> bool {
        (self.flags & Self::FLAG_SUBSCRIBED) != 0
    }

    #[inline(always)]
    pub const fn numeric_id(&self) -> u32 {
        u32::from_be_bytes([self.id[4], self.id[5], self.id[6], self.id[7]])
    }

    #[inline(always)]
    pub const fn numeric_id_u64(&self) -> u64 {
        u64::from_be_bytes(self.id)
    }

    #[inline(always)]
    pub const fn mac48(&self) -> [u8; 6] {
        [
            self.id[0], self.id[1], self.id[2], self.id[3], self.id[4], self.id[5],
        ]
    }

    #[inline(always)]
    pub const fn sub_port(&self) -> u16 {
        u16::from_be_bytes([self.id[6], self.id[7]])
    }

    #[inline(always)]
    pub const fn max_hops(&self) -> u8 {
        self.routing_policy & 0x0F
    }

    #[inline(always)]
    pub const fn qos_priority(&self) -> u8 {
        (self.routing_policy >> 4) & 0x0F
    }

    #[inline(always)]
    pub fn set_routing_policy(&mut self, qos: u8, max_hops: u8) {
        self.routing_policy = ((qos & 0x0F) << 4) | (max_hops & 0x0F);
    }

    /// Check whether this contact matches an incoming topic URI string.
    pub fn matches_topic(&self, topic_uri: &str) -> bool {
        if self.contact_kind() != ContactKind::Topic {
            return false;
        }
        let uri_bytes = topic_uri.as_bytes();
        match self.address_scheme() {
            AddressScheme::Ascii => {
                if uri_bytes.len() > 8 {
                    return false;
                }
                let mut expected = [0u8; 8];
                expected[..uri_bytes.len()].copy_from_slice(uri_bytes);
                self.id == expected
            }
            AddressScheme::TopicHash => self.id == fnv1a_hash64(uri_bytes).to_be_bytes(),
            _ => false,
        }
    }

    pub fn name_as_str(&self) -> &str {
        super::read_fixed_str(&self.name)
    }

    pub fn set_name(&mut self, s: &str) {
        super::write_fixed_str(&mut self.name, s, 14);
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
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_record_endpoints_groups_topics_and_tones_roundtrip() {
        // 1. Numeric Endpoint in a Group
        let mut peer = ContactRecord::new_numeric(ContactKind::Endpoint, 0x0102_0304, "IU2AAA");
        peer.parent_group = 5;
        peer.flags = ContactRecord::FLAG_FAVORITE | ContactRecord::FLAG_AUTO_ACK;
        peer.last_rssi_dbm = -87;
        peer.link_metric = 240;
        peer.set_routing_policy(3, 7);

        let decoded = ContactRecord::from_bytes(peer.as_bytes());
        assert_eq!(decoded, peer);
        assert_eq!(decoded.contact_kind(), ContactKind::Endpoint);
        assert_eq!(decoded.address_scheme(), AddressScheme::Numeric);
        assert_eq!(decoded.numeric_id(), 0x0102_0304);
        assert_eq!(decoded.parent_group, 5);
        assert_eq!(decoded.qos_priority(), 3);
        assert_eq!(decoded.max_hops(), 7);
        assert_eq!(decoded.name_as_str(), "IU2AAA");

        // 2. Short & Hierarchical Pub/Sub Topics
        let short_topic = ContactRecord::new_topic("#WX-EU", "WX EUROPE");
        assert_eq!(short_topic.contact_kind(), ContactKind::Topic);
        assert_eq!(short_topic.address_scheme(), AddressScheme::Ascii);
        assert!(short_topic.is_subscribed());
        assert!(short_topic.matches_topic("#WX-EU"));
        assert!(!short_topic.matches_topic("#WX-US"));

        let deep_topic = ContactRecord::new_topic("mesh/eu/alpine/emergency", "#ALPINE-SOS");
        assert_eq!(deep_topic.address_scheme(), AddressScheme::TopicHash);
        assert!(deep_topic.matches_topic("mesh/eu/alpine/emergency"));
        assert!(!deep_topic.matches_topic("mesh/eu/alpine/chat"));

        // 3. Packed 16-digit DTMF / 5-Tone SelCall sequence
        let digits = [1, 2, 3, 4, 5, 0xA, 0xE];
        let tone_ct = ContactRecord::new_tone_seq(&digits, "RPT-OPEN");
        let mut unpacked = [0u8; 16];
        let len = tone_ct.decode_tone_seq(&mut unpacked);
        assert_eq!(len, 7);
        assert_eq!(&unpacked[..len], &digits);
    }
}
