//! Firmware Radio Subsystem Integration, VFO Channel State, and Audio Feedback.

pub mod coordinator;
pub mod dwell;
pub mod fsk_stream;

pub use coordinator::*;
pub use dwell::DwellController;
pub use fsk_stream::*;

use crate::services::melody::{BeepType, DialogSoundType, MelodyEngine, RogerType};

pub trait RadioMelodyExt {
    fn play_beep(&mut self, beep: BeepType);
    fn play_roger(&mut self, roger: RogerType);
    fn play_dialog_sound(&mut self, sound: DialogSoundType);
}

/// System RF state holding live RSSI telemetry, the Multi-VFO `RadioCoordinator`,
/// and the non-blocking `ToneSequencer`.
#[derive(Debug, Clone)]
pub struct RadioState {
    pub rssi_dbm: i16,
    pub coord: RadioCoordinator,
    pub tone_seq: crate::services::melody::ToneSequencer,
}

impl RadioState {
    pub const fn new() -> Self {
        Self {
            rssi_dbm: -120,
            coord: RadioCoordinator::new(),
            tone_seq: crate::services::melody::ToneSequencer::new(),
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

#[inline(never)]
fn play_preset_notes<B: drivers::bk4829::Bk4829Bus>(
    radio: &mut drivers::bk4829::Bk4829<'_, B>,
    notes: &[drivers::audio::ToneNote],
) {
    if !notes.is_empty() {
        if crate::system::is_headless() {
            crate::system::push_headless_notes(notes);
            return;
        }
        drivers::bk4829::Bk4829::play_tone_sequence(radio, notes);
    }
}

impl<'a, B: drivers::bk4829::Bk4829Bus> RadioMelodyExt
    for drivers::bk4829::Bk4829<'a, B>
{
    fn play_beep(&mut self, beep: BeepType) {
        play_preset_notes(self, MelodyEngine::beep_notes(beep));
    }

    fn play_roger(&mut self, roger: RogerType) {
        play_preset_notes(self, MelodyEngine::roger_notes(roger));
    }

    fn play_dialog_sound(&mut self, sound: DialogSoundType) {
        play_preset_notes(self, MelodyEngine::dialog_notes(sound));
    }
}
