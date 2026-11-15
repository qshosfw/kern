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

pub trait RadioMelodyExt {
    fn play_beep(&mut self, beep: BeepType);
    fn play_roger(&mut self, roger: RogerType);
    fn play_dialog_sound(&mut self, sound: DialogSoundType);
}

/// System RF state holding live RSSI telemetry and the Multi-VFO `RadioCoordinator`.
#[derive(Debug, Clone)]
pub struct RadioState {
    pub rssi_dbm: i16,
    pub coord: RadioCoordinator,
}

impl RadioState {
    pub const fn new() -> Self {
        Self {
            rssi_dbm: -120,
            coord: RadioCoordinator::new(),
        }
    }

    #[inline(never)]
    pub unsafe fn init_in_place(ptr: *mut Self) {
        unsafe {
            core::ptr::write_bytes(ptr as *mut u8, 0, core::mem::size_of::<Self>());
            let s = &mut *ptr;
            s.rssi_dbm = -120;
            s.coord.init_zeroed();
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
            let notes = crate::services::melody::ROGER_DEFS
                .get(idx)
                .map_or(crate::services::melody::ROGER_CLASSIC_FALLBACK, |d| d.notes);
            drivers::bk4829::Bk4829::play_tone_sequence(self, notes);
        }
    }

    fn play_dialog_sound(&mut self, sound: DialogSoundType) {
        let notes = MelodyEngine::dialog_notes(sound);
        if !notes.is_empty() {
            drivers::bk4829::Bk4829::play_tone_sequence(self, notes);
        }
    }
}
