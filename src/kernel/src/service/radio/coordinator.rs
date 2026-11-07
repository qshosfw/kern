//! Formal Microkernel RF Radio Coordinator Contract (§8.2 / AGENTS.md §1.1).
//!
//! Defines the normative trait for multi-VFO coordination, frequency stepping,
//! cross-band routing, PTT arbitration, and fine-grained RF resource locks.

use super::arbitrator::{ClientToken, TxPermission};
use super::channel::{ChannelConfig, VfoSlot};

/// High-level declarative RF Radio Coordinator interface.
///
/// Decouples applications, UI shells, and background daemons from physical
/// radio chipsets and peripheral driver instances.
pub trait RadioCoordinator {
    /// Active primary VFO slot selected for user interaction and TX.
    fn active_vfo(&self) -> VfoSlot;

    /// Listening VFO slot currently being monitored or dwelled upon.
    fn listening_vfo(&self) -> VfoSlot;

    /// Select a specific VFO slot as active primary.
    fn select_vfo(&mut self, slot: VfoSlot);

    /// Toggle active VFO selection (e.g. between VFO A and VFO B).
    fn toggle_vfo(&mut self);

    /// Step active or targeted VFO frequency by signed delta in Hertz.
    fn step_vfo_frequency(&mut self, slot: VfoSlot, delta_hz: i32);

    /// Toggle repeater reverse (swap TX/RX frequencies/tones) on target VFO.
    fn toggle_reverse(&mut self, slot: VfoSlot);

    /// Whether RF transmitter is currently keyed and active.
    fn is_transmitting(&self) -> bool;

    /// Whether valid carrier/signal is detected on listening VFO.
    fn is_carrier_active(&self) -> bool;

    /// Immutable reference to the active VFO channel configuration.
    fn active_channel(&self) -> &ChannelConfig;

    /// Immutable reference to the listening VFO channel configuration.
    fn listening_channel(&self) -> &ChannelConfig;

    /// Request PTT start on behalf of the foreground user interface.
    fn request_ptt_start(&mut self) -> TxPermission;

    /// Request PTT start on behalf of a specific client and lock tier.
    fn request_ptt_start_client(&mut self, client: ClientToken) -> TxPermission;

    /// Request PTT stop / key release and disengage transmitter.
    fn request_ptt_stop(&mut self);

    /// Request fine-grained resource lock on a VFO slot.
    fn request_vfo_lock(
        &mut self,
        client: ClientToken,
        slot: VfoSlot,
        timeout_ms: u16,
        mute_audio: bool,
    ) -> bool;

    /// Release an existing resource lock held by client.
    fn release_vfo_lock(&mut self, client: ClientToken) -> bool;

    /// Whether any VFO or RF resource is currently locked by a client.
    fn is_locked(&self) -> bool;
}
