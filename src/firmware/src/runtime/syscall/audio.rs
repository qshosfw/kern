//! Audio Subsystem System Call Handlers (`CapAudioVTable`, `interface_id = 0x0003`).

use super::active_ctx_opt;
use kernel::proc::abi::CapAudioVTable;
use kernel::proc::primitives::*;

pub unsafe extern "C" fn api_audio_play_tone(freq_hz: u16, duration_ms: u16) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.play_tone(freq_hz, duration_ms);
    }
}

pub unsafe extern "C" fn api_audio_play_beep(beep_type: u8) {
    if let Some(ctx) = active_ctx_opt() {
        AudioPrimitive::play_beep(ctx, beep_type);
    }
}

pub unsafe extern "C" fn api_audio_set_path(on: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_audio_path(on != 0);
    }
}

pub unsafe extern "C" fn api_audio_capture_start(rate_hz: u16) -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.capture_start(rate_hz).is_ok() {
            0
        } else {
            1
        }
    } else {
        1
    }
}

pub unsafe extern "C" fn api_audio_capture_read(out_samples: *mut i16, max_samples: u16) -> u16 {
    if !out_samples.is_null() && max_samples > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let slice =
                unsafe { core::slice::from_raw_parts_mut(out_samples, max_samples as usize) };
            ctx.capture_read(slice) as u16
        } else {
            0
        }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_audio_capture_stop() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.capture_stop();
    }
}

pub unsafe extern "C" fn api_audio_set_volume(volume: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_volume(volume);
    }
}

pub unsafe extern "C" fn api_audio_get_volume() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.get_volume()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_audio_stop() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.stop_audio();
    }
}

pub unsafe extern "C" fn api_audio_is_playing() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_audio_playing() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_audio_set_mic_gain(gain: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_mic_gain(gain);
    }
}

pub unsafe extern "C" fn api_audio_read_mic_sample() -> u16 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.read_mic_sample()
    } else {
        0
    }
}

pub unsafe extern "C" fn api_audio_is_capturing() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.is_capturing() { 1 } else { 0 }
    } else {
        0
    }
}

pub static AUDIO_VTABLE: CapAudioVTable = CapAudioVTable {
    audio_play_tone: api_audio_play_tone,
    audio_play_beep: api_audio_play_beep,
    audio_set_path: api_audio_set_path,
    audio_capture_start: api_audio_capture_start,
    audio_capture_read: api_audio_capture_read,
    audio_capture_stop: api_audio_capture_stop,
    audio_set_volume: api_audio_set_volume,
    audio_get_volume: api_audio_get_volume,
    audio_stop: api_audio_stop,
    audio_is_playing: api_audio_is_playing,
    audio_set_mic_gain: api_audio_set_mic_gain,
    audio_read_mic_sample: api_audio_read_mic_sample,
    audio_is_capturing: api_audio_is_capturing,
};
