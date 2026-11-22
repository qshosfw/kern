//! Multi-VFO and RF Subsystem Coordinator Middleware.
//!
//! Orchestrates dual-VFO and triple-VFO priority scanning (Dual Watch / Triple Watch),
//! adaptive dwell watching, cross-band transmission offsets, repeater 1750 Hz bursts,
//! tail elimination tone bursts, FSK sidecar channel locking, and granular multi-app RF arbitration.

use super::dwell::DwellController;
pub use kernel::service::radio::*;
use utils::units::{Frequency, Hertz};

/// Dual Watch / Multi-VFO dwelling operational modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DualWatchMode {
    Off,
    EqualDwell {
        dwell_ms: u16,
    },
    Priority {
        priority_vfo: VfoSlot,
        check_interval_ms: u16,
    },
    Adaptive,
    TripleWatch {
        dwell_ms: u16,
    },
}

/// Cross-band RF transmit and receive routing policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CrossBandMode {
    Off = 0,
    FixedTxOnA = 1,
    FixedTxOnB = 2,
    MasterFollowsRx = 3,
}

impl CrossBandMode {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if val <= 3 {
            Some(unsafe { core::mem::transmute::<u8, Self>(val) })
        } else {
            None
        }
    }
}

/// Transmit target selection policy when Dual Watch / Multi-VFO is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DualWatchTxPolicy {
    /// Always transmit on the actively selected primary VFO.
    ActiveVfo,
    /// Transmit on the listening VFO if carrier was open or within post-carrier hold period.
    FollowCarrier,
}

/// Physical PTT key behavior policy (`/pref/keys.cfg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PttMode {
    Momentary = 0,
    Toggle = 1,
    DoublePressLatch = 2,
}

impl PttMode {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Toggle,
            2 => Self::DoublePressLatch,
            _ => Self::Momentary,
        }
    }
}

/// High-level outcome of processing a physical or logical PTT key transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEventOutcome {
    None,
    Started(TxPermission),
    Stopped,
}

/// Snapshot of real-time channel activity and signal telemetry for UI widgets and applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelTelemetry {
    pub slot: VfoSlot,
    pub rssi_dbm: i16,
    pub activity_count: u16,
    pub is_carrier_open: bool,
    pub is_locked: bool,
    pub lock_client: ClientId,
}

/// Real-time activity state of an individual VFO for display UI widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfoActivityState {
    Idle,
    Listening,
    CarrierActive,
    FskLocked,
    Transmitting,
}

/// FSK Sidecar background listening policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FskSidecarMode {
    /// Background FSK reception disabled.
    Disabled,
    /// Follow whichever VFO is currently active/dwelling when voice squelch is closed.
    FollowActiveVfo,
    /// Dedicate background FSK reception strictly to a single designated VFO (e.g. VFO B for APRS).
    DedicatedVfo(VfoSlot),
}

/// Granular configuration parameters for the background FSK Sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FskSidecarConfig {
    /// Operational listening mode.
    pub mode: FskSidecarMode,
    /// Automatically mute speaker audio when FSK sync is detected to prevent loud digital screeching.
    pub mute_audio_on_sync: bool,
    /// Duration to freeze Dual Watch and lock VFO dwelling when FSK sync/preamble is detected (ms). Default: 2000 ms.
    pub sync_hold_ms: u16,
    /// Post-frame guard time to keep VFO locked waiting for potential ACKs or reply bursts (ms). Default: 500 ms.
    pub post_frame_hold_ms: u16,
    /// Voice squelch priority override: an incoming voice carrier immediately interrupts idle FSK hold.
    pub voice_priority_override: bool,
}

impl FskSidecarConfig {
    pub const DEFAULT: Self = Self {
        mode: FskSidecarMode::FollowActiveVfo,
        mute_audio_on_sync: true,
        sync_hold_ms: 2000,
        post_frame_hold_ms: 500,
        voice_priority_override: true,
    };
}

/// Granular audio routing state for coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioRoutingState {
    /// Normal voice audio path (unmuted when squelch opens).
    NormalVoice,
    /// Muted for background digital mode or FSK preamble.
    MutedForDigital,
    /// 1750 Hz repeater access tone burst active.
    ToneBurstActive { remaining_ms: u16 },
    /// Raw uncompressed / bypass baseband audio.
    BypassRaw,
}

/// System-level Multi-VFO, Sidecar, and RF Subsystem Coordinator.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct RadioCoordinator {
    // ── Hot 1-byte scalar fields (offset 0..22: 1-insn ldrb/strb) ─────────
    pub active_vfo: VfoSlot,
    pub listening_vfo: VfoSlot,
    pub cross_band: CrossBandMode,
    pub is_transmitting: bool,
    /// Granular transmission state machine (Idle, Active, TailBurst).
    pub tx_state: TxState,
    pub carrier_active: bool,
    pub subaudible_matched: bool,
    pub fsk_sync_active: bool,
    pub audio_path_muted: bool,
    /// Physical tuner currently routed through the audio cross-bar to the power amplifier.
    pub audio_source: AudioCrossbarSource,
    /// Busy Channel Lockout evaluation policy.
    pub bclo_policy: BcloPolicy,
    /// Dual Watch transmit target selection policy.
    pub dw_tx_policy: DualWatchTxPolicy,
    /// Pre-timeout acoustic warning lead time in seconds. Default: 10s.
    pub tot_warning_lead_seconds: u8,
    /// Pre-timeout acoustic warning flag.
    pub tot_warning_given: bool,
    /// Lockout penalty active: PTT must be released before transmitting again.
    pub tot_locked_out: bool,
    /// Whether dynamic adaptive squelch tracking is enabled.
    pub adaptive_squelch_enabled: bool,
    /// Sub-divider counter for background adaptive squelch floor sampling.
    pub adaptive_tick_div: u8,
    /// True if PA power was folded back due to high die/board temperature during prolonged TX.
    pub pa_thermal_foldback_active: bool,
    /// True when manual Monitor (squelch defeat) is active on the listening VFO.
    pub monitor_active: bool,
    /// True when PTT is latched on in `PttMode::Toggle` or `PttMode::DoublePressLatch`.
    pub ptt_latched: bool,

    // ── Hot 2-byte scalar fields (offset 20..58: 1-insn ldrh/strh) ─────────
    pub dual_watch: DualWatchMode,
    /// Time-Out Timer limit in seconds (0 = disabled).
    pub tot_limit_seconds: u16,
    /// Current transmission elapsed time in 10ms ticks.
    pub tot_elapsed_10ms: u16,
    pub dwell: DwellController,
    pub activity_count: [u16; 3],
    pub rssi_moving_avg: [i16; 3],
    /// Timestamp (ms) of the last PTT press for `PttMode::DoublePressLatch` detection.
    pub last_ptt_press_ms: u32,

    // ── Sub-controllers and state machines ───────────────────────────────
    /// Real-time RX Squelch Tail Elimination (STE) controller.
    pub ste: RxSteController,
    pub fsk_config: FskSidecarConfig,
    pub arbitrator: RadioArbitrator,
    pub audio_routing: AudioRoutingState,
    pub vfo_memory_index: [Option<u16>; 3],
    /// Per-VFO dynamic noise-floor estimator for Adaptive Squelch tracking.
    pub adaptive_squelch: [AdaptiveSquelchTracker; 3],

    // ── Large channel storage array (150+ bytes) ─────────────────────────
    pub channels: [ChannelConfig; 3],
}

impl RadioCoordinator {
    pub const fn new() -> Self {
        let mut ch_a = ChannelConfig::new(433_500_000);
        ch_a.power = RfPowerLevel::High;
        ch_a.bandwidth = Bandwidth::Narrow12k5;

        let mut ch_b = ChannelConfig::new(145_500_000);
        ch_b.power = RfPowerLevel::Low;
        ch_b.bandwidth = Bandwidth::Wide25k;

        let mut ch_c = ChannelConfig::new(144_800_000);
        ch_c.power = RfPowerLevel::Low;
        ch_c.bandwidth = Bandwidth::Narrow12k5;

        Self {
            active_vfo: VfoSlot::VfoA,
            listening_vfo: VfoSlot::VfoA,
            dual_watch: DualWatchMode::Off,
            cross_band: CrossBandMode::Off,
            is_transmitting: false,
            tx_state: TxState::Idle,
            carrier_active: false,
            subaudible_matched: false,
            fsk_sync_active: false,
            audio_path_muted: true,
            audio_source: AudioCrossbarSource::Primary,
            bclo_policy: BcloPolicy::CarrierOnly,
            dw_tx_policy: DualWatchTxPolicy::ActiveVfo,
            tot_warning_lead_seconds: 10,
            tot_warning_given: false,
            tot_locked_out: false,
            adaptive_squelch_enabled: true,
            adaptive_tick_div: 0,
            pa_thermal_foldback_active: false,
            monitor_active: false,
            ptt_latched: false,
            tot_limit_seconds: 60, // Default 60-second TOT
            tot_elapsed_10ms: 0,
            activity_count: [0, 0, 0],
            rssi_moving_avg: [-120, -120, -120],
            last_ptt_press_ms: u32::MAX - 500,
            ste: RxSteController::new(),
            dwell: DwellController::new(),
            audio_routing: AudioRoutingState::NormalVoice,
            arbitrator: RadioArbitrator::new(),
            fsk_config: FskSidecarConfig::DEFAULT,
            vfo_memory_index: [None, Some(1), None],
            adaptive_squelch: [
                AdaptiveSquelchTracker::new(),
                AdaptiveSquelchTracker::new(),
                AdaptiveSquelchTracker::new(),
            ],
            channels: [ch_a, ch_b, ch_c],
        }
    }

    /// Initialize non-zero fields in-place on a zeroed `RadioCoordinator` instance.
    pub fn init_zeroed(&mut self) {
        const INIT_CH: [(u32, RfPowerLevel, Bandwidth); 3] = [
            (433_500_000, RfPowerLevel::High, Bandwidth::Narrow12k5),
            (145_500_000, RfPowerLevel::Low, Bandwidth::Wide25k),
            (144_800_000, RfPowerLevel::Low, Bandwidth::Narrow12k5),
        ];
        for (i, &(freq, power, bw)) in INIT_CH.iter().enumerate() {
            let ch = &mut self.channels[i];
            ch.rx_freq = Frequency::new(freq);
            ch.step = StepSetting::DEFAULT;
            ch.power = power;
            ch.bandwidth = bw;
            ch.squelch_level = 4;
            ch.ste_mode = CssTailMode::Tone55Hz;
        }
        self.fsk_config = FskSidecarConfig::DEFAULT;
        self.vfo_memory_index[1] = Some(1);
        self.arbitrator.active_lock.client.id = ClientId::SYSTEM;
        self.dwell.resume_delay_ms = DwellController::DEFAULT_RESUME_DELAY_MS;
        self.rssi_moving_avg = [-120; 3];
        self.audio_path_muted = true;
        self.bclo_policy = BcloPolicy::CarrierOnly;
        self.tot_limit_seconds = 60;
        self.tot_warning_lead_seconds = 10;
        self.last_ptt_press_ms = u32::MAX - 500;
        self.adaptive_squelch = [AdaptiveSquelchTracker::new(); 3];
        self.adaptive_squelch_enabled = true;
    }

    #[inline(never)]
    pub fn active_channel(&self) -> &ChannelConfig {
        &self.channels[self.active_vfo.index().min(self.channels.len() - 1)]
    }

    #[inline(never)]
    pub fn active_channel_mut(&mut self) -> &mut ChannelConfig {
        let idx = self.active_vfo.index().min(self.channels.len() - 1);
        &mut self.channels[idx]
    }

    #[inline(never)]
    pub fn listening_channel(&self) -> &ChannelConfig {
        &self.channels[self.listening_vfo.index().min(self.channels.len() - 1)]
    }

    /// Query the physical tuner currently routed through the audio cross-bar.
    #[inline(always)]
    pub fn audio_source(&self) -> AudioCrossbarSource {
        self.audio_source
    }

    /// Dynamically route physical tuner audio to speaker / power amplifier.
    #[inline(always)]
    pub fn set_audio_source(&mut self, source: AudioCrossbarSource) {
        self.audio_source = source;
    }

    /// Determine the effective VFO slot that will be used for transmission.
    pub fn effective_tx_slot(&self) -> VfoSlot {
        match self.cross_band {
            CrossBandMode::Off => match self.dw_tx_policy {
                DualWatchTxPolicy::ActiveVfo => self.active_vfo,
                DualWatchTxPolicy::FollowCarrier => {
                    if (self.carrier_active || self.dwell.is_rx_held())
                        && self.dual_watch != DualWatchMode::Off
                    {
                        self.listening_vfo
                    } else {
                        self.active_vfo
                    }
                }
            },
            CrossBandMode::FixedTxOnA => VfoSlot::VfoA,
            CrossBandMode::FixedTxOnB => VfoSlot::VfoB,
            CrossBandMode::MasterFollowsRx => self.listening_vfo,
        }
    }

    pub fn effective_tx_channel(&self) -> &ChannelConfig {
        let slot = self.effective_tx_slot();
        &self.channels[slot.index().min(self.channels.len() - 1)]
    }

    // ── Application Arbitration & RF Resource Locking ──

    /// Pop the active lock and restore the preempted saved lock, if present.
    pub fn pop_and_restore_lock(&mut self) -> Option<VfoArbitrationLock> {
        let restored = self.arbitrator.pop_and_restore();
        if let Some(saved) = restored {
            self.listening_vfo = saved.slot;
            self.fsk_sync_active = saved.client.id == ClientId::FSK_SIDECAR;
        } else {
            self.fsk_sync_active = false;
        }
        self.dwell.reset_dwell();
        restored
    }

    /// Request fine-grained resource locks on behalf of an application or subsystem.
    pub fn request_resource_lock(
        &mut self,
        client: ClientToken,
        resources: RfResourceMask,
        slot: VfoSlot,
        timeout_ms: u16,
        mute_audio: bool,
    ) -> bool {
        if self.arbitrator.request_lock(
            client,
            resources,
            slot,
            timeout_ms,
            mute_audio,
            self.is_transmitting,
        ) {
            self.listening_vfo = slot;
            self.dwell.reset_dwell();
            true
        } else {
            false
        }
    }

    /// Request a standard VFO channel lock for an application.
    pub fn request_vfo_lock(
        &mut self,
        client: ClientToken,
        slot: VfoSlot,
        timeout_ms: u16,
        mute_audio: bool,
    ) -> bool {
        let res = RfResourceMask::for_vfo(slot)
            .union(RfResourceMask::BASEBAND_MODEM)
            .union(if mute_audio {
                RfResourceMask::AUDIO_OUTPUT
            } else {
                RfResourceMask::NONE
            });
        self.request_resource_lock(client, res, slot, timeout_ms, mute_audio)
    }

    /// Extend the active lock duration for the current holding client.
    pub fn extend_vfo_lock(&mut self, client: ClientToken, timeout_ms: u16) -> bool {
        self.arbitrator.extend_lock(client, timeout_ms)
    }

    /// Explicitly release an RF resource lock held by a client.
    pub fn release_vfo_lock(&mut self, client: ClientToken) -> bool {
        if self.arbitrator.release_lock(client) {
            if !self.arbitrator.is_locked() {
                self.fsk_sync_active = false;
            } else if let Some(slot) = self.arbitrator.locked_slot() {
                self.listening_vfo = slot;
                self.fsk_sync_active = self.arbitrator.active_client().id == ClientId::FSK_SIDECAR;
            }
            self.dwell.reset_dwell();
            true
        } else {
            false
        }
    }

    #[inline(never)]
    fn resync_after_unlock<R: RadioService>(&mut self, radio: &mut R, was_muted: bool) {
        self.dwell.reset_dwell();
        let locked = self.arbitrator.is_locked();
        if locked {
            self.listening_vfo = self.arbitrator.active_lock.slot;
            self.fsk_sync_active = self.arbitrator.active_client().id == ClientId::FSK_SIDECAR;
        } else {
            self.fsk_sync_active = false;
            if !self.is_transmitting && self.dual_watch == DualWatchMode::Off {
                self.listening_vfo = self.active_vfo;
            }
        }
        if !self.is_transmitting {
            radio.configure_channel(
                &self.channels[self.listening_vfo.index().min(self.channels.len() - 1)],
            );
            if locked || was_muted {
                self.set_audio_muted(radio, locked && self.arbitrator.active_lock.audio_muted);
            }
        }
    }

    /// Release any arbitrator lock owned by `client_id` and resynchronize hardware RX state.
    pub fn release_client_lock<R: RadioService>(
        &mut self,
        radio: &mut R,
        client_id: ClientId,
    ) -> bool {
        let was_active = self.arbitrator.active_lock.is_active()
            && self.arbitrator.active_lock.client.id == client_id;
        let was_muted = self.arbitrator.active_lock.audio_muted;
        if !self.arbitrator.release_client(client_id) {
            return false;
        }
        if was_active {
            if !self.arbitrator.is_locked() {
                self.listening_vfo = self.active_vfo;
            }
            self.resync_after_unlock(radio, was_muted);
        }
        true
    }

    /// Force-release all active locks (e.g. user pressed PTT or exit key).
    pub fn force_release_all_locks(&mut self) {
        self.arbitrator.force_release_all();
        self.fsk_sync_active = false;
        self.dwell.reset_dwell();
    }

    #[inline(always)]
    pub fn is_vfo_locked(&self) -> bool {
        self.arbitrator.is_locked()
    }

    #[inline(always)]
    pub fn lock_client(&self) -> ClientToken {
        self.arbitrator.active_client()
    }

    #[inline(always)]
    pub fn locked_slot(&self) -> Option<VfoSlot> {
        self.arbitrator.locked_slot()
    }

    /// Current arbitration lock depth (0 = none, 1 = active lock, 2 = active + stashed lock).
    #[inline(always)]
    pub fn lock_depth(&self) -> usize {
        self.arbitrator.lock_depth() as usize
    }

    /// Reference to the active lock descriptor, if active.
    #[inline(always)]
    pub fn active_lock(&self) -> Option<&VfoArbitrationLock> {
        if self.arbitrator.is_locked() {
            Some(&self.arbitrator.active_lock)
        } else {
            None
        }
    }

    /// Reference to the stashed preempted lock descriptor, if present.
    #[inline(always)]
    pub fn saved_lock(&self) -> Option<&VfoArbitrationLock> {
        self.arbitrator.saved_lock.as_ref()
    }

    /// Check if a specific RF hardware resource is currently locked.
    #[inline(always)]
    pub fn is_resource_locked(&self, res: RfResourceMask) -> bool {
        self.arbitrator.is_resource_locked(res, None)
    }

    // ── FSK Sidecar Event Handlers ──

    pub const FSK_CLIENT: ClientToken = ClientToken::new(ClientId::FSK_SIDECAR, LockTier::SERVICE);

    #[inline(never)]
    pub fn set_audio_muted<R: RfAudioPath>(&mut self, radio: &mut R, muted: bool) {
        if self.audio_path_muted != muted {
            self.audio_path_muted = muted;
            radio.mute_audio(muted);
        }
    }

    /// Notify coordinator that an FSK preamble/sync burst has been detected on the physical layer.
    pub fn notify_fsk_sync<R: RfAudioPath>(&mut self, radio: &mut R, slot: VfoSlot) -> RadioEvent {
        self.fsk_sync_active = true;
        let mute = self.fsk_config.mute_audio_on_sync;

        self.request_vfo_lock(Self::FSK_CLIENT, slot, self.fsk_config.sync_hold_ms, mute);

        if mute {
            self.set_audio_muted(radio, true);
        }

        RadioEvent::FskSyncDetected(slot)
    }

    /// Notify coordinator that an FSK frame has successfully concluded reception.
    pub fn notify_fsk_frame_finished<R: RfAudioPath>(
        &mut self,
        _radio: &mut R,
        slot: VfoSlot,
    ) -> RadioEvent {
        self.fsk_sync_active = false;
        if self.arbitrator.active_client().id == ClientId::FSK_SIDECAR {
            // Keep locked for post-frame response/ACK window
            let client = self.arbitrator.active_client();
            self.arbitrator
                .extend_lock(client, self.fsk_config.post_frame_hold_ms);
        }
        RadioEvent::FskFrameFinished(slot)
    }

    /// Abort FSK sidecar hold on error or timeout.
    pub fn notify_fsk_error<R: RfAudioPath>(&mut self, radio: &mut R) {
        if self.arbitrator.active_client().id == ClientId::FSK_SIDECAR {
            let client = self.arbitrator.active_client();
            self.release_vfo_lock(client);
            self.set_audio_muted(radio, false);
        }
    }

    // ── VFO Operations & UI Helpers ──

    /// Returns `true` if `slot` is currently driving the physical receiver hardware
    /// (not transmitting, and either unlocked or owned by the active lock on `slot`).
    #[inline(always)]
    pub fn is_slot_hardware_active(&self, slot: VfoSlot) -> bool {
        self.listening_vfo == slot
            && !self.is_transmitting
            && (!self.arbitrator.is_locked() || self.arbitrator.locked_slot() == Some(slot))
    }

    pub fn select_vfo<R: RfChannel + RfSquelch>(&mut self, radio: &mut R, slot: VfoSlot) {
        if slot.index() < self.channels.len() {
            self.active_vfo = slot;
            if self.dual_watch == DualWatchMode::Off
                && !self.is_transmitting
                && (!self.arbitrator.is_locked() || self.arbitrator.locked_slot() == Some(slot))
            {
                self.listening_vfo = slot;
                self.ste.reset();
                radio.configure_channel(&self.channels[slot.index()]);
            }
        }
    }

    pub fn toggle_vfo<R: RfChannel + RfSquelch>(&mut self, radio: &mut R) {
        let next_slot = match self.active_vfo {
            VfoSlot::VfoA => VfoSlot::VfoB,
            VfoSlot::VfoB => VfoSlot::VfoC,
            _ => VfoSlot::VfoA,
        };
        self.select_vfo(radio, next_slot);
    }

    pub fn toggle_dual_watch(&mut self) -> DualWatchMode {
        self.dual_watch = match self.dual_watch {
            DualWatchMode::Off => DualWatchMode::EqualDwell { dwell_ms: 300 },
            DualWatchMode::EqualDwell { .. } => DualWatchMode::TripleWatch { dwell_ms: 250 },
            DualWatchMode::TripleWatch { .. } => DualWatchMode::Adaptive,
            DualWatchMode::Adaptive | DualWatchMode::Priority { .. } => DualWatchMode::Off,
        };
        self.dwell.reset_dwell();
        self.dual_watch
    }

    pub fn toggle_cross_band(&mut self) -> CrossBandMode {
        self.cross_band = match self.cross_band {
            CrossBandMode::Off => CrossBandMode::FixedTxOnA,
            CrossBandMode::FixedTxOnA => CrossBandMode::FixedTxOnB,
            CrossBandMode::FixedTxOnB => CrossBandMode::MasterFollowsRx,
            CrossBandMode::MasterFollowsRx => CrossBandMode::Off,
        };
        self.cross_band
    }

    pub fn toggle_reverse<R: RfChannel + RfSubaudible>(&mut self, radio: &mut R, slot: VfoSlot) {
        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &mut self.channels[idx];
        ch.reverse = !ch.reverse;
        let rx_freq = ch.effective_rx_freq();
        let rx_sub = ch.effective_rx_subaudible();
        if self.is_slot_hardware_active(slot) {
            radio.set_rx_frequency(rx_freq);
            radio.set_rx_subaudible(rx_sub);
        }
    }

    /// Toggle talkaround (direct simplex transmission on RX frequency, bypassing repeater shift).
    pub fn toggle_talkaround<R: RfChannel + RfSubaudible>(
        &mut self,
        radio: &mut R,
        slot: VfoSlot,
    ) -> bool {
        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &mut self.channels[idx];
        let enabled = !ch.talkaround();
        ch.set_talkaround(enabled);
        let rx_freq = ch.effective_rx_freq();
        let rx_sub = ch.effective_rx_subaudible();
        if self.is_slot_hardware_active(slot) {
            radio.set_rx_frequency(rx_freq);
            radio.set_rx_subaudible(rx_sub);
        }
        enabled
    }

    /// Apply a sub-Hz Fractional-N Vernier frequency offset (in 0.1 Hz units) to a VFO slot.
    pub fn set_vfo_sub_hz_offset<R: RfFineTuning>(
        &mut self,
        radio: &mut R,
        slot: VfoSlot,
        offset_dhz: i16,
    ) {
        let idx = slot.index().min(self.channels.len() - 1);
        self.channels[idx].fine_offset_dhz = offset_dhz;
        let carrier_hz = self.channels[idx].effective_rx_freq().raw();
        if self.is_slot_hardware_active(slot) {
            radio.set_sub_hz_offset(carrier_hz, offset_dhz);
        }
    }

    /// Check granular transmission permission for a specific VFO slot and client token.
    #[inline(never)]
    pub fn check_tx_permission(&self, slot: VfoSlot, client: ClientToken) -> TxPermission {
        if self.is_transmitting && matches!(self.tx_state, TxState::Active | TxState::LeadIn { .. })
        {
            return TxPermission::BlockedResourceLocked;
        }

        // Hardware Safeguard: Lock out TX if battery pack is critically depleted
        let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
        if pwr.is_critical && !pwr.is_charging {
            return TxPermission::BlockedBatteryCritical;
        }

        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &self.channels[idx];
        let tx_inhibited =
            ch.tx_inhibit() || ch.rx_freq.raw() == 0 || ch.effective_tx_freq().raw() == 0;
        let carrier_active = ch.busy_lock && self.carrier_active && self.listening_vfo == slot;
        let tone_matched = ch.effective_rx_subaudible() != SubaudibleTone::None
            && self.subaudible_matched
            && self.listening_vfo == slot;
        let effective_bclo = if ch.tone_bclo() {
            BcloPolicy::ToneMismatchOnly
        } else {
            self.bclo_policy
        };

        self.arbitrator.evaluate_tx_permission(
            client,
            effective_bclo,
            carrier_active,
            tone_matched,
            tx_inhibited,
            self.tot_locked_out,
        )
    }

    /// Check whether transmission is allowed on the specified VFO slot.
    pub fn can_transmit_on(&self, slot: VfoSlot) -> bool {
        self.check_tx_permission(
            slot,
            ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND),
        )
        .is_granted()
    }

    /// Check whether transmission is allowed on the currently active VFO slot.
    #[inline(always)]
    pub fn can_transmit(&self) -> bool {
        self.can_transmit_on(self.effective_tx_slot())
    }

    /// Enable or disable manual Monitor (squelch defeat) on the listening VFO.
    ///
    /// When enabled, unmutes receiver audio regardless of carrier/subaudible thresholds,
    /// illuminates the green RX status LED, and pauses Dual Watch hopping on the monitored channel.
    #[inline(never)]
    pub fn set_monitor<R: RadioService>(&mut self, radio: &mut R, enabled: bool) {
        if self.is_transmitting && enabled {
            return;
        }
        self.monitor_active = enabled;
        if enabled {
            bsp::set_status_led(bsp::StatusLed::Green);
            self.set_audio_muted(radio, false);
        } else if !self.carrier_active {
            if !self.is_transmitting {
                bsp::set_status_led(bsp::StatusLed::Off);
            }
            self.set_audio_muted(radio, true);
        }
    }

    /// Toggle manual Monitor (squelch defeat) mode and return the new state.
    pub fn toggle_monitor<R: RadioService>(&mut self, radio: &mut R) -> bool {
        let next = !self.monitor_active;
        self.set_monitor(radio, next);
        self.monitor_active
    }

    /// Unified OS-level PTT state machine supporting `Momentary`, `Toggle`, and `DoublePressLatch`
    /// modes across all foreground and background applications.
    pub fn handle_ptt_key<R: RadioService>(
        &mut self,
        radio: &mut R,
        mode: PttMode,
        pressed: bool,
        now_ms: u32,
        client: ClientToken,
    ) -> PttEventOutcome {
        if self.ptt_latched && !self.is_transmitting {
            self.ptt_latched = false;
        }

        let should_start = match mode {
            PttMode::Momentary => Some(pressed),
            PttMode::Toggle => {
                if pressed {
                    if self.is_transmitting {
                        Some(false)
                    } else {
                        self.ptt_latched = true;
                        Some(true)
                    }
                } else {
                    None
                }
            }
            PttMode::DoublePressLatch => {
                if pressed {
                    let diff_ms = now_ms.wrapping_sub(self.last_ptt_press_ms);
                    self.last_ptt_press_ms = now_ms;
                    if self.ptt_latched {
                        Some(false)
                    } else {
                        if diff_ms <= 400 {
                            self.ptt_latched = true;
                        }
                        Some(true)
                    }
                } else if !self.ptt_latched {
                    Some(false)
                } else {
                    None
                }
            }
        };

        match should_start {
            Some(true) => {
                let perm = self.request_ptt_start(radio, client);
                if !perm.is_granted() {
                    self.ptt_latched = false;
                }
                PttEventOutcome::Started(perm)
            }
            Some(false) => {
                self.ptt_latched = false;
                if self.is_transmitting {
                    self.request_ptt_stop(radio);
                    PttEventOutcome::Stopped
                } else {
                    PttEventOutcome::None
                }
            }
            None => PttEventOutcome::None,
        }
    }

    /// Called when the physical PTT key is released. Clears TOT lockout penalty.
    pub fn on_ptt_released(&mut self) {
        self.tot_locked_out = false;
        self.tot_elapsed_10ms = 0;
        self.tot_warning_given = false;
    }

    pub fn start_tx<R: RadioService>(&mut self, radio: &mut R) {
        self.monitor_active = false;
        self.is_transmitting = true;
        let slot = self.effective_tx_slot();
        self.listening_vfo = slot;
        self.dwell.reset_dwell();
        let tx_ch = self.effective_tx_channel();
        let tx_subaudible = tx_ch.effective_tx_subaudible();
        let tx_freq = tx_ch.effective_tx_freq();
        let tx_power = tx_ch.power;
        let tx_mod = tx_ch.modulation;
        let tx_bw = tx_ch.bandwidth;

        radio.set_tx_subaudible(tx_subaudible);
        if tx_subaudible != SubaudibleTone::None {
            self.tx_state = TxState::LeadIn { remaining_ms: 50 };
            radio.mute_mic(true);
        } else {
            self.tx_state = TxState::Active;
            radio.mute_mic(false);
        }
        bsp::set_status_led(bsp::StatusLed::Red);
        self.tot_elapsed_10ms = 0;
        self.tot_warning_given = false;
        self.pa_thermal_foldback_active = false;
        radio.set_tx_frequency(tx_freq);

        // Kernel Power Governor: Dynamic Brownout Protection
        // When battery voltage is low, high-power TX (5W / ~2A) causes severe cell IR voltage sag
        // that can drop the pack below 5.5V and brownout the MCU. Throttle power level to Medium.
        let mut target_power = tx_power;
        let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
        if pwr.is_low && target_power == RfPowerLevel::High {
            target_power = RfPowerLevel::Medium;
        }

        radio.set_power_level(target_power);
        radio.set_modulation(tx_mod);
        radio.set_bandwidth(tx_bw);
        radio.tx_prepare();
        radio.tx_ramp_up(target_power as u8);
        radio.tx_unmute();
    }

    pub fn stop_tx<R: RadioService>(&mut self, radio: &mut R) {
        radio.send_css_tail(CssTailMode::None);
        radio.mute_mic(false);
        radio.tx_mute();
        radio.tx_ramp_down();
        radio.tx_conclude();
        if let AudioRoutingState::ToneBurstActive { .. } = self.audio_routing {
            self.audio_routing = AudioRoutingState::NormalVoice;
        }
        self.is_transmitting = false;
        self.ptt_latched = false;
        self.tx_state = TxState::Idle;
        self.pa_thermal_foldback_active = false;
        bsp::set_status_led(if self.carrier_active || self.monitor_active {
            bsp::StatusLed::Green
        } else {
            bsp::StatusLed::Off
        });
        self.tot_elapsed_10ms = 0;
        self.tot_warning_given = false;
        self.dwell.on_tx_finished();
        if let Some(locked_slot) = self.arbitrator.locked_slot() {
            self.listening_vfo = locked_slot;
        } else if matches!(
            self.cross_band,
            CrossBandMode::FixedTxOnA | CrossBandMode::FixedTxOnB
        ) {
            self.listening_vfo = self.active_vfo;
        }
        let rx_ch = self.listening_channel();
        let freq = rx_ch.effective_rx_freq();
        let pwr = rx_ch.power;
        let mod_mode = rx_ch.modulation;
        let bw = rx_ch.bandwidth;
        let rx_subaudible = rx_ch.effective_rx_subaudible();
        radio.set_rx_frequency(freq);
        radio.set_power_level(pwr);
        radio.set_modulation(mod_mode);
        radio.set_bandwidth(bw);
        radio.set_rx_subaudible(rx_subaudible);
    }

    /// High-level request to initiate PTT transmission on behalf of user or client.
    pub fn request_ptt_start<R: RadioService>(
        &mut self,
        radio: &mut R,
        client: ClientToken,
    ) -> TxPermission {
        let slot = self.effective_tx_slot();
        let perm = self.check_tx_permission(slot, client);
        if !perm.is_granted() {
            return perm;
        }

        if let TxState::TailBurst { .. } = self.tx_state {
            self.tx_state = TxState::Active;
            radio.mute_mic(false);
            return TxPermission::Granted;
        }

        self.start_tx(radio);
        TxPermission::Granted
    }

    /// High-level request to conclude PTT transmission with tail elimination support.
    pub fn request_ptt_stop<R: RadioService>(&mut self, radio: &mut R) {
        self.on_ptt_released();

        if let AudioRoutingState::ToneBurstActive { .. } = self.audio_routing {
            radio.mute_tones();
            self.audio_routing = AudioRoutingState::NormalVoice;
        }

        if !self.is_transmitting {
            return;
        }

        let tx_ch = self.effective_tx_channel();
        let ste_mode = tx_ch.ste_mode;
        if tx_ch.effective_tx_subaudible().is_ctcss() && ste_mode != CssTailMode::None {
            radio.send_css_tail(ste_mode);
            radio.mute_mic(true);
            self.tx_state = TxState::TailBurst { remaining_ms: 150 };
        } else {
            self.stop_tx(radio);
        }
    }

    /// Check if Busy Channel Lockout is actively blocking transmission on slot.
    #[inline(always)]
    pub fn is_bclo_blocked(&self, slot: VfoSlot) -> bool {
        matches!(
            self.check_tx_permission(
                slot,
                ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND)
            ),
            TxPermission::BlockedBusyChannel
        )
    }

    /// Query channel telemetry for display UI widgets and background services.
    pub fn channel_telemetry(&self, slot: VfoSlot) -> ChannelTelemetry {
        let idx = slot.index().min(self.channels.len() - 1);
        let is_locked = self.arbitrator.locked_slot() == Some(slot);
        ChannelTelemetry {
            slot,
            rssi_dbm: self.rssi_moving_avg[idx],
            activity_count: self.activity_count[idx],
            is_carrier_open: self.carrier_active && self.listening_vfo == slot,
            is_locked,
            lock_client: if is_locked {
                self.arbitrator.active_client().id
            } else {
                ClientId::NONE
            },
        }
    }

    #[inline(always)]
    pub fn is_slot_tx_active(&self, slot: VfoSlot) -> bool {
        self.is_transmitting && self.effective_tx_slot() == slot
    }

    #[inline(always)]
    pub fn is_slot_rx_active(&self, slot: VfoSlot) -> bool {
        (self.listening_vfo == slot && self.carrier_active)
            || self.arbitrator.locked_slot() == Some(slot)
    }

    /// Query the complete display activity state of a VFO.
    pub fn vfo_activity_state(&self, slot: VfoSlot) -> VfoActivityState {
        if self.is_transmitting
            && self.effective_tx_channel().rx_freq == self.channels[slot.index()].rx_freq
        {
            VfoActivityState::Transmitting
        } else if self.arbitrator.locked_slot() == Some(slot) {
            VfoActivityState::FskLocked
        } else if self.carrier_active && self.listening_vfo == slot {
            VfoActivityState::CarrierActive
        } else if self.listening_vfo == slot {
            VfoActivityState::Listening
        } else {
            VfoActivityState::Idle
        }
    }

    #[inline(always)]
    pub fn vfo_rssi(&self, slot: VfoSlot) -> i16 {
        self.rssi_moving_avg[slot.index().min(self.rssi_moving_avg.len() - 1)]
    }

    #[inline(always)]
    pub fn vfo_channel(&self, slot: VfoSlot) -> &ChannelConfig {
        &self.channels[slot.index().min(self.channels.len() - 1)]
    }

    #[inline(always)]
    pub fn vfo_channel_mut(&mut self, slot: VfoSlot) -> &mut ChannelConfig {
        let idx = slot.index().min(self.channels.len() - 1);
        &mut self.channels[idx]
    }

    /// Adjust VFO frequency by step in Hertz (supporting positive and negative steps).
    /// Respects per-channel `freq_lock` (`FLAG_FREQ_LOCK`).
    pub fn step_vfo_freq<R: RfChannel>(&mut self, radio: &mut R, slot: VfoSlot, step_hz: i32) {
        let idx = slot.index().min(self.channels.len() - 1);
        if self.channels[idx].freq_lock() {
            return;
        }
        let cur = self.channels[idx].rx_freq.raw() as i64;
        let next = (cur + step_hz as i64).clamp(18_000_000, 1_300_000_000) as u32;
        self.channels[idx].rx_freq = utils::units::Frequency::new(next);
        let eff_rx = self.channels[idx].effective_rx_freq();
        if self.is_slot_hardware_active(slot) {
            radio.set_rx_frequency(eff_rx);
        }
    }

    #[inline(always)]
    pub fn dwell_timer_ms(&self) -> u16 {
        self.dwell.dwell_timer_ms
    }

    #[inline(always)]
    pub fn rx_hold_timer_ms(&self) -> u16 {
        self.dwell.rx_hold_timer_ms
    }

    // ── 1750 Hz Repeater Access Tone Burst ──

    /// Trigger a 1750 Hz tone burst for European/international repeater access.
    #[inline(never)]
    pub fn trigger_1750hz_burst<R: RadioService>(
        &mut self,
        radio: &mut R,
        duration_ms: u16,
    ) -> bool {
        let slot = self.effective_tx_slot();
        let perm = self.check_tx_permission(
            slot,
            ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND),
        );
        if !perm.is_granted() {
            return false;
        }
        if !self.is_transmitting {
            self.start_tx(radio);
        }
        radio.mute_mic(true);
        radio.set_tone1(1750, 100);
        self.audio_routing = AudioRoutingState::ToneBurstActive {
            remaining_ms: duration_ms.max(10),
        };
        true
    }

    /// Cancel active 1750 Hz tone burst and silence tone generator.
    pub fn stop_1750hz_burst<R: RadioService>(&mut self, radio: &mut R) {
        if let AudioRoutingState::ToneBurstActive { .. } = self.audio_routing {
            radio.mute_tones();
            self.audio_routing = AudioRoutingState::NormalVoice;
            if self.is_transmitting && self.tx_state == TxState::Active {
                radio.mute_mic(false);
            }
        }
    }

    fn load_record_into_channel(ch: &mut ChannelConfig, rec: &crate::data::ChannelRecord) {
    use crate::data::ChannelRecord;
    ch.rx_freq = Frequency::<Hertz>::new(rec.rx_frequency_hz);
    ch.tx_shift = if rec.tx_frequency_hz != 0 && rec.tx_frequency_hz != rec.rx_frequency_hz {
        let rx = rec.rx_frequency_hz;
        let tx = rec.tx_frequency_hz;
        if tx >= rx {
            RepeaterShift::Positive(Frequency::<Hertz>::new(tx - rx))
        } else {
            RepeaterShift::Negative(Frequency::<Hertz>::new(rx - tx))
        }
    } else {
        RepeaterShift::Simplex
    };
    ch.step = StepSetting::from_hz_x10(rec.step_hz_x10);
    ch.bandwidth =
        if rec.bandwidth_50hz >= 400 || (rec.flags & ChannelRecord::FLAG_WIDE_FILTER) != 0 {
            Bandwidth::Wide25k
        } else if rec.bandwidth_50hz > 0 && rec.bandwidth_50hz <= 150 {
            Bandwidth::Narrow6k25
        } else {
            Bandwidth::Narrow12k5
        };
    ch.modulation = ModulationMode::from_u8(rec.modulation);
    ch.power = if rec.power_mw == 0 || rec.power_mw >= 4000 {
        RfPowerLevel::High
    } else if rec.power_mw >= 1500 {
        RfPowerLevel::Medium
    } else {
        RfPowerLevel::Low
    };
    ch.rx_subaudible = rec.rx_subaudible_tone();
    ch.tx_subaudible = rec.tx_subaudible_tone();
    ch.squelch_level = rec.squelch_level;
    ch.flags = rec.flags;
    ch.reverse = (rec.flags & ChannelRecord::FLAG_REVERSE) != 0;
    ch.busy_lock = (rec.flags & ChannelRecord::FLAG_BUSY_LOCKOUT) != 0;
    ch.compander = CompanderMode::from_u8(rec.compand);
    ch.scrambler = if rec.scram_mode == 0 {
        ScramblerMode::Off
    } else {
        ScramblerMode::InvertFrequency(rec.scram_param.max(rec.scram_mode))
    };
    ch.ste_mode = CssTailMode::from_u8(rec.ste);
    ch.mic_gain_db = rec.mic_gain;
    ch.roger_id = rec.roger;
    ch.tot_limit_15s = rec.tx_limit;
    ch.proto = rec.proto;
    ch.proto_ref = rec.proto_ref;
}

    fn save_channel_to_record(ch: &ChannelConfig, rec: &mut crate::data::ChannelRecord) {
    use crate::data::ChannelRecord;
    const BW_50HZ: [u16; 3] = [500, 250, 125];
    const PWR_MW: [u16; 3] = [500, 2000, 5000];

    rec.rx_frequency_hz = ch.rx_freq.raw();
    rec.tx_frequency_hz = match ch.tx_shift {
        RepeaterShift::Simplex => 0,
        RepeaterShift::Positive(off) => ch.rx_freq.raw().saturating_add(off.raw()),
        RepeaterShift::Negative(off) => ch.rx_freq.raw().saturating_sub(off.raw()),
    };
    rec.step_hz_x10 = ch.step.in_hz_x10();
    rec.bandwidth_50hz = BW_50HZ[ch.bandwidth as usize];
    rec.modulation = ch.modulation.to_u8();
    rec.power_mw = PWR_MW[ch.power as usize];
    rec.set_rx_subaudible_tone(ch.rx_subaudible);
    rec.set_tx_subaudible_tone(ch.tx_subaudible);
    rec.squelch_level = ch.squelch_level;

    let mut flags = ch.flags & !(ChannelRecord::FLAG_REVERSE | ChannelRecord::FLAG_BUSY_LOCKOUT);
    if ch.reverse {
        flags |= ChannelRecord::FLAG_REVERSE;
    }
    if ch.busy_lock {
        flags |= ChannelRecord::FLAG_BUSY_LOCKOUT;
    }
    rec.flags = flags;
    rec.compand = ch.compander as u8;
    match ch.scrambler {
        ScramblerMode::Off => {
            rec.scram_mode = 0;
            rec.scram_param = 0;
        }
        ScramblerMode::InvertFrequency(code) => {
            rec.scram_mode = 1;
            rec.scram_param = code;
        }
    }
    rec.ste = ch.ste_mode as u8;
    rec.mic_gain = ch.mic_gain_db;
    rec.roger = ch.roger_id;
    rec.tx_limit = ch.tot_limit_15s;
    rec.proto = ch.proto;
    rec.proto_ref = ch.proto_ref;
    rec.record_crc = rec.compute_crc();
}

    // ── Memory Channel Loading & Serialization ──

    /// Load a ChannelRecord from codeplug/KernFS storage into a target VFO slot.
    pub fn load_channel_into_vfo<R: RfChannel + RfSquelch>(
        &mut self,
        radio: &mut R,
        slot: VfoSlot,
        memory_idx: Option<u16>,
        rec: &crate::data::ChannelRecord,
    ) {
        let should_apply = self.is_slot_hardware_active(slot);
        self.populate_channel_config(slot, memory_idx, rec);
        if should_apply {
            let idx = slot.index().min(self.channels.len() - 1);
            radio.configure_channel(&self.channels[idx]);
        }
    }

    fn populate_channel_config(
        &mut self,
        slot: VfoSlot,
        memory_idx: Option<u16>,
        rec: &crate::data::ChannelRecord,
    ) {
        let idx = slot.index().min(self.channels.len() - 1);
        Self::load_record_into_channel(&mut self.channels[idx], rec);
        self.vfo_memory_index[idx] = memory_idx;
    }

    /// Serialize current VFO tuning parameters into a ChannelRecord with zero attribute loss.
    pub fn save_vfo_to_channel(&self, slot: VfoSlot, rec: &mut crate::data::ChannelRecord) {
        let idx = slot.index().min(self.channels.len() - 1);
        Self::save_channel_to_record(&self.channels[idx], rec);
    }

    /// Apply a comprehensive, generic ModulationProfile (modulation, bandwidth, AF response,
    /// AGC mode, audio routing) to the radio hardware for a specific VFO slot.
    pub fn apply_modulation_profile<R: RadioService>(
        &mut self,
        radio: &mut R,
        slot: VfoSlot,
        profile: &ModulationProfile,
    ) {
        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &mut self.channels[idx];
        ch.modulation = profile.mode;
        ch.bandwidth = profile.default_bandwidth;

        if self.is_slot_hardware_active(slot) {
            kernel::service::radio::RfModulation::set_modulation(radio, profile.mode);
            kernel::service::radio::RfModulation::set_bandwidth(radio, profile.default_bandwidth);
            radio.set_af_response(&profile.af_response);
            radio.set_agc_mode(profile.agc_mode, false);
            radio.set_agc_speed(profile.agc_speed);
            match profile.audio_routing {
                AudioRoutingPolicy::Muted => radio.mute_audio(true),
                AudioRoutingPolicy::Speaker => radio.mute_audio(false),
                _ => {}
            }
        }
    }

    #[inline(always)]
    fn next_watch_vfo(&self) -> VfoSlot {
        match self.dual_watch {
            DualWatchMode::TripleWatch { .. } => match self.listening_vfo {
                VfoSlot::VfoA => VfoSlot::VfoB,
                VfoSlot::VfoB => VfoSlot::VfoC,
                _ => VfoSlot::VfoA,
            },
            _ => self.listening_vfo.other(),
        }
    }

    #[inline(never)]
    fn tick_tx<R: RadioService>(&mut self, radio: &mut R) -> Option<RadioEvent> {
        // Lead-in countdown before unmuting mic
        if let TxState::LeadIn { remaining_ms } = self.tx_state {
            if remaining_ms <= 10 {
                self.tx_state = TxState::Active;
                radio.mute_mic(false);
            } else {
                self.tx_state = TxState::LeadIn {
                    remaining_ms: remaining_ms.saturating_sub(10),
                };
            }
        }

        // Active transmission state machine
        if self.is_transmitting && matches!(self.tx_state, TxState::Active | TxState::LeadIn { .. })
        {
            self.tot_elapsed_10ms = self.tot_elapsed_10ms.saturating_add(1);

            // Silicon Thermal & Supply Protection (checked every 320 ms, bitmask avoids M0+ div):
            // - Emergency cutoff: die >= 85.0°C or VDDA < 2.85V -> abort TX immediately.
            // - Stage 2 foldback: die >= 78.0°C or VDDA < 2.95V -> drop PA to Low (level 0).
            // - Stage 1 foldback: die >= 65.0°C, low battery, or VDDA < 3.05V -> drop High to Med (1).
            if (self.tot_elapsed_10ms & 31) == 0 {
                let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
                let vdda_valid = pwr.vdda_mv >= 1800;
                if pwr.mcu_temp_deci_c >= 850 || (vdda_valid && pwr.vdda_mv < 2850) {
                    self.pa_thermal_foldback_active = true;
                    self.stop_tx(radio);
                    return Some(RadioEvent::TotExpired);
                } else if pwr.mcu_temp_deci_c >= 780 || (vdda_valid && pwr.vdda_mv < 2950) {
                    if !self.pa_thermal_foldback_active
                        || self.effective_tx_channel().power != RfPowerLevel::Low
                    {
                        self.pa_thermal_foldback_active = true;
                        radio.tx_ramp_up(0);
                    }
                } else if !self.pa_thermal_foldback_active
                    && (pwr.mcu_temp_deci_c >= 650
                        || pwr.is_low
                        || (vdda_valid && pwr.vdda_mv < 3050))
                    && self.effective_tx_channel().power == RfPowerLevel::High
                {
                    self.pa_thermal_foldback_active = true;
                    radio.tx_ramp_up(1);
                }
            }

            if self.tot_limit_seconds > 0 {
                let limit_ticks = (self.tot_limit_seconds as u32) * 100;
                let elapsed = self.tot_elapsed_10ms as u32;
                if elapsed >= limit_ticks {
                    self.stop_tx(radio);
                    self.tot_locked_out = true; // Lockout penalty until physical PTT release
                    return Some(RadioEvent::TotExpired);
                } else {
                    let lead = (self
                        .tot_warning_lead_seconds
                        .min((self.tot_limit_seconds >> 1) as u8)
                        as u32)
                        * 100;
                    if limit_ticks > lead
                        && elapsed == limit_ticks - lead
                        && !self.tot_warning_given
                    {
                        self.tot_warning_given = true;
                        return Some(RadioEvent::TotWarning {
                            remaining_sec: self.tot_warning_lead_seconds,
                        });
                    }
                }
            }
            return Some(RadioEvent::None);
        }

        // Subaudible tail burst countdown
        if let TxState::TailBurst { remaining_ms } = self.tx_state {
            if remaining_ms <= 10 {
                self.stop_tx(radio);
                return Some(RadioEvent::TxEnded);
            } else {
                self.tx_state = TxState::TailBurst {
                    remaining_ms: remaining_ms.saturating_sub(10),
                };
                return Some(RadioEvent::None);
            }
        }

        None
    }

    // ── Main 10ms System Tick ──

    #[inline(never)]
    fn calc_watch_target(&self, idx: usize) -> Option<(u16, VfoSlot)> {
        match self.dual_watch {
            DualWatchMode::Off => None,
            DualWatchMode::EqualDwell { dwell_ms } | DualWatchMode::TripleWatch { dwell_ms } => {
                Some((dwell_ms, self.next_watch_vfo()))
            }
            DualWatchMode::Priority {
                priority_vfo,
                check_interval_ms,
            } => {
                let target = if self.listening_vfo != priority_vfo {
                    (check_interval_ms, priority_vfo)
                } else {
                    (100, priority_vfo.other())
                };
                Some(target)
            }
            DualWatchMode::Adaptive => Some((
                DwellController::calc_adaptive_dwell(
                    self.rssi_moving_avg[idx],
                    self.activity_count[idx],
                ),
                self.next_watch_vfo(),
            )),
        }
    }

    #[inline(never)]
    fn tick_tone_burst<R: RadioService>(&mut self, radio: &mut R) {
        if let AudioRoutingState::ToneBurstActive { remaining_ms } = self.audio_routing {
            if remaining_ms <= 10 {
                radio.mute_tones();
                self.audio_routing = AudioRoutingState::NormalVoice;
                if self.is_transmitting && self.tx_state == TxState::Active {
                    radio.mute_mic(false);
                }
            } else {
                self.audio_routing = AudioRoutingState::ToneBurstActive {
                    remaining_ms: remaining_ms.saturating_sub(10),
                };
            }
        }
    }

    #[inline(never)]
    fn tick_arbitrator<R: RadioService>(&mut self, radio: &mut R) -> Option<RadioEvent> {
        let arb_ev = self.arbitrator.tick(10)?;
        match arb_ev {
            ArbitrationEvent::LockRestored {
                client,
                resources,
                slot,
                needs_mute: _,
            } => {
                self.resync_after_unlock(radio, false);
                Some(RadioEvent::LockRestored {
                    client,
                    resources,
                    slot,
                })
            }
            ArbitrationEvent::LockReleased {
                client,
                resources,
                slot,
                was_muted,
            } => {
                self.resync_after_unlock(radio, was_muted);
                if client.id == ClientId::FSK_SIDECAR {
                    Some(RadioEvent::FskLockReleased(slot))
                } else {
                    Some(RadioEvent::LockReleased {
                        client,
                        resources,
                        slot,
                    })
                }
            }
        }
    }

    #[inline(never)]
    fn process_carrier_active<R: RadioService>(&mut self, radio: &mut R, idx: usize) -> RadioEvent {
        self.activity_count[idx] = self.activity_count[idx].saturating_add(1);

        // Voice carrier priority override: if squelch opens while FSK sidecar is holding the channel,
        // immediately restore audio so the user hears voice
        if self.fsk_config.voice_priority_override
            && self.arbitrator.active_client().id == ClientId::FSK_SIDECAR
        {
            self.set_audio_muted(radio, false);
        }

        if !self.carrier_active {
            self.carrier_active = true;
            if !self.is_transmitting {
                bsp::set_status_led(bsp::StatusLed::Green);
            }
            if !self.arbitrator.is_locked() || !self.arbitrator.active_lock.audio_muted {
                self.set_audio_muted(radio, false);
            }
            self.dwell.on_carrier_opened();
            self.subaudible_matched = false;
            self.ste.reset();
            RadioEvent::CarrierOpened(self.listening_vfo)
        } else {
            self.dwell.on_carrier_refresh();

            let tone_cfg = self.listening_channel().rx_subaudible != SubaudibleTone::None;
            let tone_present = if tone_cfg {
                radio.is_subaudible_matched()
            } else {
                false
            };
            self.subaudible_matched = tone_present;
            let tail_burst = radio.has_tail_burst_detected();
            if !self.monitor_active {
                if let Some(should_mute) =
                    self.ste
                        .process_tick_with_tail(true, tone_cfg, tone_present, tail_burst)
                {
                    self.set_audio_muted(radio, should_mute);
                }
            }

            RadioEvent::None
        }
    }

    #[inline(never)]
    pub fn tick_10ms<R: RadioService>(&mut self, radio: &mut R) -> RadioEvent {
        if let Some(ev) = self.tick_tx(radio) {
            return ev;
        }

        // 0. Process active 1750 Hz tone burst countdown
        self.tick_tone_burst(radio);

        // 1. Process active arbitration lock countdown
        if let Some(ev) = self.tick_arbitrator(radio) {
            return ev;
        }

        // 2. Telemetry and Squelch / Carrier detection
        let carrier = radio.is_carrier_open();
        let current_rssi = radio.get_rssi_dbm();
        let idx = self
            .listening_vfo
            .index()
            .min(self.rssi_moving_avg.len() - 1);
        self.rssi_moving_avg[idx] =
            (((self.rssi_moving_avg[idx] as i32 * 3) + current_rssi as i32) >> 2) as i16;

        // Dynamic AM Linearity Mapping: adjust front-end gain inversely to carrier RSSI in AM mode
        let listening_mod = self.channels[idx].modulation;
        if matches!(listening_mod, ModulationMode::Am | ModulationMode::AmB) {
            radio.compensate_am_linearity(current_rssi);
        }

        if carrier {
            return self.process_carrier_active(radio, idx);
        }

        // Background Adaptive Squelch Noise-Floor Estimator (runs when channel is idle)
        if self.adaptive_squelch_enabled {
            self.adaptive_tick_div = self.adaptive_tick_div.wrapping_add(1);
            if (self.adaptive_tick_div & 0x0F) == 0 {
                let metrics = SignalMetrics {
                    rssi_dbm: current_rssi,
                    snr: 0,
                    noise: radio.get_noise(),
                    glitch: radio.get_glitch(),
                    carrier_open: false,
                };
                self.adaptive_squelch[idx].update(&metrics);
            }
        }

        if self.carrier_active {
            self.carrier_active = false;
            self.subaudible_matched = false;
            self.ste.reset();
            if !self.monitor_active {
                if !self.is_transmitting {
                    bsp::set_status_led(bsp::StatusLed::Off);
                }
                self.set_audio_muted(radio, true);
            }
            return RadioEvent::CarrierClosed(self.listening_vfo);
        }

        // 3. Post-carrier hold
        if self.dwell.is_rx_held() {
            self.dwell.tick_10ms();
            return RadioEvent::None;
        }

        // 4. Freeze Dual Watch dwelling if VFO is held by an application, FSK sidecar, or Monitor mode
        if self.arbitrator.is_locked() || self.monitor_active {
            return RadioEvent::None;
        }

        // 5. Dual Watch / Triple Watch time-division multiplexing
        let Some((dwell_ms, next_vfo)) = self.calc_watch_target(idx) else {
            return RadioEvent::None;
        };

        if self.dwell.is_dwell_expired(dwell_ms) {
            self.dwell.reset_dwell();
            self.listening_vfo = next_vfo;
            self.ste.reset();
            radio.configure_channel(&self.channels[next_vfo.index()]);
            RadioEvent::DualWatchSwitched(next_vfo)
        } else {
            self.dwell.tick_10ms();
            RadioEvent::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRadio {
        configured_freqs: [u32; 8],
        configured_count: usize,
        rx_freq: u32,
        tx_freq: u32,
        carrier_open: bool,
        muted: bool,
        mic_muted: bool,
        tx_state: TransceiverTxState,
        tail_sent: CssTailMode,
        bandwidth: Bandwidth,
        rx_subaudible: SubaudibleTone,
        tone_matched: bool,
    }

    impl MockRadio {
        fn new() -> Self {
            Self {
                configured_freqs: [0; 8],
                configured_count: 0,
                rx_freq: 0,
                tx_freq: 0,
                carrier_open: false,
                muted: false,
                mic_muted: false,
                tx_state: TransceiverTxState::IdleRx,
                tail_sent: CssTailMode::None,
                bandwidth: Bandwidth::Narrow12k5,
                rx_subaudible: SubaudibleTone::None,
                tone_matched: false,
            }
        }
    }

    impl RfChannel for MockRadio {
        fn configure_channel(&mut self, config: &ChannelConfig) {
            self.rx_freq = config.effective_rx_freq().raw();
            self.bandwidth = config.bandwidth;
            self.rx_subaudible = config.rx_subaudible;
            if self.configured_count < self.configured_freqs.len() {
                self.configured_freqs[self.configured_count] = self.rx_freq;
                self.configured_count += 1;
            }
        }
        fn set_rx_frequency(&mut self, freq: Frequency<Hertz>) {
            self.rx_freq = freq.raw();
        }
        fn set_tx_frequency(&mut self, freq: Frequency<Hertz>) {
            self.tx_freq = freq.raw();
        }
        fn set_power_level(&mut self, _level: RfPowerLevel) {}
    }

    impl RfFineTuning for MockRadio {
        fn set_crystal_trim(&mut self, _trim: i16) {}
        fn set_pll_offset_10hz(&mut self, _offset_10hz: i16) {}
        fn get_crystal_trim(&mut self) -> i16 {
            0
        }
        fn set_sub_hz_offset(&mut self, _carrier_hz: u32, _offset_dhz: i16) {}
    }

    impl RfModulation for MockRadio {
        fn set_modulation(&mut self, _mode: ModulationMode) {}
        fn set_bandwidth(&mut self, bw: Bandwidth) {
            self.bandwidth = bw;
        }
        fn set_deviation(&mut self, _dev: DeviationLimit) {}
        fn set_compander(&mut self, _compander: CompanderMode) {}
        fn set_scrambler(&mut self, _scrambler: ScramblerMode) {}
    }

    impl RfSquelch for MockRadio {
        fn set_squelch_level(&mut self, _level: u8) {}
        fn set_squelch_thresholds(&mut self, _thresholds: &SquelchThresholds) {}
        fn get_signal_metrics(&mut self) -> SignalMetrics {
            SignalMetrics {
                rssi_dbm: -110,
                snr: 0,
                noise: 0,
                glitch: 0,
                carrier_open: self.carrier_open,
            }
        }
        fn is_carrier_open(&mut self) -> bool {
            self.carrier_open
        }
        fn get_rssi_dbm(&mut self) -> i16 {
            -110
        }
        fn get_snr(&mut self) -> u8 {
            0
        }
        fn get_glitch(&mut self) -> u8 {
            0
        }
        fn get_noise(&mut self) -> u8 {
            0
        }
    }

    impl RfSubaudible for MockRadio {
        fn set_rx_subaudible(&mut self, tone: SubaudibleTone) {
            self.rx_subaudible = tone;
        }
        fn set_tx_subaudible(&mut self, _tone: SubaudibleTone) {}
        fn is_subaudible_matched(&mut self) -> bool {
            self.tone_matched
        }
        fn send_css_tail(&mut self, mode: CssTailMode) {
            self.tail_sent = mode;
        }
    }

    impl RfFskPhy for MockRadio {
        fn fsk_configure_phy(&mut self, _config: &FskPhyConfig) -> Result<(), ()> {
            Ok(())
        }
        fn fsk_enable(&mut self, _enable: bool) {}
        fn fsk_enable_rx(&mut self, _enable: bool) {}
        fn fsk_enable_tx(&mut self, _enable: bool) {}
        fn fsk_clear_rx_fifo(&mut self) {}
        fn fsk_clear_tx_fifo(&mut self) {}
        fn fsk_read_status(&mut self) -> FskPhyStatus {
            FskPhyStatus::default()
        }
        fn fsk_read_fifo(&mut self, _out_words: &mut [u16]) -> usize {
            0
        }
        fn fsk_write_fifo(&mut self, _words: &[u16]) -> usize {
            0
        }
        fn fsk_set_tone_gain(&mut self, _gain: u8) {}
        fn fsk_set_baud_divider(&mut self, _divider: u16) {}
        fn fsk_set_deviation(&mut self, _deviation_hz: u16) {}
        fn fsk_rearm_rx(&mut self) {}
        fn send_fsk_packet_with_timeout(
            &mut self,
            _words: &[u16],
            _timeout_ms: u16,
        ) -> Result<(), ()> {
            Ok(())
        }
    }

    impl RfToneGenerator for MockRadio {
        fn set_tone1_raw(&mut self, _freq_word: u16, _gain: u8, _enabled: bool) {}
        fn set_tone2_raw(&mut self, _freq_word: u16, _gain: u8, _enabled: bool) {}
        fn set_tone1_freq_word(&mut self, _freq_word: u16) {}
        fn set_tone2_freq_word(&mut self, _freq_word: u16) {}
        fn set_tone1_gain(&mut self, _gain: u8) {}
        fn set_tone2_gain(&mut self, _gain: u8) {}
        fn enable_tone1(&mut self, _enable: bool) {}
        fn enable_tone2(&mut self, _enable: bool) {}
        fn mute_tones(&mut self) {}
    }

    impl RfAudioPath for MockRadio {
        fn set_af_response(&mut self, _response: &AfResponse) {}
        fn set_mic_gain(&mut self, _gain_db: u8) {}
        fn set_rf_volume(&mut self, _level: u8) {}
        fn set_tone_gain(&mut self, _gain: u8) {}
        fn play_tone(&mut self, _freq_hz: u16, _duration_ms: u16) {}
        fn play_tone_sequence(&mut self, _notes: &[ToneNote]) {}
        fn play_tone_sequence_with_gain(&mut self, _notes: &[ToneNote], _gain: u8) {}
        fn mute_audio(&mut self, mute: bool) {
            self.muted = mute;
        }
        fn mute_mic(&mut self, mute: bool) {
            self.mic_muted = mute;
        }
    }

    impl RfAgc for MockRadio {
        fn set_agc_table(&mut self, _table: &[u16; 5]) {}
        fn set_agc_mode(&mut self, _mode: AgcListeningMode, _disable: bool) {}
        fn set_agc_speed(&mut self, _speed: AgcSpeed) {}
    }

    impl RfPowerSequencer for MockRadio {
        fn tx_prepare(&mut self) {
            self.tx_state = TransceiverTxState::ActiveTransmit;
        }
        fn tx_ramp_up(&mut self, _target_power_level: u8) {}
        fn tx_unmute(&mut self) {}
        fn tx_mute(&mut self) {}
        fn tx_ramp_down(&mut self) {}
        fn tx_conclude(&mut self) {
            self.tx_state = TransceiverTxState::IdleRx;
        }
        fn tx_state(&self) -> TransceiverTxState {
            self.tx_state
        }
    }

    impl RadioService for MockRadio {
        fn sleep(&mut self) {}
        fn wake(&mut self) {}
        fn set_xtal_freq_trim(&mut self, _trim: i16) {}
    }

    #[test]
    fn test_coordinator_lock_restoration_reconfigures_radio_and_dual_watch() {
        let mut coord = RadioCoordinator::new();
        let mut radio = MockRadio::new();
        coord.channels[0].rx_freq = Frequency::new(145_500_000);
        coord.channels[1].rx_freq = Frequency::new(433_500_000);
        coord.active_vfo = VfoSlot::VfoA;
        coord.listening_vfo = VfoSlot::VfoA;
        coord.dual_watch = DualWatchMode::Off;

        let fsk = ClientToken::new(ClientId::FSK_SIDECAR, LockTier::SERVICE);
        let ui = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);
        assert!(coord.request_vfo_lock(fsk, VfoSlot::VfoA, 100, true));
        assert!(coord.request_vfo_lock(ui, VfoSlot::VfoB, 20, false));
        coord.set_audio_muted(&mut radio, false);
        radio.configure_channel(&coord.channels[1]);
        assert_eq!(radio.rx_freq, 433_500_000);

        // UI lock expires -> FSK lock restored AND hardware retuned back to VFO A.
        let _ = coord.tick_10ms(&mut radio);
        let ev = coord.tick_10ms(&mut radio);
        assert!(matches!(
            ev,
            RadioEvent::LockRestored {
                slot: VfoSlot::VfoA,
                ..
            }
        ));
        assert_eq!(radio.rx_freq, 145_500_000);
        assert!(radio.muted);

        // Release FSK -> hardware stays cleanly configured on active VFO A and audio unmuted.
        for _ in 0..10 {
            let _ = coord.tick_10ms(&mut radio);
        }
        assert!(!coord.is_vfo_locked());
        assert!(!radio.muted);
        assert_eq!(radio.rx_freq, 145_500_000);
    }

    #[test]
    fn test_coordinator_ptt_ctcss_lead_in_tail_and_bclo_gating() {
        let mut coord = RadioCoordinator::new();
        let mut radio = MockRadio::new();
        coord.channels[0].rx_freq = Frequency::new(145_700_000);
        coord.channels[0].tx_subaudible = SubaudibleTone::Ctcss(885);
        coord.channels[0].busy_lock = true;
        coord.bclo_policy = BcloPolicy::CarrierOnly;

        let ui = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);

        // BCLO must block TX while carrier is active on listening VFO.
        coord.carrier_active = true;
        assert_eq!(
            coord.request_ptt_start(&mut radio, ui),
            TxPermission::BlockedBusyChannel
        );
        coord.carrier_active = false;

        // CTCSS channel enters 50ms LeadIn (mic muted) -> Active (mic unmuted).
        assert_eq!(
            coord.request_ptt_start(&mut radio, ui),
            TxPermission::Granted
        );
        assert!(coord.is_transmitting);
        assert!(radio.mic_muted);
        for _ in 0..5 {
            let _ = coord.tick_10ms(&mut radio);
        }
        assert_eq!(coord.tx_state, TxState::Active);
        assert!(!radio.mic_muted);

        // PTT stop triggers 150ms 55Hz tail burst -> full teardown to RX.
        coord.request_ptt_stop(&mut radio);
        assert!(matches!(coord.tx_state, TxState::TailBurst { .. }));
        assert_eq!(radio.tail_sent, CssTailMode::Tone55Hz);
        for _ in 0..15 {
            let _ = coord.tick_10ms(&mut radio);
        }
        assert!(!coord.is_transmitting);
        assert_eq!(coord.tx_state, TxState::Idle);
        assert_eq!(radio.rx_freq, 145_700_000);
    }

    #[test]
    fn test_coordinator_dual_watch_freezes_when_locked_and_alternates() {
        let mut coord = RadioCoordinator::new();
        let mut radio = MockRadio::new();
        coord.channels[0].rx_freq = Frequency::new(145_500_000);
        coord.channels[1].rx_freq = Frequency::new(433_500_000);
        coord.listening_vfo = VfoSlot::VfoA;
        coord.dual_watch = DualWatchMode::EqualDwell { dwell_ms: 20 };

        let bg = ClientToken::new(ClientId::SCANNER, LockTier::BACKGROUND);
        assert!(coord.request_vfo_lock(bg, VfoSlot::VfoA, 20, false));
        // Locked for 20ms: ticks 1 & 2 handle arbitration expiration, not dual-watch hop.
        assert_eq!(coord.tick_10ms(&mut radio), RadioEvent::None);
        assert!(matches!(
            coord.tick_10ms(&mut radio),
            RadioEvent::LockReleased { .. }
        ));

        // Unlocked: after 20ms dwell ticks, switches to VfoB and configures 433.500 MHz.
        assert_eq!(coord.tick_10ms(&mut radio), RadioEvent::None);
        assert_eq!(coord.tick_10ms(&mut radio), RadioEvent::None);
        assert_eq!(
            coord.tick_10ms(&mut radio),
            RadioEvent::DualWatchSwitched(VfoSlot::VfoB)
        );
        assert_eq!(radio.rx_freq, 433_500_000);
    }

    #[test]
    fn test_coordinator_tone_bclo_crossband_bw_and_client_unlock_restoration() {
        let mut coord = RadioCoordinator::new();
        let mut radio = MockRadio::new();
        coord.dual_watch = DualWatchMode::Off;
        coord.channels[0].rx_freq = Frequency::new(145_500_000);
        coord.channels[0].bandwidth = Bandwidth::Narrow12k5;
        coord.channels[0].rx_subaudible = SubaudibleTone::Ctcss(885);
        coord.channels[0].busy_lock = true;
        coord.channels[1].rx_freq = Frequency::new(433_500_000);
        coord.channels[1].bandwidth = Bandwidth::Wide25k;
        coord.channels[1].tx_subaudible = SubaudibleTone::DcsNormal(0o023);
        coord.cross_band = CrossBandMode::FixedTxOnB;
        coord.bclo_policy = BcloPolicy::ToneMismatchOnly;
        coord.listening_vfo = VfoSlot::VfoA;

        // ToneMismatchOnly: busy carrier without tone match blocks TX on VfoA.
        radio.carrier_open = true;
        radio.tone_matched = false;
        let _ = coord.tick_10ms(&mut radio); // CarrierOpened
        let _ = coord.tick_10ms(&mut radio); // polls is_subaudible_matched
        let ui = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);
        assert_eq!(
            coord.check_tx_permission(VfoSlot::VfoA, ui),
            TxPermission::BlockedBusyChannel
        );

        // Matched CTCSS tone unlocks ToneMismatchOnly permission.
        radio.tone_matched = true;
        let _ = coord.tick_10ms(&mut radio);
        assert_eq!(
            coord.check_tx_permission(VfoSlot::VfoA, ui),
            TxPermission::Granted
        );

        // Cross-band TX applies TX slot bandwidth (Wide25k) and stop_tx restores RX bw + tone.
        assert_eq!(
            coord.request_ptt_start(&mut radio, ui),
            TxPermission::Granted
        );
        assert_eq!(radio.bandwidth, Bandwidth::Wide25k);
        radio.carrier_open = false;
        coord.stop_tx(&mut radio);
        assert_eq!(radio.bandwidth, Bandwidth::Narrow12k5);
        assert_eq!(radio.rx_subaudible, SubaudibleTone::Ctcss(885));

        // Preempting KRX client lock on VfoB restores stashed FSK lock on VfoA when released.
        let fsk = ClientToken::new(ClientId::FSK_SIDECAR, LockTier::BACKGROUND);
        let krx_id = ClientId::for_krx_pid(3);
        let krx = ClientToken::new(krx_id, LockTier::FOREGROUND);
        assert!(coord.request_resource_lock(fsk, RfResourceMask::VFO_A, VfoSlot::VfoA, 500, true));
        assert!(coord.request_resource_lock(krx, RfResourceMask::VFO_B, VfoSlot::VfoB, 500, false));
        assert_eq!(coord.listening_vfo, VfoSlot::VfoB);
        assert!(coord.release_client_lock(&mut radio, krx_id));
        assert_eq!(coord.listening_vfo, VfoSlot::VfoA);
        assert_eq!(coord.arbitrator.active_client(), fsk);
        assert!(coord.fsk_sync_active);
    }

    #[test]
    fn channel_recall_preserves_reference_tuning_steps() {
        let mut coord = RadioCoordinator::new();
        let mut radio = MockRadio::new();
        let mut record =
            crate::data::ChannelRecord::new_repeater(145_750_000, 145_150_000, 0, "GRID");

        for units in [
            0,
            1,
            2,
            5,
            10,
            25,
            50,
            100,
            125,
            250,
            500,
            625,
            833,
            900,
            1000,
            1250,
            1500,
            2000,
            2500,
            3000,
            5000,
            10000,
            12500,
            20000,
            25000,
            50000,
            u16::MAX,
        ] {
            record.step_hz_x10 = units;
            record.record_crc = record.compute_crc();
            coord.load_channel_into_vfo(&mut radio, VfoSlot::VfoA, Some(0), &record);
            assert_eq!(
                coord.vfo_channel(VfoSlot::VfoA).step.in_hz(),
                if units == 0 {
                    12_500
                } else {
                    u32::from(units) * 10
                },
            );
            coord.save_vfo_to_channel(VfoSlot::VfoA, &mut record);
            assert_eq!(record.step_hz_x10, units);
        }
    }

    #[test]
    fn test_external_vfo_app_channel_roundtrip_talkaround_and_multi_app_arbitration() {
        use crate::data::ChannelRecord;

        let mut coord = RadioCoordinator::new();
        let mut radio = MockRadio::new();
        coord.dual_watch = DualWatchMode::Off;

        // 1. Background Scanner locks VfoA (BACKGROUND), FSK Sidecar locks VfoB (SERVICE),
        //    and external VFO KRX app (FOREGROUND) locks VfoA.
        let scanner = ClientToken::new(ClientId::SCANNER, LockTier::BACKGROUND);
        let fsk = ClientToken::new(ClientId::FSK_SIDECAR, LockTier::SERVICE);
        let vfo_app_id = ClientId::for_krx_pid(2);
        let vfo_app = ClientToken::new(vfo_app_id, LockTier::FOREGROUND);

        assert!(coord.request_vfo_lock(scanner, VfoSlot::VfoA, 500, true));
        assert!(coord.request_vfo_lock(fsk, VfoSlot::VfoB, 400, true));
        assert!(coord.request_vfo_lock(vfo_app, VfoSlot::VfoA, 300, false));
        assert_eq!(coord.lock_depth(), 3);
        assert_eq!(coord.listening_vfo, VfoSlot::VfoA);

        // 2. While holding the lock on VfoA, the external VFO app loads a rich ChannelRecord
        let mut rec = ChannelRecord::new_repeater(145_750_000, 145_150_000, 0, "R6 ALPINE");
        rec.set_rx_subaudible_tone(SubaudibleTone::Ctcss(1035));
        rec.set_tx_subaudible_tone(SubaudibleTone::DcsNormal(0o047));
        rec.flags = ChannelRecord::FLAG_BUSY_LOCKOUT
            | ChannelRecord::FLAG_TONE_BCLO
            | ChannelRecord::FLAG_CALL_MATCH_MUTE;
        rec.compand = 3; // Both
        rec.scram_mode = 1;
        rec.scram_param = 4;
        rec.ste = 3; // PhaseShift180Deg
        rec.mic_gain = 18;
        rec.roger = 2;
        rec.tx_limit = 8;
        rec.proto = 5;
        rec.proto_ref = 42;

        coord.load_channel_into_vfo(&mut radio, VfoSlot::VfoA, Some(7), &rec);
        // Must configure hardware immediately even though VfoA is locked by vfo_app!
        assert_eq!(radio.rx_freq, 145_750_000);
        assert_eq!(radio.rx_subaudible, SubaudibleTone::Ctcss(1035));
        assert_eq!(coord.vfo_memory_index[0], Some(7));

        // 3. Test reverse and talkaround while locked on VfoA
        coord.toggle_reverse(&mut radio, VfoSlot::VfoA);
        assert_eq!(radio.rx_freq, 145_150_000);
        assert_eq!(radio.rx_subaudible, SubaudibleTone::DcsNormal(0o047));

        assert!(coord.toggle_talkaround(&mut radio, VfoSlot::VfoA));
        // Talkaround forces direct RX frequency (145.750 MHz) and unswapped subaudible
        assert_eq!(radio.rx_freq, 145_750_000);
        assert_eq!(coord.channels[0].effective_tx_freq().raw(), 145_750_000);
        assert!(!coord.toggle_talkaround(&mut radio, VfoSlot::VfoA));
        coord.toggle_reverse(&mut radio, VfoSlot::VfoA);
        assert_eq!(radio.rx_freq, 145_750_000);

        // 4. Test save_vfo_to_channel preserves all attributes 100% losslessly
        let mut saved = ChannelRecord::empty();
        coord.save_vfo_to_channel(VfoSlot::VfoA, &mut saved);
        assert!(saved.is_crc_valid());
        assert_eq!(saved.rx_frequency_hz, 145_750_000);
        assert_eq!(saved.tx_frequency_hz, 145_150_000);
        assert_eq!(saved.rx_subaudible_tone(), SubaudibleTone::Ctcss(1035));
        assert_eq!(saved.tx_subaudible_tone(), SubaudibleTone::DcsNormal(0o047));
        assert_eq!(saved.compand, 3);
        assert_eq!(saved.scram_param, 4);
        assert_eq!(saved.ste, 3);
        assert_eq!(saved.mic_gain, 18);
        assert_eq!(saved.roger, 2);
        assert_eq!(saved.tx_limit, 8);
        assert_eq!(saved.proto, 5);
        assert_eq!(saved.proto_ref, 42);
        assert_ne!(saved.flags & ChannelRecord::FLAG_BUSY_LOCKOUT, 0);
        assert_ne!(saved.flags & ChannelRecord::FLAG_TONE_BCLO, 0);
        assert_ne!(saved.flags & ChannelRecord::FLAG_CALL_MATCH_MUTE, 0);

        // 5. Per-channel FLAG_TX_INHIBIT blocks TX; clearing it permits TX with custom 180-deg STE tail
        coord.channels[0].set_tx_inhibit(true);
        assert_eq!(
            coord.check_tx_permission(VfoSlot::VfoA, vfo_app),
            TxPermission::BlockedTxInhibited
        );
        coord.channels[0].set_tx_inhibit(false);
        coord.channels[0].tx_subaudible = SubaudibleTone::Ctcss(1035);
        assert_eq!(
            coord.request_ptt_start(&mut radio, vfo_app),
            TxPermission::Granted
        );
        coord.request_ptt_stop(&mut radio);
        assert_eq!(radio.tail_sent, CssTailMode::PhaseShift180Deg);
        for _ in 0..15 {
            let _ = coord.tick_10ms(&mut radio);
        }
        assert!(!coord.is_transmitting);

        // 6. When external VFO app closes/releases its lock, FSK Sidecar (VfoB) is restored,
        //    and when FSK Sidecar releases, Background Scanner (VfoA) is restored!
        assert!(coord.release_client_lock(&mut radio, vfo_app_id));
        assert_eq!(coord.lock_depth(), 2);
        assert_eq!(coord.listening_vfo, VfoSlot::VfoB);
        assert_eq!(coord.arbitrator.active_client(), fsk);

        assert!(coord.release_client_lock(&mut radio, ClientId::FSK_SIDECAR));
        assert_eq!(coord.lock_depth(), 1);
        assert_eq!(coord.listening_vfo, VfoSlot::VfoA);
        assert_eq!(coord.arbitrator.active_client(), scanner);
    }

    #[test]
    fn test_coordinator_ptt_modes_and_monitor_squelch_defeat() {
        let mut coord = RadioCoordinator::new();
        let mut radio = MockRadio::new();
        let ui = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);

        // 1. Monitor mode (Squelch Defeat) unmutes audio without carrier and freezes Dual Watch
        coord.dual_watch = DualWatchMode::EqualDwell { dwell_ms: 20 };
        coord.listening_vfo = VfoSlot::VfoA;
        radio.muted = true;
        assert!(coord.toggle_monitor(&mut radio));
        assert!(coord.monitor_active);
        assert!(!radio.muted);

        // Even after 50ms of ticks without carrier, Monitor mode keeps audio unmuted and stays on VfoA
        for _ in 0..5 {
            assert_eq!(coord.tick_10ms(&mut radio), RadioEvent::None);
        }
        assert_eq!(coord.listening_vfo, VfoSlot::VfoA);
        assert!(!radio.muted);

        // Toggling Monitor off mutes audio and resumes Dual Watch
        assert!(!coord.toggle_monitor(&mut radio));
        assert!(!coord.monitor_active);
        assert!(radio.muted);

        // 2. PttMode::Toggle latches on first press and stops on second press
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::Toggle, true, 1000, ui),
            PttEventOutcome::Started(TxPermission::Granted)
        );
        assert!(coord.is_transmitting);
        assert!(coord.ptt_latched);
        // Key release in Toggle mode does not stop TX
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::Toggle, false, 1100, ui),
            PttEventOutcome::None
        );
        assert!(coord.is_transmitting);
        // Second press stops TX and clears latch
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::Toggle, true, 1500, ui),
            PttEventOutcome::Stopped
        );
        assert!(!coord.is_transmitting);
        assert!(!coord.ptt_latched);

        // 3. PttMode::DoublePressLatch: single press (>400ms gap) acts as momentary;
        //    second press within <=400ms latches TX on release!
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::DoublePressLatch, true, 3000, ui),
            PttEventOutcome::Started(TxPermission::Granted)
        );
        assert!(!coord.ptt_latched);
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::DoublePressLatch, false, 3100, ui),
            PttEventOutcome::Stopped
        );
        assert!(!coord.is_transmitting);

        // Second press at 3250ms (250ms after 3000ms press <= 400ms) -> LATCHES!
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::DoublePressLatch, true, 3250, ui),
            PttEventOutcome::Started(TxPermission::Granted)
        );
        assert!(coord.ptt_latched);
        // Releasing while latched keeps transmitting
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::DoublePressLatch, false, 3350, ui),
            PttEventOutcome::None
        );
        assert!(coord.is_transmitting);
        // Next press unlatches and stops TX
        assert_eq!(
            coord.handle_ptt_key(&mut radio, PttMode::DoublePressLatch, true, 4000, ui),
            PttEventOutcome::Stopped
        );
        assert!(!coord.is_transmitting);
        assert!(!coord.ptt_latched);
    }
}
