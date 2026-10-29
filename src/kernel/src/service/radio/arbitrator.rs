//! RF Resource Arbitration Primitives, Client Identifiers, and Lock Contracts (§1.1 / §1.5).

use super::channel::VfoSlot;

/// Universal 32-bit FourCC client identifier for RF arbitration.
///
/// Any application, driver, sidecar, or background protocol can declare its own
/// unique 4-character ASCII tag without requiring centralized coordinator modifications.
///
/// Standard conventions:
/// - `b"SYST"`: Operating system kernel / background supervisor
/// - `b"VFO "`: Interactive VFO user interface
/// - `b"SCAN"`: Channel / frequency search engine
/// - `b"FSK "`: Physical FSK sidecar modem
/// - `b"HRMS"`: Hermes mesh network engine
/// - `b"APRS"`: APRS packet terminal
/// - `b"DIGI"`: Digital modes engine
/// - `b"BCON"`: Automated beacon transmitter
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientId(pub u32);

impl ClientId {
    pub const NONE: Self = Self(0);
    pub const SYSTEM: Self = Self::from_tag(b"SYST");
    pub const VFO_UI: Self = Self::from_tag(b"VFO ");
    pub const SCANNER: Self = Self::from_tag(b"SCAN");
    pub const FSK_SIDECAR: Self = Self::from_tag(b"FSK ");
    pub const HERMES_MESH: Self = Self::from_tag(b"HRMS");
    pub const APRS: Self = Self::from_tag(b"APRS");
    pub const DIGMODE: Self = Self::from_tag(b"DIGI");
    pub const BEACON: Self = Self::from_tag(b"BCON");

    #[inline(always)]
    pub const fn from_tag(tag: &[u8; 4]) -> Self {
        Self(u32::from_be_bytes(*tag))
    }

    #[inline(always)]
    pub const fn to_tag(&self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    /// Render tag as 4-character ASCII string into caller-provided buffer.
    pub fn tag_str<'a>(&self, buf: &'a mut [u8; 4]) -> &'a str {
        *buf = self.to_tag();
        unsafe { core::str::from_utf8_unchecked(buf) }
    }
}

/// Hierarchical arbitration priority tier for RF hardware resource locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockTier(pub u8);

impl LockTier {
    pub const IDLE: Self = Self(0);
    pub const BACKGROUND: Self = Self(10);
    pub const SERVICE: Self = Self(20);
    pub const FOREGROUND: Self = Self(30);
    pub const EXCLUSIVE: Self = Self(40);
    pub const EMERGENCY: Self = Self(50);

    #[inline(always)]
    pub const fn level(&self) -> u8 {
        self.0
    }
}

/// Generic client token for arbitration and resource ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientToken {
    pub id: ClientId,
    pub tier: LockTier,
}

impl ClientToken {
    pub const IDLE: Self = Self {
        id: ClientId::SYSTEM,
        tier: LockTier::IDLE,
    };

    pub const fn new(id: ClientId, tier: LockTier) -> Self {
        Self { id, tier }
    }

    #[inline(always)]
    pub const fn priority(&self) -> u8 {
        self.tier.0
    }

    #[inline(always)]
    pub const fn is_idle(&self) -> bool {
        self.tier.0 == LockTier::IDLE.0
    }
}

/// Bitmask of fine-grained RF and baseband hardware resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RfResourceMask(pub u8);

impl RfResourceMask {
    pub const NONE: Self = Self(0);
    pub const VFO_A: Self = Self(1 << 0);
    pub const VFO_B: Self = Self(1 << 1);
    pub const VFO_C: Self = Self(1 << 2);
    pub const TRANSMITTER: Self = Self(1 << 3);
    pub const AUDIO_OUTPUT: Self = Self(1 << 4);
    pub const BASEBAND_MODEM: Self = Self(1 << 5);
    /// Secondary broad-band HF receiver (e.g. Si4732).
    pub const SECONDARY_RECEIVER: Self = Self(1 << 6);
    /// Broadcast FM receiver alias (BK1080).
    pub const BROADCAST_FM: Self = Self(1 << 6);
    /// Auxiliary data bus / GPS / BLE telemetry.
    pub const AUX_TELEMETRY: Self = Self(1 << 7);
    pub const ALL: Self = Self(0xFF);

    #[inline(always)]
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline(always)]
    pub const fn intersects(&self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    #[inline(always)]
    pub const fn union(&self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline(always)]
    pub const fn for_vfo(slot: VfoSlot) -> Self {
        match slot {
            VfoSlot::VfoA => Self::VFO_A,
            VfoSlot::VfoB => Self::VFO_B,
            _ => Self::VFO_C,
        }
    }
}

/// Active RF arbitration lock descriptor holding hardware resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfoArbitrationLock {
    pub client: ClientToken,
    pub resources: RfResourceMask,
    pub slot: VfoSlot,
    pub remaining_ms: u16,
    pub audio_muted: bool,
}

impl VfoArbitrationLock {
    pub const IDLE: Self = Self {
        client: ClientToken::IDLE,
        resources: RfResourceMask::NONE,
        slot: VfoSlot::VfoA,
        remaining_ms: 0,
        audio_muted: false,
    };

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        !self.client.is_idle() && self.remaining_ms > 0
    }
}

/// Participant contract for applications and services that interface with the RF coordinator.
pub trait RfClientParticipant {
    /// Return the unique FourCC identifier of this client.
    fn client_id(&self) -> ClientId;

    /// Default priority tier when requesting resource locks.
    fn default_tier(&self) -> LockTier {
        LockTier::SERVICE
    }

    /// Construct an active client token for arbitration requests.
    fn client_token(&self) -> ClientToken {
        ClientToken::new(self.client_id(), self.default_tier())
    }
}

/// Busy Channel Lockout (BCLO) arbitration policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BcloPolicy {
    /// Transmission always allowed regardless of channel carrier.
    Disabled,
    /// Transmission blocked if carrier is present (carrier squelch open).
    CarrierOnly,
    /// Transmission blocked only if carrier is present and subaudible CTCSS/DCS does not match.
    ToneMismatchOnly,
}

/// Detailed result when evaluating or requesting RF transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxPermission {
    /// Transmission granted and permitted.
    Granted,
    /// Transmission blocked because channel is busy under active BCLO policy.
    BlockedBusyChannel,
    /// Transmission blocked because an equal or higher-priority client holds transmitter lock.
    BlockedResourceLocked,
    /// Transmission blocked because channel or bandplan is marked transmit-inhibited (RX-only).
    BlockedTxInhibited,
    /// Transmission blocked because Time-Out Timer expired; PTT must be released to clear lockout.
    BlockedTotPenalty,
}

impl TxPermission {
    #[inline(always)]
    pub const fn is_granted(&self) -> bool {
        matches!(self, Self::Granted)
    }
}

/// Granular RF transmission state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxState {
    /// Radio receiver active (idle).
    Idle,
    /// Radio actively transmitting carrier and modulation.
    Active,
    /// PTT released; transmitting subaudible 55 Hz / phase-shifted tail burst before unkeying PA.
    TailBurst { remaining_ms: u16 },
}

impl TxState {
    #[inline(always)]
    pub const fn is_transmitting(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_id_fourcc() {
        let vfo = ClientId::VFO_UI;
        let mut buf = [0u8; 4];
        assert_eq!(vfo.tag_str(&mut buf), "VFO ");
        assert_eq!(ClientId::from_tag(b"SCAN"), ClientId::SCANNER);
    }

    #[test]
    fn test_lock_tier_preemption() {
        assert!(LockTier::EMERGENCY > LockTier::EXCLUSIVE);
        assert!(LockTier::EXCLUSIVE > LockTier::FOREGROUND);
        assert!(LockTier::FOREGROUND > LockTier::SERVICE);
        assert!(LockTier::SERVICE > LockTier::BACKGROUND);
        assert!(LockTier::BACKGROUND > LockTier::IDLE);
    }

    #[test]
    fn test_resource_mask_intersections() {
        let vfo_a = RfResourceMask::VFO_A;
        let vfo_b = RfResourceMask::VFO_B;
        assert!(!vfo_a.intersects(vfo_b));

        let combo = vfo_a.union(RfResourceMask::AUDIO_OUTPUT);
        assert!(combo.intersects(RfResourceMask::AUDIO_OUTPUT));
        assert!(combo.intersects(RfResourceMask::VFO_A));
        assert!(!combo.intersects(vfo_b));
    }
}
