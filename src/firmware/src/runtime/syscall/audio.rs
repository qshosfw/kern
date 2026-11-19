//! Audio Subsystem System Call Dispatcher (`Op::Aud*`, domain `0x5`).

#[cfg(test)]
use super::active_ctx_opt;
#[cfg(not(test))]
use crate::services::radio::RadioMelodyExt;

#[inline(never)]
pub unsafe fn dispatch_aud(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    if idx == 0 {
        let dur = a1 as u16;
        if dur == 0xFFFF {
            let beep = crate::services::melody::BeepType::from_u8(a0 as u8);
            #[cfg(not(test))]
            crate::kern_hw().radio.play_beep(beep);
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.play_beep(beep);
            }
        } else {
            #[cfg(not(test))]
            crate::kern_hw().radio.play_tone(a0 as u16, dur);
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.play_tone(a0 as u16, dur);
            }
        }
        return 0;
    }

    if idx != 1 {
        return 0;
    }

    match a0 as u8 {
        0 => {
            let on = a1 != 0;
            #[cfg(not(test))]
            {
                let aud = &mut crate::kern_hw().audio;
                if on {
                    aud.enable();
                } else {
                    aud.disable();
                }
            }
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                if let Some(ref mut aud) = ctx.audio {
                    if on {
                        aud.enable();
                    } else {
                        aud.disable();
                    }
                }
            }
            0
        }
        1 => {
            let vol = a1 as u8;
            #[cfg(not(test))]
            {
                let hw = crate::kern_hw();
                hw.audio.set_volume(vol);
                hw.radio.set_volume_level(vol);
                crate::master_config().audio.volume = vol;
            }
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.set_volume(vol);
            }
            0
        }
        2 => crate::master_config_ref().audio.volume as usize,
        3 => {
            #[cfg(not(test))]
            {
                crate::kern_hw().audio.is_active() as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| {
                    ctx.audio.as_ref().is_some_and(|a| a.is_active()) as usize
                })
            }
        }
        4 => {
            #[cfg(not(test))]
            crate::kern_hw().radio.set_mic_gain(a1 as u8);
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.radio.set_mic_gain(a1 as u8);
            }
            0
        }
        5 => {
            #[cfg(not(test))]
            {
                crate::kern_hw()
                    .radio
                    .read_reg(drivers::bk4829::registers::REG_AUDIO_AMP) as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| {
                    ctx.radio
                        .read_reg(drivers::bk4829::registers::REG_AUDIO_AMP)
                        as usize
                })
            }
        }
        6 => usize::from(crate::audio_capture_mut().start_capture(a1 as u16).is_err()),
        7 => {
            let out = a1 as *mut i16;
            let max = a2 as u16;
            if !out.is_null() && max > 0 {
                let slice = unsafe { core::slice::from_raw_parts_mut(out, max as usize) };
                crate::audio_capture_mut().read_samples(slice)
            } else {
                0
            }
        }
        8 => {
            crate::audio_capture_mut().stop_capture();
            0
        }
        9 => crate::audio_capture_mut().is_capturing() as usize,
        _ => 0,
    }
}
