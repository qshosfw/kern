//! Firmware Radio Subsystem Integration, VFO Channel State, and Audio Feedback.

pub mod beacon;
pub mod coordinator;
pub mod dwell;
pub mod fastscan;
pub mod fsk_protocol;
pub mod fsk_stream;
pub mod tone_service;

pub use beacon::*;
pub use coordinator::*;
pub use dwell::DwellController;
pub use fastscan::*;
pub use fsk_protocol::*;
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
    pub beacon: BeaconEngine,
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
            beacon: BeaconEngine::new(),
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
        self.vfo_a.frequency = self.coord.channels[0].effective_rx_freq();
        self.vfo_b.frequency = self.coord.channels[1].effective_rx_freq();
        self.vfo_c.frequency = self.coord.channels[2].effective_rx_freq();

        self.vfo_a.power = self.coord.channels[0].power.as_str();
        self.vfo_b.power = self.coord.channels[1].power.as_str();
        self.vfo_c.power = self.coord.channels[2].power.as_str();

        self.vfo_a.bw = self.coord.channels[0].bandwidth.as_str();
        self.vfo_b.bw = self.coord.channels[1].bandwidth.as_str();
        self.vfo_c.bw = self.coord.channels[2].bandwidth.as_str();

        self.vfo_a.mod_type = self.coord.channels[0].modulation.as_str();
        self.vfo_b.mod_type = self.coord.channels[1].modulation.as_str();
        self.vfo_c.mod_type = self.coord.channels[2].modulation.as_str();

        self.active_vfo = self.coord.active_vfo.index() as u8;

        let lock_a = self.coord.app_lock.is_active() && self.coord.app_lock.slot == VfoSlot::VfoA;
        let lock_b = self.coord.app_lock.is_active() && self.coord.app_lock.slot == VfoSlot::VfoB;
        let lock_c = self.coord.app_lock.is_active() && self.coord.app_lock.slot == VfoSlot::VfoC;

        self.vfo_a.rx_active =
            (self.coord.listening_vfo == VfoSlot::VfoA && self.coord.carrier_active) || lock_a;
        self.vfo_b.rx_active =
            (self.coord.listening_vfo == VfoSlot::VfoB && self.coord.carrier_active) || lock_b;
        self.vfo_c.rx_active =
            (self.coord.listening_vfo == VfoSlot::VfoC && self.coord.carrier_active) || lock_c;

        let is_tx = self.coord.is_transmitting;
        let tx_slot = self.coord.effective_tx_slot();
        self.vfo_a.tx_active = is_tx && tx_slot == VfoSlot::VfoA;
        self.vfo_b.tx_active = is_tx && tx_slot == VfoSlot::VfoB;
        self.vfo_c.tx_active = is_tx && tx_slot == VfoSlot::VfoC;
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
            drivers::bk4829::Bk4829::play_tone_sequence(
                self,
                crate::services::melody::ROGER_CLASSIC_FALLBACK,
            );
        }
    }

    fn play_dialog_sound(&mut self, sound: DialogSoundType) {
        let notes = MelodyEngine::dialog_notes(sound);
        if !notes.is_empty() {
            drivers::bk4829::Bk4829::play_tone_sequence(self, notes);
        }
    }
}
