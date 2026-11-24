//! Modular Tone & Audio Sequence Composition Engine.
//!
//! Provides zero-alloc dynamic tone sequencing, rich presets (Roger beeps, keypress beeps,
//! alerts), and custom user-configurable tones decoupled from the hardware driver layer.

pub mod format;
pub mod presets;
pub mod types;

pub use format::*;
pub use presets::*;
pub use types::*;

use crate::services::SystemStorage;
use kernel::service::AudioPriority;

/// Maximum notes buffered in the non-blocking `ToneSequencer`.
pub const SEQUENCER_MAX_NOTES: usize = 8;

/// Non-blocking deadline state machine phase for tone / PA warm-up playback.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TonePhase {
    Idle = 0,
    PaWarmup = 1,
    NoteOn = 2,
    Teardown = 3,
}

/// Hardware contract for driving the non-blocking `ToneSequencer` (implemented by `Bk4829` & mock tests).
pub trait ToneHardware {
    fn tone_gain(&self) -> u8;
    fn capture_rx_pwr(&mut self) -> u16;
    fn begin_tone_sequence(&mut self) -> bool;
    fn step_tone_note(&mut self, freq_hz: u16);
    fn mute_tone_note(&mut self);
    fn prepare_tone_teardown(&mut self);
    fn finish_tone_teardown(&mut self, rx_pwr: u16);
}

impl<'a, B: drivers::bk4829::Bk4829Bus, State> ToneHardware
    for drivers::bk4829::Bk4829<'a, B, State>
{
    #[inline(always)]
    fn tone_gain(&self) -> u8 {
        drivers::bk4829::Bk4829::tone_gain(self)
    }
    #[inline(always)]
    fn capture_rx_pwr(&mut self) -> u16 {
        drivers::bk4829::Bk4829::capture_rx_pwr(self)
    }
    #[inline(always)]
    fn begin_tone_sequence(&mut self) -> bool {
        drivers::bk4829::Bk4829::begin_tone_sequence(self)
    }
    #[inline(always)]
    fn step_tone_note(&mut self, freq_hz: u16) {
        drivers::bk4829::Bk4829::step_tone_note(self, freq_hz);
    }
    #[inline(always)]
    fn mute_tone_note(&mut self) {
        drivers::bk4829::Bk4829::mute_tone_note(self);
    }
    #[inline(always)]
    fn prepare_tone_teardown(&mut self) {
        drivers::bk4829::Bk4829::prepare_tone_teardown(self);
    }
    #[inline(always)]
    fn finish_tone_teardown(&mut self, rx_pwr: u16) {
        drivers::bk4829::Bk4829::finish_tone_teardown(self, rx_pwr);
    }
}

/// Deadline-driven, zero-stall tone & PA warm-up sequencer (`0 ms` blocking in event dispatch).
///
/// Hot 1-byte/2-byte control fields sit in bytes `0..12` for Cortex-M0+ Thumb-1 `imm5` reach.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct ToneSequencer {
    phase: TonePhase,
    idx: u8,
    len: u8,
    priority: u8,
    rx_pwr: u16,
    _pad: u16,
    deadline_ms: u32,
    notes: [ToneNote; SEQUENCER_MAX_NOTES],
}

impl ToneSequencer {
    pub const fn new() -> Self {
        Self {
            phase: TonePhase::Idle,
            idx: 0,
            len: 0,
            priority: 0,
            rx_pwr: 0x3FF1,
            _pad: 0,
            deadline_ms: 0,
            notes: [ToneNote::new(0, 0); SEQUENCER_MAX_NOTES],
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.phase != TonePhase::Idle
    }

    #[inline(never)]
    fn start_current_note<H: ToneHardware>(&mut self, hw: &mut H, now_ms: u32) {
        let note = self.notes[self.idx as usize];
        if note.freq_hz == 0 || note.duration_ms == 0 {
            hw.mute_tone_note();
            hw.step_tone_note(0);
        } else {
            hw.step_tone_note(note.freq_hz);
        }
        self.phase = TonePhase::NoteOn;
        self.deadline_ms = now_ms.wrapping_add(note.duration_ms as u32);
    }

    /// Enqueue a note sequence with explicit priority and timestamp (testable / host-deterministic).
    #[inline(never)]
    pub fn enqueue_at<H: ToneHardware>(
        &mut self,
        hw: &mut H,
        priority: AudioPriority,
        notes: &[ToneNote],
        now_ms: u32,
    ) {
        if notes.is_empty() {
            return;
        }
        if crate::system::is_headless() {
            crate::system::push_headless_notes(notes);
            return;
        }
        if hw.tone_gain() == 0 {
            return;
        }
        let prio = priority as u8;
        if self.is_active() && prio < self.priority {
            return;
        }
        let count = notes.len().min(SEQUENCER_MAX_NOTES);
        self.notes[..count].copy_from_slice(&notes[..count]);
        self.len = count as u8;
        self.idx = 0;
        self.priority = prio;
        if !self.is_active() {
            self.rx_pwr = hw.capture_rx_pwr();
        }
        if !hw.begin_tone_sequence() {
            self.phase = TonePhase::PaWarmup;
            self.deadline_ms = now_ms.wrapping_add(50);
        } else {
            self.start_current_note(hw, now_ms);
        }
    }

    /// Compact 4-register AAPCS entry point (`r0..r3`, zero stack spills) for UI/system tones.
    #[inline(never)]
    pub fn enqueue<H: ToneHardware>(&mut self, hw: &mut H, notes: &[ToneNote]) {
        self.enqueue_at(hw, AudioPriority::UiTone, notes, py32_hal::delay::millis());
    }

    /// Load a roger melody file directly into the sequencer buffer (zero stack array) and play.
    #[inline(never)]
    pub fn enqueue_roger_file<H: ToneHardware>(
        &mut self,
        hw: &mut H,
        storage: &mut SystemStorage,
        path: &str,
    ) {
        let mut buf = [ToneNote::new(0, 0); SEQUENCER_MAX_NOTES];
        let n = load_roger_notes_slice(storage, path, &mut buf);
        if n > 0 {
            self.enqueue_at(
                hw,
                AudioPriority::TxRoger,
                &buf[..n],
                py32_hal::delay::millis(),
            );
        }
    }

    /// Advance the sequencer when `now_ms` reaches `deadline_ms`. Safe across `u32` wraparound.
    #[inline(never)]
    pub fn tick<H: ToneHardware>(&mut self, hw: &mut H, now_ms: u32) {
        if self.phase == TonePhase::Idle {
            return;
        }
        if (now_ms.wrapping_sub(self.deadline_ms) as i32) < 0 {
            return;
        }
        match self.phase {
            TonePhase::Idle => {}
            TonePhase::PaWarmup => {
                self.start_current_note(hw, now_ms);
            }
            TonePhase::NoteOn => {
                hw.mute_tone_note();
                self.idx += 1;
                if self.idx < self.len {
                    self.start_current_note(hw, now_ms);
                } else {
                    hw.prepare_tone_teardown();
                    self.phase = TonePhase::Teardown;
                    self.deadline_ms = now_ms.wrapping_add(5);
                }
            }
            TonePhase::Teardown => {
                hw.finish_tone_teardown(self.rx_pwr);
                self.phase = TonePhase::Idle;
                self.priority = 0;
            }
        }
    }

    /// Drain an active cue synchronously before deep-sleep clock shutdown.
    #[inline(never)]
    pub fn drain_blocking<H: ToneHardware>(&mut self, hw: &mut H) {
        while self.is_active() {
            py32_hal::wdt::Watchdog::feed();
            self.tick(hw, py32_hal::delay::millis());
            cortex_m::asm::wfi();
        }
    }

    /// Immediately silence any in-flight tone and restore RX power/AF state (e.g. on PTT TX start).
    #[inline(never)]
    pub fn abort<H: ToneHardware>(&mut self, hw: &mut H) {
        if self.is_active() {
            hw.mute_tone_note();
            hw.prepare_tone_teardown();
            hw.finish_tone_teardown(self.rx_pwr);
            self.phase = TonePhase::Idle;
            self.priority = 0;
        }
    }
}

#[inline(never)]
fn read_kt_file(
    storage: &mut SystemStorage,
    path: &str,
    allow_krgr: bool,
    out: &mut [ToneNote],
) -> usize {
    if path.is_empty() {
        return 0;
    }
    #[cfg(not(test))]
    {
        let Some(staging) = crate::data::arena::try_staging_buf() else {
            return 0;
        };
        let raw_buf = &mut staging[..140];
        let Ok(n) = storage.read_file(path, raw_buf) else {
            return 0;
        };
        parse_kt_bytes(&raw_buf[..n], allow_krgr, out)
    }
    #[cfg(test)]
    {
        let mut raw_buf = [0u8; 140];
        let Ok(n) = storage.read_file(path, &mut raw_buf) else {
            return 0;
        };
        parse_kt_bytes(&raw_buf[..n], allow_krgr, out)
    }
}

fn parse_kt_bytes(raw: &[u8], allow_krgr: bool, out: &mut [ToneNote]) -> usize {
    let n = raw.len();
    let (mut pos, count_raw) = if n >= 4 && &raw[0..2] == b"KT" && raw[2] == 1 {
        let count = if raw[3] > 0 {
            raw[3] as usize
        } else {
            (n - 4) >> 2
        };
        (4usize, count)
    } else if allow_krgr && n >= 16 && &raw[0..4] == b"KRGR" {
        (16usize, raw[5] as usize)
    } else {
        return 0;
    };
    let count = count_raw.min(out.len());
    let mut written = 0usize;
    while pos + 4 <= n && written < count {
        let freq = u16::from_be_bytes([raw[pos], raw[pos + 1]]);
        let dur = u16::from_be_bytes([raw[pos + 2], raw[pos + 3]]);
        out[written] = ToneNote::new(freq, dur);
        written += 1;
        pos += 4;
    }
    written
}

fn fill_notes(slice: &[ToneNote], out: &mut [ToneNote]) -> usize {
    let count = slice.len().min(out.len());
    let mut i = 0;
    while i < count {
        out[i] = slice[i];
        i += 1;
    }
    count
}

#[inline(never)]
pub fn load_roger_notes_slice(
    storage: &mut SystemStorage,
    path: &str,
    out: &mut [ToneNote],
) -> usize {
    let n = read_kt_file(storage, path, true, out);
    if n > 0 {
        return n;
    }
    fill_notes(pool_slice(RANGE_ROGER_CLASSIC), out)
}

/// Load roger melody notes into `out` from storage path, with fallback to classic default.
#[inline(never)]
pub fn load_roger_notes(
    storage: &mut SystemStorage,
    path: &str,
    out: &mut [ToneNote; MAX_MELODY_NOTES],
) -> usize {
    load_roger_notes_slice(storage, path, out)
}

/// Load jingle melody notes into `out` from storage path, or fallback to selected preset.
#[inline(never)]
pub fn load_jingle_notes(
    storage: &mut SystemStorage,
    boot_jingle_mode: u8,
    path: &str,
    out: &mut [ToneNote; MAX_MELODY_NOTES],
) -> usize {
    if boot_jingle_mode == 0 {
        return 0;
    }
    let n = read_kt_file(storage, path, false, out);
    if n > 0 {
        return n;
    }
    fill_notes(
        MelodyEngine::boot_jingle_notes(BootJingleType::from_u8(boot_jingle_mode)),
        out,
    )
}

#[cfg(all(feature = "filesystem", feature = "boot_jingle"))]
fn write_provision_ton(storage: &mut SystemStorage, path: &str, name: &str, notes: &[ToneNote]) {
    if !storage.exists(path) {
        let mut buf = [0u8; 160];
        let max_notes = ((buf.len() - 4) / 4).min(MAX_MELODY_NOTES);
        let note_count = notes.len().min(max_notes);
        buf[0] = b'K';
        buf[1] = b'T';
        buf[2] = 1;
        buf[3] = note_count as u8;
        let mut pos = 4;
        for note in &notes[..note_count] {
            buf[pos..pos + 2].copy_from_slice(&note.freq_hz.to_be_bytes());
            buf[pos + 2..pos + 4].copy_from_slice(&note.duration_ms.to_be_bytes());
            pos += 4;
        }
        let max_name = buf.len().saturating_sub(pos + 2);
        let name_len = name.len().min(max_name).min(u8::MAX as usize);
        if name_len > 0 {
            buf[pos] = TAG_NAME;
            buf[pos + 1] = name_len as u8;
            pos += 2;
            buf[pos..pos + name_len].copy_from_slice(&name.as_bytes()[..name_len]);
            pos += name_len;
        }
        let _ = storage.write_file(path, &buf[..pos]);
    }
}

#[cfg(all(feature = "filesystem", feature = "boot_jingle"))]
fn build_tone_path<'a>(
    buf: &'a mut [u8; 32],
    dir_prefix: &[u8],
    file_name: &str,
) -> Option<&'a str> {
    let fname = file_name.as_bytes();
    let total = dir_prefix.len().checked_add(fname.len())?;
    if total > buf.len() {
        return None;
    }
    buf[..dir_prefix.len()].copy_from_slice(dir_prefix);
    buf[dir_prefix.len()..total].copy_from_slice(fname);
    utils::format::ascii_str(&buf[..total])
}

#[cfg(not(all(feature = "filesystem", feature = "boot_jingle")))]
pub fn provision_jingles(_storage: &mut SystemStorage) {}

/// Provision default boot jingle tone files into LittleFS (/user/jingles)
#[cfg(all(feature = "filesystem", feature = "boot_jingle"))]
pub fn provision_jingles(storage: &mut SystemStorage) {
    let _ = storage.create_dir("/user/jingles");
    let mut path_buf = [0u8; 32];

    for def in JINGLE_DEFS {
        if let Some(path) = build_tone_path(&mut path_buf, b"/user/jingles/", def.file_name) {
            write_provision_ton(storage, path, def.name, def.notes);
        }
    }
}

pub struct MelodyEngine;

impl MelodyEngine {
    /// Return the const note slice for a built-in beep preset.
    pub fn beep_notes(beep: BeepType) -> &'static [ToneNote] {
        pool_slice(BEEP_RANGES[beep as usize])
    }

    /// Return the const note slice for a dialog/notification sound.
    pub fn dialog_notes(sound: DialogSoundType) -> &'static [ToneNote] {
        pool_slice(DIALOG_RANGES[sound as usize])
    }

    /// Return the const note slice for a built-in roger preset.
    pub fn roger_notes(roger: RogerType) -> &'static [ToneNote] {
        match roger {
            RogerType::Off => &[],
            RogerType::Tiny => pool_slice(RANGE_ROGER_TINY),
            #[cfg(feature = "extra_rogers")]
            other => {
                let idx = (other as usize).saturating_sub(3);
                EXTRA_ROGER_NOTES
                    .get(idx)
                    .copied()
                    .unwrap_or_else(|| pool_slice(RANGE_ROGER_CLASSIC))
            }
            #[cfg(not(feature = "extra_rogers"))]
            _ => pool_slice(RANGE_ROGER_CLASSIC),
        }
    }

    /// Return the const note slice for a boot jingle.
    pub fn boot_jingle_notes(jingle: BootJingleType) -> &'static [ToneNote] {
        match jingle {
            BootJingleType::Off => &[],
            #[cfg(feature = "boot_jingle")]
            BootJingleType::Nokia => JINGLE_NOKIA,
            #[cfg(feature = "boot_jingle")]
            BootJingleType::Chirp => JINGLE_CHIRP,
            #[cfg(feature = "boot_jingle")]
            BootJingleType::GameBoy => JINGLE_GAMEBOY,
            _ => pool_slice(RANGE_JINGLE_CHIME),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kt_bytes_supports_dynamic_and_explicit_counts() {
        // Explicit count = 2
        let explicit = [
            b'K', b'T', 1, 2, 0x03, 0xE8, 0x00, 0x32, 0x07, 0xD0, 0x00, 0x64,
        ];
        let mut out = [ToneNote::new(0, 0); MAX_MELODY_NOTES];
        assert_eq!(parse_kt_bytes(&explicit, false, &mut out), 2);
        assert_eq!(out[0], ToneNote::new(1000, 50));
        assert_eq!(out[1], ToneNote::new(2000, 100));

        // Dynamic count = 0 derives note count from remaining 4-byte pairs
        let dynamic = [
            b'K', b'T', 1, 0, 0x01, 0xB8, 0x00, 0x28, 0x02, 0x58, 0x00, 0x50,
        ];
        assert_eq!(parse_kt_bytes(&dynamic, false, &mut out), 2);
        assert_eq!(out[0], ToneNote::new(440, 40));
        assert_eq!(out[1], ToneNote::new(600, 80));
    }

    struct MockToneHw {
        gain: u8,
        amp_active: bool,
        last_freq: u16,
        muted: u8,
        prepared: u8,
        finished: u8,
        restored_pwr: u16,
    }

    impl MockToneHw {
        fn new(amp_active: bool) -> Self {
            Self {
                gain: 80,
                amp_active,
                last_freq: 0,
                muted: 0,
                prepared: 0,
                finished: 0,
                restored_pwr: 0,
            }
        }
    }

    impl ToneHardware for MockToneHw {
        fn tone_gain(&self) -> u8 {
            self.gain
        }
        fn capture_rx_pwr(&mut self) -> u16 {
            0x3FF1
        }
        fn begin_tone_sequence(&mut self) -> bool {
            let was = self.amp_active;
            self.amp_active = true;
            was
        }
        fn step_tone_note(&mut self, freq_hz: u16) {
            self.last_freq = freq_hz;
        }
        fn mute_tone_note(&mut self) {
            self.muted += 1;
        }
        fn prepare_tone_teardown(&mut self) {
            self.prepared += 1;
        }
        fn finish_tone_teardown(&mut self, rx_pwr: u16) {
            self.finished += 1;
            self.restored_pwr = rx_pwr;
        }
    }

    #[test]
    fn tone_sequencer_cold_pa_warmup_and_teardown_are_non_blocking() {
        let mut seq = ToneSequencer::new();
        let mut hw = MockToneHw::new(false);
        let notes = [ToneNote::new(880, 40), ToneNote::new(1175, 25)];

        seq.enqueue_at(&mut hw, AudioPriority::UiTone, &notes, 1000);
        assert!(seq.is_active());
        assert_eq!(hw.last_freq, 0); // Still in 50ms cold PA warmup

        // Before 50ms warmup deadline: no note started yet
        seq.tick(&mut hw, 1049);
        assert_eq!(hw.last_freq, 0);

        // At 1050ms: note 0 starts
        seq.tick(&mut hw, 1050);
        assert_eq!(hw.last_freq, 880);

        // At 1090ms: note 0 ends, note 1 starts
        seq.tick(&mut hw, 1090);
        assert_eq!(hw.last_freq, 1175);
        assert_eq!(hw.muted, 1);

        // At 1115ms: note 1 ends, enters 5ms teardown
        seq.tick(&mut hw, 1115);
        assert_eq!(hw.prepared, 1);
        assert_eq!(hw.finished, 0);
        assert!(seq.is_active());

        // At 1120ms: teardown completes, sequencer idle
        seq.tick(&mut hw, 1120);
        assert_eq!(hw.finished, 1);
        assert_eq!(hw.restored_pwr, 0x3FF1);
        assert!(!seq.is_active());
    }

    #[test]
    fn tone_sequencer_priority_preemption_and_ptt_abort() {
        let mut seq = ToneSequencer::new();
        let mut hw = MockToneHw::new(true); // Warm PA: immediate note start
        let roger = [ToneNote::new(500, 60)];
        let beep = [ToneNote::new(880, 40)];

        seq.enqueue_at(&mut hw, AudioPriority::TxRoger, &roger, 100);
        assert_eq!(hw.last_freq, 500);

        // Lower-priority UiTone must not preempt TxRoger
        seq.enqueue_at(&mut hw, AudioPriority::UiTone, &beep, 110);
        assert_eq!(hw.last_freq, 500);

        // Higher-priority EmergencyAlert preempts cleanly
        seq.enqueue_at(&mut hw, AudioPriority::EmergencyAlert, &beep, 120);
        assert_eq!(hw.last_freq, 880);

        // PTT abort silences immediately and restores RX power
        seq.abort(&mut hw);
        assert!(!seq.is_active());
        assert_eq!(hw.finished, 1);
        assert_eq!(hw.restored_pwr, 0x3FF1);
    }
}
