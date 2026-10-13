pub use crate::services::melody::{
    BeepType, DialogSoundType, MelodyBuffer, MelodyEngine, RogerType,
};
pub use drivers::audio::{ToneNote, VOLUME_TABLE};

pub use kernel::service::{AudioOutputTarget, AudioPriority, AudioService, AudioSession};

/// Hardware-independent Audio Session Arbiter managing amplifier gating and priority channels.
#[derive(Debug, Clone, Copy)]
pub struct AudioSessionManager {
    current_priority: AudioPriority,
    current_target: AudioOutputTarget,
    is_muted: bool,
    master_volume: u8,
}

impl AudioSessionManager {
    pub const fn new() -> Self {
        Self {
            current_priority: AudioPriority::Idle,
            current_target: AudioOutputTarget::Speaker,
            is_muted: false,
            master_volume: 5,
        }
    }

    /// Try to acquire an audio session at the specified priority.
    /// Returns true if granted (priority >= current_priority).
    pub fn request_session<A: AudioService>(
        &mut self,
        audio: Option<&mut A>,
        priority: AudioPriority,
        target: AudioOutputTarget,
    ) -> bool {
        if priority >= self.current_priority {
            self.current_priority = priority;
            self.current_target = target;
            if let Some(amp) = audio {
                if priority == AudioPriority::Mute {
                    amp.disable();
                } else if self.master_volume > 0 {
                    amp.enable();
                }
            }
            true
        } else {
            false
        }
    }

    /// Release an audio session, falling back to Idle.
    pub fn release_session<A: AudioService>(
        &mut self,
        audio: Option<&mut A>,
        priority: AudioPriority,
    ) {
        if self.current_priority <= priority {
            self.current_priority = AudioPriority::Idle;
            if let Some(amp) = audio {
                amp.disable();
            }
        }
    }

    pub fn current_priority(&self) -> AudioPriority {
        self.current_priority
    }

    pub fn is_speaker_active(&self) -> bool {
        self.current_priority > AudioPriority::Mute && !self.is_muted
    }

    pub fn set_master_volume(&mut self, vol: u8) {
        self.master_volume = vol;
    }

    /// Temporarily mute audio amplifier during frequency hop, retune, or packet decode.
    pub fn with_muted<A: AudioService, R, F: FnOnce() -> R>(
        &mut self,
        mut audio: Option<&mut A>,
        f: F,
    ) -> R {
        let prev_prio = self.current_priority;
        let prev_target = self.current_target;
        self.request_session(
            audio.as_deref_mut(),
            AudioPriority::Mute,
            AudioOutputTarget::Speaker,
        );
        let res = f();
        self.current_priority = prev_prio;
        self.current_target = prev_target;
        if let Some(amp) = audio {
            if prev_prio > AudioPriority::Mute && self.master_volume > 0 {
                amp.enable();
            } else {
                amp.disable();
            }
        }
        res
    }
}
