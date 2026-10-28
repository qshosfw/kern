//! Firmware Radio Subsystem Integration, VFO Channel State, and Audio Feedback.

pub mod coordinator;
pub mod dwell;
pub mod fastscan;
pub mod fsk_stream;
pub mod tone_service;

pub use coordinator::*;
pub use dwell::DwellController;
pub use fastscan::*;
pub use fsk_stream::*;
pub use tone_service::*;

use crate::services::melody::{BeepType, DialogSoundType, MelodyEngine, RogerType};
use utils::units::{Frequency, Hertz};

pub use kernel::service::radio::*;

pub trait RadioMelodyExt {
    fn play_beep(&mut self, beep: BeepType);
    fn play_roger(&mut self, roger: RogerType);
    fn play_dialog_sound(&mut self, sound: DialogSoundType);
}

/// State representation of a single VFO channel for display widgets and user editing.
#[derive(Debug, Clone, Copy)]
pub struct VfoChannelState {
    pub vfo_num: u8,
    pub channel_num: Option<u16>,
    pub channel_name: &'static str,
    pub frequency: Frequency<Hertz>,
    pub power: &'static str,
    pub bw: &'static str,
    pub mod_type: &'static str,
    pub rx_active: bool,
    pub tx_active: bool,
}

impl VfoChannelState {
    pub const fn new(
        vfo_num: u8,
        channel_num: Option<u16>,
        channel_name: &'static str,
        freq_hz: u32,
        power: &'static str,
        bw: &'static str,
        mod_type: &'static str,
    ) -> Self {
        Self {
            vfo_num,
            channel_num,
            channel_name,
            frequency: Frequency::new(freq_hz),
            power,
            bw,
            mod_type,
            rx_active: false,
            tx_active: false,
        }
    }
}

/// System configuration & VFO channel state.
#[derive(Debug, Clone)]
pub struct RadioState {
    pub vfo_a: VfoChannelState,
    pub vfo_b: VfoChannelState,
    pub vfo_c: VfoChannelState,
    pub active_vfo: u8, // 0 = VFO A, 1 = VFO B, 2 = VFO C
    pub squelch_level: u8,
    pub volume: u8,
    pub step_khz: u16,
    pub contrast: u8,
    pub rssi_dbm: i16,
    pub coord: RadioCoordinator,
    pub fastscan: FastScanEngine,
}

impl RadioState {
    pub const fn new() -> Self {
        Self {
            vfo_a: VfoChannelState::new(1, None, "MAIN VFO", 433_500_000, "H", "N", "FM"),
            vfo_b: VfoChannelState::new(2, Some(1), "RPT 2M", 145_500_000, "L", "W", "FM"),
            vfo_c: VfoChannelState::new(3, None, "CALL 2M", 144_800_000, "L", "N", "FM"),
            active_vfo: 0,
            squelch_level: 4,
            volume: 5,
            step_khz: 12,
            contrast: 31,
            rssi_dbm: -120,
            coord: RadioCoordinator::new(),
            fastscan: FastScanEngine::new(430_000_000, 25_000),
        }
    }

    pub fn selected_vfo(&self) -> &VfoChannelState {
        match self.active_vfo {
            0 => &self.vfo_a,
            1 => &self.vfo_b,
            _ => &self.vfo_c,
        }
    }

    pub fn selected_vfo_mut(&mut self) -> &mut VfoChannelState {
        match self.active_vfo {
            0 => &mut self.vfo_a,
            1 => &mut self.vfo_b,
            _ => &mut self.vfo_c,
        }
    }

    pub fn toggle_vfo(&mut self) {
        self.active_vfo = if self.active_vfo == 0 {
            1
        } else if self.active_vfo == 1 {
            2
        } else {
            0
        };
        self.coord.active_vfo = match self.active_vfo {
            0 => VfoSlot::VfoA,
            1 => VfoSlot::VfoB,
            _ => VfoSlot::VfoC,
        };
    }

    pub fn sync_from_coord(&mut self) {
        self.active_vfo = self.coord.active_vfo.index() as u8;

        let is_tx = self.coord.is_transmitting;
        let tx_slot = self.coord.effective_tx_slot();
        let app_lock_active = self.coord.app_lock.is_active();
        let app_lock_slot = self.coord.app_lock.slot;
        let listening = self.coord.listening_vfo;
        let carrier = self.coord.carrier_active;

        let vfos = [&mut self.vfo_a, &mut self.vfo_b, &mut self.vfo_c];
        let slots = [VfoSlot::VfoA, VfoSlot::VfoB, VfoSlot::VfoC];

        for (i, vfo) in vfos.into_iter().enumerate() {
            let ch = &self.coord.channels[i];
            let slot = slots[i];
            vfo.frequency = ch.effective_rx_freq();
            vfo.power = ch.power.as_str();
            vfo.bw = ch.bandwidth.as_str();
            vfo.mod_type = ch.modulation.as_str();
            let lock = app_lock_active && app_lock_slot == slot;
            vfo.rx_active = (listening == slot && carrier) || lock;
            vfo.tx_active = is_tx && tx_slot == slot;
        }
    }
}

impl<'a, State> RadioMelodyExt for drivers::bk4829::Bk4829<'a, State> {
    fn play_beep(&mut self, beep: BeepType) {
        let notes = MelodyEngine::beep_notes(beep);
        if !notes.is_empty() {
            drivers::bk4829::Bk4829::play_tone_sequence(self, notes);
        }
    }

    fn play_roger(&mut self, roger: RogerType) {
        if roger != RogerType::Off {
            let idx = (roger as usize).saturating_sub(1);
            let defs = crate::services::melody::ROGER_DEFS;
            if idx < defs.len() {
                drivers::bk4829::Bk4829::play_tone_sequence(self, defs[idx].notes);
            } else {
                drivers::bk4829::Bk4829::play_tone_sequence(
                    self,
                    crate::services::melody::ROGER_CLASSIC_FALLBACK,
                );
            }
        }
    }

    fn play_dialog_sound(&mut self, sound: DialogSoundType) {
        let notes = MelodyEngine::dialog_notes(sound);
        if !notes.is_empty() {
            drivers::bk4829::Bk4829::play_tone_sequence(self, notes);
        }
    }
}
