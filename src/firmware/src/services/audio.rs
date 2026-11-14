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
        let prev_muted = self.is_muted;
        self.is_muted = true;
        if let Some(amp) = audio.as_deref_mut() {
            amp.disable();
        }
        let res = f();
        self.is_muted = prev_muted;
        if let Some(amp) = audio {
            if !self.is_muted
                && self.current_priority > AudioPriority::Mute
                && self.master_volume > 0
            {
                amp.enable();
            } else {
                amp.disable();
            }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockAudio {
        enabled: bool,
        vol: u8,
        disable_count: u32,
        enable_count: u32,
    }

    impl MockAudio {
        fn new() -> Self {
            Self {
                enabled: false,
                vol: 5,
                disable_count: 0,
                enable_count: 0,
            }
        }
    }

    impl AudioService for MockAudio {
        fn set_volume(&mut self, level: u8) {
            self.vol = level;
        }
        fn volume(&self) -> u8 {
            self.vol
        }
        fn enable(&mut self) {
            self.enabled = true;
            self.enable_count += 1;
        }
        fn disable(&mut self) {
            self.enabled = false;
            self.disable_count += 1;
        }
        fn is_enabled(&self) -> bool {
            self.enabled
        }
        fn is_active(&self) -> bool {
            self.enabled
        }
        fn hardware_gain(&self) -> (u8, u8) {
            (0, self.vol)
        }
    }

    #[test]
    fn with_muted_silences_amp_even_during_higher_priority_session() {
        let mut mgr = AudioSessionManager::new();
        let mut amp = MockAudio::new();

        assert!(mgr.request_session(
            Some(&mut amp),
            AudioPriority::RxVoice,
            AudioOutputTarget::Speaker
        ));
        assert!(amp.is_enabled());
        assert!(mgr.is_speaker_active());

        let muted_inside = mgr.with_muted(Some(&mut amp), || {
            // Hardware amp must have been disabled despite RxVoice priority
            false
        });
        assert!(!muted_inside);
        assert!(amp.disable_count >= 1);
        assert!(amp.is_enabled());
        assert_eq!(mgr.current_priority(), AudioPriority::RxVoice);
        assert!(mgr.is_speaker_active());
    }
}
