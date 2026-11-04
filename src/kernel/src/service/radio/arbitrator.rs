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
    /// Transmission blocked because battery voltage is critically low (< cutoff threshold).
    BlockedBatteryCritical,
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
    /// PTT keyed; transmitting unmodulated carrier / subaudible tone lead-in before opening mic.
    LeadIn { remaining_ms: u16 },
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

/// Event returned when an arbitration lock expires or is restored during a time tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArbitrationEvent {
    /// Active lock expired and was released.
    LockReleased {
        client: ClientToken,
        resources: RfResourceMask,
        slot: VfoSlot,
        was_muted: bool,
    },
    /// A previously preempted lock was restored to active status.
    LockRestored {
        client: ClientToken,
        resources: RfResourceMask,
        slot: VfoSlot,
        needs_mute: bool,
    },
}

/// System RF Hardware Resource Arbitrator.
///
/// Provides fine-grained conflict arbitration, priority-tiered lock preemption,
/// and automatic stack restoration for multi-task radio operations (§1.1 / §1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioArbitrator {
    pub active_lock: VfoArbitrationLock,
    pub saved_lock: Option<VfoArbitrationLock>,
}

impl RadioArbitrator {
    pub const fn new() -> Self {
        Self {
            active_lock: VfoArbitrationLock::IDLE,
            saved_lock: None,
        }
    }

    /// Check if any client currently holds an active lock.
    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.active_lock.is_active()
    }

    /// The client currently holding the active lock.
    #[inline(always)]
    pub fn active_client(&self) -> ClientToken {
        self.active_lock.client
    }

    /// The VFO slot currently locked, if any.
    #[inline(always)]
    pub fn locked_slot(&self) -> Option<VfoSlot> {
        if self.active_lock.is_active() {
            Some(self.active_lock.slot)
        } else {
            None
        }
    }

    /// Current arbitration lock stack depth (0 = idle, 1 = active, 2 = active + preempted).
    #[inline(always)]
    pub fn lock_depth(&self) -> u8 {
        if !self.active_lock.is_active() {
            0
        } else if self.saved_lock.is_some() {
            2
        } else {
            1
        }
    }

    /// Check if a specific resource is locked by any client other than `exempt_client`.
    #[inline]
    pub fn is_resource_locked(
        &self,
        resources: RfResourceMask,
        exempt_client: Option<ClientToken>,
    ) -> bool {
        if !self.active_lock.is_active() {
            return false;
        }
        if let Some(exempt) = exempt_client {
            if self.active_lock.client == exempt {
                return false;
            }
        }
        self.active_lock.resources.intersects(resources)
    }

    /// Request fine-grained resource locks with priority preemption.
    pub fn request_lock(
        &mut self,
        client: ClientToken,
        resources: RfResourceMask,
        slot: VfoSlot,
        timeout_ms: u16,
        mute_audio: bool,
        is_transmitting: bool,
    ) -> bool {
        if is_transmitting && resources.intersects(RfResourceMask::TRANSMITTER) {
            return false;
        }

        if self.active_lock.is_active() && self.active_lock.client != client {
            if self.active_lock.resources.intersects(resources) {
                if client.priority() <= self.active_lock.client.priority() {
                    return false; // Lower or equal priority cannot preempt
                }
                // Preempt active lock: stash into saved_lock
                self.saved_lock = Some(self.active_lock);
            }
        }

        self.active_lock = VfoArbitrationLock {
            client,
            resources,
            slot,
            remaining_ms: timeout_ms.max(10),
            audio_muted: mute_audio,
        };
        true
    }

    /// Extend the active lock duration for the holding client.
    pub fn extend_lock(&mut self, client: ClientToken, timeout_ms: u16) -> bool {
        if self.active_lock.is_active() && self.active_lock.client == client {
            self.active_lock.remaining_ms = timeout_ms.max(self.active_lock.remaining_ms);
            true
        } else {
            false
        }
    }

    /// Pop active lock and restore preempted saved lock, if any.
    pub fn pop_and_restore(&mut self) -> Option<VfoArbitrationLock> {
        if let Some(saved) = self.saved_lock.take() {
            self.active_lock = saved;
            Some(saved)
        } else {
            self.active_lock = VfoArbitrationLock::IDLE;
            None
        }
    }

    /// Explicitly release a lock held by a client.
    pub fn release_lock(&mut self, client: ClientToken) -> bool {
        if self.active_lock.client == client {
            self.pop_and_restore();
            true
        } else if let Some(saved) = self.saved_lock {
            if saved.client == client {
                self.saved_lock = None;
                return true;
            }
            false
        } else {
            false
        }
    }

    /// Force-release all locks.
    pub fn force_release_all(&mut self) {
        self.active_lock = VfoArbitrationLock::IDLE;
        self.saved_lock = None;
    }

    /// Step lock countdown by `delta_ms` and return an event if lock expired.
    pub fn tick(&mut self, delta_ms: u16) -> Option<ArbitrationEvent> {
        if !self.active_lock.is_active() {
            return None;
        }

        if self.active_lock.remaining_ms <= delta_ms {
            let client = self.active_lock.client;
            let resources = self.active_lock.resources;
            let slot = self.active_lock.slot;
            let was_muted = self.active_lock.audio_muted;

            let restored = self.pop_and_restore();
            if let Some(res) = restored {
                Some(ArbitrationEvent::LockRestored {
                    client: res.client,
                    resources: res.resources,
                    slot: res.slot,
                    needs_mute: res.audio_muted,
                })
            } else {
                Some(ArbitrationEvent::LockReleased {
                    client,
                    resources,
                    slot,
                    was_muted,
                })
            }
        } else {
            self.active_lock.remaining_ms = self.active_lock.remaining_ms.saturating_sub(delta_ms);
            None
        }
    }

    /// Evaluate whether a client has permission to transmit on the target channel.
    pub fn evaluate_tx_permission(
        &self,
        client: ClientToken,
        bclo: BcloPolicy,
        carrier_active: bool,
        tx_inhibited: bool,
        tot_locked_out: bool,
    ) -> TxPermission {
        if tot_locked_out {
            return TxPermission::BlockedTotPenalty;
        }
        if tx_inhibited {
            return TxPermission::BlockedTxInhibited;
        }
        if self.is_resource_locked(RfResourceMask::TRANSMITTER, Some(client))
            && client.priority() < LockTier::EXCLUSIVE.0
        {
            return TxPermission::BlockedResourceLocked;
        }
        if carrier_active && bclo != BcloPolicy::Disabled {
            return TxPermission::BlockedBusyChannel;
        }
        TxPermission::Granted
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

    #[test]
    fn test_arbitrator_lock_and_preemption() {
        let mut arb = RadioArbitrator::new();
        assert!(!arb.is_locked());
        assert_eq!(arb.lock_depth(), 0);

        let scanner = ClientToken::new(ClientId::SCANNER, LockTier::BACKGROUND);
        let fsk = ClientToken::new(ClientId::FSK_SIDECAR, LockTier::EXCLUSIVE);

        // 1. Acquire background lock for Scanner on VFO A
        assert!(arb.request_lock(
            scanner,
            RfResourceMask::VFO_A,
            VfoSlot::VfoA,
            500,
            true,
            false
        ));
        assert!(arb.is_locked());
        assert_eq!(arb.lock_depth(), 1);
        assert_eq!(arb.active_client(), scanner);
        assert_eq!(arb.locked_slot(), Some(VfoSlot::VfoA));

        // 2. Reject lower/equal priority lock on overlapping resource
        let mesh = ClientToken::new(ClientId::HERMES_MESH, LockTier::BACKGROUND);
        assert!(!arb.request_lock(
            mesh,
            RfResourceMask::VFO_A,
            VfoSlot::VfoA,
            300,
            false,
            false
        ));
        assert_eq!(arb.lock_depth(), 1);
        assert_eq!(arb.active_client(), scanner);

        // 3. Higher priority FSK sidecar preempts Scanner
        assert!(arb.request_lock(fsk, RfResourceMask::VFO_A, VfoSlot::VfoA, 200, true, false));
        assert_eq!(arb.lock_depth(), 2);
        assert_eq!(arb.active_client(), fsk);
        assert!(arb.saved_lock.is_some());
        assert_eq!(arb.saved_lock.unwrap().client, scanner);

        // 4. When FSK releases or expires, Scanner is restored
        assert!(arb.release_lock(fsk));
        assert_eq!(arb.lock_depth(), 1);
        assert_eq!(arb.active_client(), scanner);
        assert!(arb.saved_lock.is_none());

        // 5. Release scanner
        assert!(arb.release_lock(scanner));
        assert_eq!(arb.lock_depth(), 0);
        assert!(!arb.is_locked());
    }

    #[test]
    fn test_arbitrator_tick_timeout_and_restoration() {
        let mut arb = RadioArbitrator::new();
        let bg = ClientToken::new(ClientId::SCANNER, LockTier::BACKGROUND);
        let fg = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);

        arb.request_lock(bg, RfResourceMask::VFO_B, VfoSlot::VfoB, 100, false, false);
        arb.request_lock(fg, RfResourceMask::VFO_B, VfoSlot::VfoB, 50, true, false);
        assert_eq!(arb.lock_depth(), 2);

        // Step 20ms: no expiration yet
        assert_eq!(arb.tick(20), None);
        assert_eq!(arb.active_lock.remaining_ms, 30);

        // Step 30ms: fg expires, restoring bg
        let ev = arb.tick(30);
        assert!(matches!(
            ev,
            Some(ArbitrationEvent::LockRestored {
                client,
                slot: VfoSlot::VfoB,
                needs_mute: false,
                ..
            }) if client == bg
        ));
        assert_eq!(arb.lock_depth(), 1);
        assert_eq!(arb.active_client(), bg);

        // Step 100ms: bg expires, arbitration idle
        let ev2 = arb.tick(100);
        assert!(matches!(
            ev2,
            Some(ArbitrationEvent::LockReleased {
                client,
                slot: VfoSlot::VfoB,
                was_muted: false,
                ..
            }) if client == bg
        ));
        assert_eq!(arb.lock_depth(), 0);
        assert!(!arb.is_locked());
    }

    #[test]
    fn test_arbitrator_tx_permissions() {
        let mut arb = RadioArbitrator::new();
        let user = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);
        let sidecar = ClientToken::new(ClientId::FSK_SIDECAR, LockTier::EXCLUSIVE);

        // Standard idle permission
        assert_eq!(
            arb.evaluate_tx_permission(user, BcloPolicy::CarrierOnly, false, false, false),
            TxPermission::Granted
        );

        // TOT lockout penalty
        assert_eq!(
            arb.evaluate_tx_permission(user, BcloPolicy::Disabled, false, false, true),
            TxPermission::BlockedTotPenalty
        );

        // TX Inhibited (RX-only)
        assert_eq!(
            arb.evaluate_tx_permission(user, BcloPolicy::Disabled, false, true, false),
            TxPermission::BlockedTxInhibited
        );

        // BCLO carrier active
        assert_eq!(
            arb.evaluate_tx_permission(user, BcloPolicy::CarrierOnly, true, false, false),
            TxPermission::BlockedBusyChannel
        );

        // Resource locked by another client
        arb.request_lock(
            sidecar,
            RfResourceMask::TRANSMITTER,
            VfoSlot::VfoA,
            500,
            false,
            false,
        );
        assert_eq!(
            arb.evaluate_tx_permission(user, BcloPolicy::Disabled, false, false, false),
            TxPermission::BlockedResourceLocked
        );

        // Holding client allowed to transmit
        assert_eq!(
            arb.evaluate_tx_permission(sidecar, BcloPolicy::Disabled, false, false, false),
            TxPermission::Granted
        );
    }
}
