//! Audio Subsystem System Call Handlers (`AudioApi`, `interface_id = 0x0003`).

#[cfg(test)]
use super::active_ctx_opt;
#[cfg(not(test))]
use crate::services::radio::RadioMelodyExt;
use kernel::proc::abi::AudioApi;

pub unsafe extern "C" fn api_audio_play_tone(freq_hz: u16, duration_ms: u16) {
    #[cfg(not(test))]
    crate::kern_hw().radio.play_tone(freq_hz, duration_ms);
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        ctx.play_tone(freq_hz, duration_ms);
    }
}

pub unsafe extern "C" fn api_audio_play_beep(beep_type: u8) {
    let beep = crate::services::melody::BeepType::from_u8(beep_type);
    #[cfg(not(test))]
    crate::kern_hw().radio.play_beep(beep);
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        ctx.play_beep(beep);
    }
}

pub unsafe extern "C" fn api_audio_set_path(on: u8) {
    #[cfg(not(test))]
    {
        let aud = &mut crate::kern_hw().audio;
        if on != 0 {
            aud.enable();
        } else {
            aud.disable();
        }
    }
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        if let Some(ref mut aud) = ctx.audio {
            if on != 0 {
                aud.enable();
            } else {
                aud.disable();
            }
        }
    }
}

pub unsafe extern "C" fn api_audio_capture_start(rate_hz: u16) -> u8 {
    u8::from(crate::audio_capture_mut().start_capture(rate_hz).is_err())
}

pub unsafe extern "C" fn api_audio_capture_read(out_samples: *mut i16, max_samples: u16) -> u16 {
    if !out_samples.is_null() && max_samples > 0 {
        let slice = unsafe { core::slice::from_raw_parts_mut(out_samples, max_samples as usize) };
        crate::audio_capture_mut().read_samples(slice) as u16
    } else {
        0
    }
}

pub unsafe extern "C" fn api_audio_capture_stop() {
    crate::audio_capture_mut().stop_capture();
}

pub unsafe extern "C" fn api_audio_set_volume(volume: u8) {
    #[cfg(not(test))]
    {
        let hw = crate::kern_hw();
        hw.audio.set_volume(volume);
        hw.radio.set_volume_level(volume);
        crate::master_config().audio.volume = volume;
    }
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_volume(volume);
    }
}

pub unsafe extern "C" fn api_audio_get_volume() -> u8 {
    crate::master_config_ref().audio.volume
}

pub unsafe extern "C" fn api_audio_stop() {
    unsafe { api_audio_set_path(0) }
}

pub unsafe extern "C" fn api_audio_is_playing() -> u8 {
    #[cfg(not(test))]
    {
        crate::kern_hw().audio.is_active() as u8
    }
    #[cfg(test)]
    {
        active_ctx_opt().map_or(0, |ctx| {
            ctx.audio.as_ref().is_some_and(|a| a.is_active()) as u8
        })
    }
}

pub unsafe extern "C" fn api_audio_set_mic_gain(gain: u8) {
    #[cfg(not(test))]
    crate::kern_hw().radio.set_mic_gain(gain);
    #[cfg(test)]
    if let Some(ctx) = active_ctx_opt() {
        ctx.radio.set_mic_gain(gain);
    }
}

pub unsafe extern "C" fn api_audio_read_mic_sample() -> u16 {
    #[cfg(not(test))]
    {
        crate::kern_hw()
            .radio
            .read_reg(drivers::bk4829::registers::REG_AUDIO_AMP)
    }
    #[cfg(test)]
    {
        active_ctx_opt().map_or(0, |ctx| {
            ctx.radio
                .read_reg(drivers::bk4829::registers::REG_AUDIO_AMP)
        })
    }
}

pub unsafe extern "C" fn api_audio_is_capturing() -> u8 {
    crate::audio_capture_mut().is_capturing() as u8
}

pub static AUDIO_VTABLE: AudioApi = AudioApi {
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
