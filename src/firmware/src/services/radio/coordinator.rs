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
pub enum CrossBandMode {
    Off,
    FixedTxOnA,
    FixedTxOnB,
    MasterFollowsRx,
}

/// Transmit target selection policy when Dual Watch / Multi-VFO is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DualWatchTxPolicy {
    /// Always transmit on the actively selected primary VFO.
    ActiveVfo,
    /// Transmit on the listening VFO if carrier was open or within post-carrier hold period.
    FollowCarrier,
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
pub struct RadioCoordinator {
    pub channels: [ChannelConfig; 3],
    pub active_vfo: VfoSlot,
    pub listening_vfo: VfoSlot,
    pub dual_watch: DualWatchMode,
    pub cross_band: CrossBandMode,
    pub fsk_config: FskSidecarConfig,
    pub app_lock: VfoArbitrationLock,
    /// Stashed background lock preempted by a higher-priority task.
    pub saved_lock: Option<VfoArbitrationLock>,
    pub dwell: DwellController,
    pub audio_routing: AudioRoutingState,
    pub vfo_memory_index: [Option<u16>; 3],
    pub is_transmitting: bool,
    /// Granular transmission state machine (Idle, Active, TailBurst).
    pub tx_state: TxState,
    pub carrier_active: bool,
    pub fsk_sync_active: bool,
    pub rssi_moving_avg: [i16; 3],
    pub activity_count: [u16; 3],
    pub audio_path_muted: bool,
    /// Physical tuner currently routed through the audio cross-bar to the power amplifier.
    pub audio_source: AudioCrossbarSource,
    /// Busy Channel Lockout evaluation policy.
    pub bclo_policy: BcloPolicy,
    /// Dual Watch transmit target selection policy.
    pub dw_tx_policy: DualWatchTxPolicy,
    /// Time-Out Timer limit in seconds (0 = disabled).
    pub tot_limit_seconds: u16,
    /// Pre-timeout acoustic warning lead time in seconds. Default: 10s.
    pub tot_warning_lead_seconds: u8,
    /// Current transmission elapsed time in 10ms ticks.
    pub tot_elapsed_10ms: u16,
    /// Pre-timeout acoustic warning flag.
    pub tot_warning_given: bool,
    /// Lockout penalty active: PTT must be released before transmitting again.
    pub tot_locked_out: bool,
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
            channels: [ch_a, ch_b, ch_c],
            active_vfo: VfoSlot::VfoA,
            listening_vfo: VfoSlot::VfoA,
            dual_watch: DualWatchMode::Off,
            cross_band: CrossBandMode::Off,
            fsk_config: FskSidecarConfig::DEFAULT,
            app_lock: VfoArbitrationLock::IDLE,
            saved_lock: None,
            dwell: DwellController::new(),
            audio_routing: AudioRoutingState::NormalVoice,
            vfo_memory_index: [None, None, None],
            is_transmitting: false,
            tx_state: TxState::Idle,
            carrier_active: false,
            fsk_sync_active: false,
            rssi_moving_avg: [-120, -120, -120],
            activity_count: [0, 0, 0],
            audio_path_muted: false,
            audio_source: AudioCrossbarSource::Primary,
            bclo_policy: BcloPolicy::CarrierOnly,
            dw_tx_policy: DualWatchTxPolicy::ActiveVfo,
            tot_limit_seconds: 60, // Default 60-second TOT
            tot_warning_lead_seconds: 10,
            tot_elapsed_10ms: 0,
            tot_warning_given: false,
            tot_locked_out: false,
        }
    }

    #[inline(always)]
    pub fn active_channel(&self) -> &ChannelConfig {
        &self.channels[self.active_vfo.index().min(self.channels.len() - 1)]
    }

    #[inline(always)]
    pub fn active_channel_mut(&mut self) -> &mut ChannelConfig {
        let idx = self.active_vfo.index().min(self.channels.len() - 1);
        &mut self.channels[idx]
    }

    #[inline(always)]
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
        if let Some(saved) = self.saved_lock.take() {
            self.app_lock = saved;
            self.listening_vfo = saved.slot;
            self.fsk_sync_active = saved.client.id == ClientId::FSK_SIDECAR;
            self.dwell.reset_dwell();
            Some(saved)
        } else {
            self.app_lock = VfoArbitrationLock::IDLE;
            self.fsk_sync_active = false;
            self.dwell.reset_dwell();
            None
        }
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
        if self.is_transmitting && resources.intersects(RfResourceMask::TRANSMITTER) {
            return false;
        }

        if self.app_lock.is_active() && self.app_lock.client != client {
            if self.app_lock.resources.intersects(resources) {
                if client.priority() <= self.app_lock.client.priority() {
                    return false; // Lower or equal priority cannot preempt
                }
                // Preempting active lock: stash into saved_lock
                self.saved_lock = Some(self.app_lock);
            }
        }

        self.app_lock = VfoArbitrationLock {
            client,
            resources,
            slot,
            remaining_ms: timeout_ms.max(10),
            audio_muted: mute_audio,
        };
        self.listening_vfo = slot;
        self.dwell.reset_dwell();
        true
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
        if self.app_lock.is_active() && self.app_lock.client == client {
            self.app_lock.remaining_ms = timeout_ms.max(self.app_lock.remaining_ms);
            true
        } else {
            false
        }
    }

    /// Explicitly release an RF resource lock held by a client.
    pub fn release_vfo_lock(&mut self, client: ClientToken) -> bool {
        if self.app_lock.client == client {
            self.pop_and_restore_lock();
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

    /// Force-release all active locks (e.g. user pressed PTT or exit key).
    pub fn force_release_all_locks(&mut self) {
        self.app_lock = VfoArbitrationLock::IDLE;
        self.saved_lock = None;
        self.fsk_sync_active = false;
        self.dwell.reset_dwell();
    }

    #[inline(always)]
    pub fn is_vfo_locked(&self) -> bool {
        self.app_lock.is_active()
    }

    #[inline(always)]
    pub fn lock_client(&self) -> ClientToken {
        self.app_lock.client
    }

    #[inline(always)]
    pub fn locked_slot(&self) -> Option<VfoSlot> {
        if self.app_lock.is_active() {
            Some(self.app_lock.slot)
        } else {
            None
        }
    }

    /// Current arbitration lock depth (0 = none, 1 = active lock, 2 = active + stashed lock).
    #[inline(always)]
    pub fn lock_depth(&self) -> usize {
        if !self.app_lock.is_active() {
            0
        } else if self.saved_lock.is_some() {
            2
        } else {
            1
        }
    }

    /// Reference to the active lock descriptor, if active.
    #[inline(always)]
    pub fn active_lock(&self) -> Option<&VfoArbitrationLock> {
        if self.app_lock.is_active() {
            Some(&self.app_lock)
        } else {
            None
        }
    }

    /// Reference to the stashed preempted lock descriptor, if present.
    #[inline(always)]
    pub fn saved_lock(&self) -> Option<&VfoArbitrationLock> {
        self.saved_lock.as_ref()
    }

    /// Check if a specific RF hardware resource is currently locked.
    #[inline(always)]
    pub fn is_resource_locked(&self, res: RfResourceMask) -> bool {
        self.app_lock.is_active() && self.app_lock.resources.intersects(res)
    }

    /// Verify whether a client is permitted to initiate RF transmission.
    pub fn can_app_transmit(&self, client: ClientToken) -> bool {
        if self.is_transmitting {
            return false;
        }
        if let Some(holder) = crate::orchestrator::lease_manager().holder(kernel::proc::LeaseKind::RadioTx) {
            let caller_pid = crate::orchestrator::proc_table().find_by_app(client.id.0 as usize).map(|p| p.pid);
            if caller_pid != Some(holder) && client.priority() < LockTier::EXCLUSIVE.0 {
                return false;
            }
        }
        if self.app_lock.is_active()
            && self
                .app_lock
                .resources
                .intersects(RfResourceMask::TRANSMITTER)
        {
            return self.app_lock.client == client || client.priority() >= LockTier::EXCLUSIVE.0;
        }
        true
    }

    // ── FSK Sidecar Event Handlers ──

    pub const FSK_CLIENT: ClientToken = ClientToken::new(ClientId::FSK_SIDECAR, LockTier::SERVICE);

    /// Notify coordinator that an FSK preamble/sync burst has been detected on the physical layer.
    pub fn notify_fsk_sync<R: RfAudioPath>(&mut self, radio: &mut R, slot: VfoSlot) -> RadioEvent {
        self.fsk_sync_active = true;
        let mute = self.fsk_config.mute_audio_on_sync;

        self.request_vfo_lock(Self::FSK_CLIENT, slot, self.fsk_config.sync_hold_ms, mute);

        if mute && !self.audio_path_muted {
            radio.mute_audio(true);
            self.audio_path_muted = true;
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
        if self.app_lock.client.id == ClientId::FSK_SIDECAR {
            // Keep locked for post-frame response/ACK window
            self.app_lock.remaining_ms = self.fsk_config.post_frame_hold_ms;
        }
        RadioEvent::FskFrameFinished(slot)
    }

    /// Abort FSK sidecar hold on error or timeout.
    pub fn notify_fsk_error<R: RfAudioPath>(&mut self, radio: &mut R) {
        if self.app_lock.client.id == ClientId::FSK_SIDECAR {
            self.release_vfo_lock(self.app_lock.client);
            if self.audio_path_muted {
                radio.mute_audio(false);
                self.audio_path_muted = false;
            }
        }
    }

    // ── VFO Operations & UI Helpers ──

    pub fn select_vfo<R: RfChannel + RfSquelch>(&mut self, radio: &mut R, slot: VfoSlot) {
        if slot.index() < self.channels.len() {
            self.active_vfo = slot;
            if self.dual_watch == DualWatchMode::Off
                && !self.is_transmitting
                && !self.app_lock.is_active()
            {
                self.listening_vfo = slot;
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

    pub fn toggle_reverse<R: RfChannel>(&mut self, radio: &mut R, slot: VfoSlot) {
        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &mut self.channels[idx];
        ch.reverse = !ch.reverse;
        if self.listening_vfo == slot && !self.is_transmitting {
            radio.set_rx_frequency(ch.effective_rx_freq());
        }
    }

    /// Check granular transmission permission for a specific VFO slot and client token.
    pub fn check_tx_permission(&self, slot: VfoSlot, client: ClientToken) -> TxPermission {
        if self.tot_locked_out {
            return TxPermission::BlockedTotPenalty;
        }

        if self.is_transmitting && self.tx_state == TxState::Active {
            return TxPermission::BlockedResourceLocked;
        }

        if self.app_lock.is_active()
            && self
                .app_lock
                .resources
                .intersects(RfResourceMask::TRANSMITTER)
        {
            if self.app_lock.client != client && client.priority() < LockTier::EXCLUSIVE.0 {
                return TxPermission::BlockedResourceLocked;
            }
        }

        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &self.channels[idx];

        if ch.rx_freq.raw() == 0 || ch.effective_tx_freq().raw() == 0 {
            return TxPermission::BlockedTxInhibited;
        }

        if ch.busy_lock && self.carrier_active && self.listening_vfo == slot {
            match self.bclo_policy {
                BcloPolicy::Disabled => {}
                BcloPolicy::CarrierOnly => return TxPermission::BlockedBusyChannel,
                BcloPolicy::ToneMismatchOnly => {
                    if ch.rx_subaudible.is_ctcss() || ch.rx_subaudible.is_dcs() {
                        return TxPermission::BlockedBusyChannel;
                    }
                    return TxPermission::BlockedBusyChannel;
                }
            }
        }

        TxPermission::Granted
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

    /// Called when the physical PTT key is released. Clears TOT lockout penalty.
    pub fn on_ptt_released(&mut self) {
        self.tot_locked_out = false;
        self.tot_elapsed_10ms = 0;
        self.tot_warning_given = false;
    }

    pub fn start_tx<R: RfChannel + RfTuning>(&mut self, radio: &mut R) {
        self.is_transmitting = true;
        self.tx_state = TxState::Active;
        bsp::set_status_led(bsp::StatusLed::Red);
        self.tot_elapsed_10ms = 0;
        self.tot_warning_given = false;
        let slot = self.effective_tx_slot();
        self.listening_vfo = slot;
        self.dwell.reset_dwell();
        let tx_ch = *self.effective_tx_channel();
        radio.set_tx_frequency(tx_ch.effective_tx_freq());
        radio.set_power_level(tx_ch.power);
        radio.set_modulation(tx_ch.modulation);
    }

    pub fn stop_tx<R: RfChannel + RfTuning + RfSubaudible>(&mut self, radio: &mut R) {
        let tx_ch = *self.effective_tx_channel();
        if tx_ch.tx_subaudible.is_ctcss() {
            radio.send_css_tail(CssTailMode::Tone55Hz);
        }
        if let AudioRoutingState::ToneBurstActive { .. } = self.audio_routing {
            self.audio_routing = AudioRoutingState::NormalVoice;
        }
        self.is_transmitting = false;
        self.tx_state = TxState::Idle;
        bsp::set_status_led(if self.carrier_active {
            bsp::StatusLed::Green
        } else {
            bsp::StatusLed::Off
        });
        self.tot_elapsed_10ms = 0;
        self.tot_warning_given = false;
        self.dwell.on_tx_finished();
        let rx_ch = *self.listening_channel();
        radio.set_rx_frequency(rx_ch.effective_rx_freq());
        radio.set_power_level(rx_ch.power);
        radio.set_modulation(rx_ch.modulation);
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

        let tx_ch = *self.effective_tx_channel();
        if tx_ch.tx_subaudible.is_ctcss() {
            radio.send_css_tail(CssTailMode::Tone55Hz);
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
        let is_locked = self.app_lock.is_active() && self.app_lock.slot == slot;
        ChannelTelemetry {
            slot,
            rssi_dbm: self.rssi_moving_avg[idx],
            activity_count: self.activity_count[idx],
            is_carrier_open: self.carrier_active && self.listening_vfo == slot,
            is_locked,
            lock_client: if is_locked {
                self.app_lock.client.id
            } else {
                ClientId::NONE
            },
        }
    }

    /// Query the complete display activity state of a VFO.
    pub fn vfo_activity_state(&self, slot: VfoSlot) -> VfoActivityState {
        if self.is_transmitting
            && self.effective_tx_channel().rx_freq == self.channels[slot.index()].rx_freq
        {
            VfoActivityState::Transmitting
        } else if self.app_lock.is_active() && self.app_lock.slot == slot {
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
    pub fn step_vfo_freq<R: RfChannel>(&mut self, radio: &mut R, slot: VfoSlot, step_hz: i32) {
        let idx = slot.index().min(self.channels.len() - 1);
        let cur = self.channels[idx].rx_freq.raw() as i64;
        let next = (cur + step_hz as i64).clamp(18_000_000, 1_300_000_000) as u32;
        self.channels[idx].rx_freq = utils::units::Frequency::new(next);
        if self.listening_vfo == slot && !self.is_transmitting {
            radio.set_rx_frequency(self.channels[idx].effective_rx_freq());
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
        }
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
        let should_apply =
            self.listening_vfo == slot && !self.is_transmitting && !self.app_lock.is_active();
        self.populate_channel_config(slot, memory_idx, rec);
        if should_apply {
            let idx = slot.index().min(self.channels.len() - 1);
            radio.configure_channel(&self.channels[idx]);
        }
    }

    #[inline(never)]
    fn populate_channel_config(
        &mut self,
        slot: VfoSlot,
        memory_idx: Option<u16>,
        rec: &crate::data::ChannelRecord,
    ) {
        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &mut self.channels[idx];

        ch.rx_freq = Frequency::<Hertz>::new(rec.rx_frequency_hz);
        ch.tx_shift = if rec.tx_frequency_hz != 0 {
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
        ch.step = match rec.step_hz_x10 {
            125 => StepSetting::Step1_25k,
            250 => StepSetting::Step2_5k,
            500 => StepSetting::Step5k,
            625 => StepSetting::Step6_25k,
            1000 => StepSetting::Step10k,
            1250 => StepSetting::Step12_5k,
            2500 => StepSetting::Step25k,
            5000 => StepSetting::Step50k,
            10000 => StepSetting::Step100k,
            _ => StepSetting::Step12_5k,
        };
        ch.bandwidth = if rec.bandwidth_50hz >= 400
            || (rec.flags & crate::data::ChannelRecord::FLAG_WIDE_FILTER) != 0
        {
            Bandwidth::Wide25k
        } else if rec.bandwidth_50hz > 0 && rec.bandwidth_50hz <= 150 {
            Bandwidth::Narrow6k25
        } else {
            Bandwidth::Narrow12k5
        };
        ch.modulation = match rec.modulation {
            1 => ModulationMode::Am,
            2 => ModulationMode::AmB,
            3 => ModulationMode::Usb,
            4 => ModulationMode::Dsb,
            5 => ModulationMode::Dsb,
            6 => ModulationMode::Cw,
            7 => ModulationMode::Wfm,
            8 => ModulationMode::PktFm,
            9 => ModulationMode::Raw,
            10 => ModulationMode::Byp,
            _ => ModulationMode::Fm,
        };
        ch.power = if rec.power_mw == 0 || rec.power_mw >= 4000 {
            RfPowerLevel::High
        } else if rec.power_mw >= 1500 {
            RfPowerLevel::Medium
        } else {
            RfPowerLevel::Low
        };
        ch.squelch_level = rec.squelch_level;
        ch.reverse = (rec.flags & crate::data::ChannelRecord::FLAG_REVERSE) != 0;

        self.vfo_memory_index[idx] = memory_idx;
    }

    /// Serialize current VFO tuning parameters into a ChannelRecord.
    pub fn save_vfo_to_channel(&self, slot: VfoSlot, rec: &mut crate::data::ChannelRecord) {
        let idx = slot.index().min(self.channels.len() - 1);
        let ch = &self.channels[idx];

        rec.rx_frequency_hz = ch.rx_freq.raw();
        rec.tx_frequency_hz = match ch.tx_shift {
            RepeaterShift::Simplex => 0,
            RepeaterShift::Positive(off) => ch.rx_freq.raw().saturating_add(off.raw()),
            RepeaterShift::Negative(off) => ch.rx_freq.raw().saturating_sub(off.raw()),
        };
        rec.step_hz_x10 = match ch.step {
            StepSetting::Step1_25k => 125,
            StepSetting::Step2_5k => 250,
            StepSetting::Step5k => 500,
            StepSetting::Step6_25k => 625,
            StepSetting::Step8_33k => 833,
            StepSetting::Step10k => 1000,
            StepSetting::Step12_5k => 1250,
            StepSetting::Step25k => 2500,
            StepSetting::Step50k => 5000,
            StepSetting::Step100k => 10000,
        };
        rec.bandwidth_50hz = match ch.bandwidth {
            Bandwidth::Wide25k => 500,    // 25 kHz
            Bandwidth::Narrow6k25 => 125, // 6.25 kHz
            Bandwidth::Narrow12k5 => 250, // 12.5 kHz
        };
        rec.modulation = match ch.modulation {
            ModulationMode::Fm => 0,
            ModulationMode::Am => 1,
            ModulationMode::AmB => 2,
            ModulationMode::Usb => 3,
            ModulationMode::Dsb => 5,
            ModulationMode::Cw => 6,
            ModulationMode::Wfm => 7,
            ModulationMode::PktFm => 8,
            ModulationMode::Raw => 9,
            ModulationMode::Byp => 10,
        };
        rec.power_mw = match ch.power {
            RfPowerLevel::Low => 500,
            RfPowerLevel::Medium => 2000,
            RfPowerLevel::High => 5000,
        };
        rec.squelch_level = ch.squelch_level;
        if ch.reverse {
            rec.flags |= crate::data::ChannelRecord::FLAG_REVERSE;
        } else {
            rec.flags &= !crate::data::ChannelRecord::FLAG_REVERSE;
        }
        rec.record_crc = rec.compute_crc();
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

        if self.listening_vfo == slot && !self.is_transmitting && !self.app_lock.is_active() {
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

    // ── Main 10ms System Tick ──

    #[inline(never)]
    pub fn tick_10ms<R: RadioService>(&mut self, radio: &mut R) -> RadioEvent {
        // Active transmission state machine
        if self.is_transmitting && self.tx_state == TxState::Active {
            self.tot_elapsed_10ms = self.tot_elapsed_10ms.saturating_add(1);
            if self.tot_limit_seconds > 0 {
                let limit_ticks = (self.tot_limit_seconds as u32) * 100;
                let elapsed = self.tot_elapsed_10ms as u32;
                if elapsed >= limit_ticks {
                    self.stop_tx(radio);
                    self.tot_locked_out = true; // Lockout penalty until physical PTT release
                    return RadioEvent::TotExpired;
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
                        return RadioEvent::TotWarning {
                            remaining_sec: self.tot_warning_lead_seconds,
                        };
                    }
                }
            }
            return RadioEvent::None;
        }

        // Subaudible tail burst countdown
        if let TxState::TailBurst { remaining_ms } = self.tx_state {
            if remaining_ms <= 10 {
                self.is_transmitting = false;
                self.tx_state = TxState::Idle;
                bsp::set_status_led(if self.carrier_active {
                    bsp::StatusLed::Green
                } else {
                    bsp::StatusLed::Off
                });
                self.tot_elapsed_10ms = 0;
                self.tot_warning_given = false;
                self.dwell.on_tx_finished();
                radio.configure_channel(self.listening_channel());
                return RadioEvent::TxEnded;
            } else {
                self.tx_state = TxState::TailBurst {
                    remaining_ms: remaining_ms.saturating_sub(10),
                };
                return RadioEvent::None;
            }
        }

        // 0. Process active 1750 Hz tone burst countdown
        if let AudioRoutingState::ToneBurstActive { remaining_ms } = self.audio_routing {
            if remaining_ms <= 10 {
                radio.mute_tones();
                self.audio_routing = AudioRoutingState::NormalVoice;
            } else {
                self.audio_routing = AudioRoutingState::ToneBurstActive {
                    remaining_ms: remaining_ms.saturating_sub(10),
                };
            }
        }

        // 1. Process active arbitration lock countdown
        if self.app_lock.is_active() {
            if self.app_lock.remaining_ms <= 10 {
                let client = self.app_lock.client;
                let resources = self.app_lock.resources;
                let slot = self.app_lock.slot;
                let was_muted = self.app_lock.audio_muted;

                let restored = self.pop_and_restore_lock();

                if was_muted && self.audio_path_muted {
                    let needs_mute = self.app_lock.is_active() && self.app_lock.audio_muted;
                    if !needs_mute {
                        radio.mute_audio(false);
                        self.audio_path_muted = false;
                    }
                }

                if let Some(res) = restored {
                    if res.audio_muted && !self.audio_path_muted {
                        radio.mute_audio(true);
                        self.audio_path_muted = true;
                    }
                    return RadioEvent::LockRestored {
                        client: res.client,
                        resources: res.resources,
                        slot: res.slot,
                    };
                }

                if client.id == ClientId::FSK_SIDECAR {
                    return RadioEvent::FskLockReleased(slot);
                } else {
                    return RadioEvent::LockReleased {
                        client,
                        resources,
                        slot,
                    };
                }
            } else {
                self.app_lock.remaining_ms = self.app_lock.remaining_ms.saturating_sub(10);
            }
        }

        // 2. Telemetry and Squelch / Carrier detection
        let carrier = radio.is_carrier_open();
        let current_rssi = radio.get_rssi_dbm();
        let idx = self
            .listening_vfo
            .index()
            .min(self.rssi_moving_avg.len() - 1);
        self.rssi_moving_avg[idx] = (self.rssi_moving_avg[idx] * 3 + current_rssi) / 4;

        if carrier {
            self.activity_count[idx] = self.activity_count[idx].saturating_add(1);

            // Voice carrier priority override: if squelch opens while FSK sidecar is holding the channel,
            // immediately restore audio so the user hears voice
            if self.fsk_config.voice_priority_override
                && self.app_lock.client.id == ClientId::FSK_SIDECAR
            {
                if self.audio_path_muted {
                    radio.mute_audio(false);
                    self.audio_path_muted = false;
                }
            }

            if !self.carrier_active {
                self.carrier_active = true;
                if !self.is_transmitting {
                    bsp::set_status_led(bsp::StatusLed::Green);
                }
                if !self.app_lock.is_active() || !self.app_lock.audio_muted {
                    radio.mute_audio(false);
                    self.audio_path_muted = false;
                }
                self.dwell.on_carrier_opened();
                return RadioEvent::CarrierOpened(self.listening_vfo);
            } else {
                self.dwell.on_carrier_refresh();
                return RadioEvent::None;
            }
        }

        if !carrier && self.carrier_active {
            self.carrier_active = false;
            if !self.is_transmitting {
                bsp::set_status_led(bsp::StatusLed::Off);
            }
            radio.mute_audio(true);
            self.audio_path_muted = true;
            return RadioEvent::CarrierClosed(self.listening_vfo);
        }

        // 3. Post-carrier hold
        if self.dwell.is_rx_held() {
            self.dwell.tick_10ms();
            return RadioEvent::None;
        }

        // 4. Freeze Dual Watch dwelling if VFO is held by an application or FSK sidecar
        if self.app_lock.is_active() {
            return RadioEvent::None;
        }

        // 5. Dual Watch / Triple Watch time-division multiplexing
        let (dwell_ms, next_vfo) = match self.dual_watch {
            DualWatchMode::Off => return RadioEvent::None,
            DualWatchMode::EqualDwell { dwell_ms } | DualWatchMode::TripleWatch { dwell_ms } => {
                (dwell_ms, self.next_watch_vfo())
            }
            DualWatchMode::Priority {
                priority_vfo,
                check_interval_ms,
            } => {
                if self.listening_vfo != priority_vfo {
                    (check_interval_ms, priority_vfo)
                } else {
                    (100, priority_vfo.other())
                }
            }
            DualWatchMode::Adaptive => (
                DwellController::calc_adaptive_dwell(
                    self.rssi_moving_avg[idx],
                    self.activity_count[idx],
                ),
                self.next_watch_vfo(),
            ),
        };

        if self.dwell.is_dwell_expired(dwell_ms) {
            self.dwell.reset_dwell();
            self.listening_vfo = next_vfo;
            radio.configure_channel(&self.channels[next_vfo.index()]);
            RadioEvent::DualWatchSwitched(next_vfo)
        } else {
            self.dwell.tick_10ms();
            RadioEvent::None
        }
    }
}
