//! Audio Subsystem System Call Dispatcher (`Op::Aud*`, domain `0x5`).

#[cfg(test)]
use super::active_ctx_opt;

#[inline(never)]
pub unsafe fn dispatch_aud(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::{idx as op, sub::aud_ctl};
    if idx == op::AudTone {
        let dur = a1 as u16;
        if dur == 0xFFFF {
            let beep = crate::services::melody::BeepType::from_u8(a0 as u8);
            #[cfg(not(test))]
            crate::radio_state().tone_seq.enqueue(
                &mut crate::kern_hw().radio,
                crate::services::melody::MelodyEngine::beep_notes(beep),
            );
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.play_beep(beep);
            }
        } else {
            #[cfg(not(test))]
            {
                let note = [crate::services::melody::ToneNote::new(a0 as u16, dur)];
                crate::radio_state()
                    .tone_seq
                    .enqueue(&mut crate::kern_hw().radio, &note);
            }
            #[cfg(test)]
            if let Some(ctx) = active_ctx_opt() {
                ctx.play_tone(a0 as u16, dur);
            }
        }
        return 0;
    }

    if idx != op::AudCtl {
        return 0;
    }

    match a0 as u8 {
        aud_ctl::PATH => {
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
                if on {
                    ctx.audio.enable();
                } else {
                    ctx.audio.disable();
                }
            }
            0
        }
        aud_ctl::IS_PLAYING => {
            #[cfg(not(test))]
            {
                crate::kern_hw().audio.is_active() as usize
            }
            #[cfg(test)]
            {
                active_ctx_opt().map_or(0, |ctx| ctx.audio.is_active() as usize)
            }
        }
        aud_ctl::MIC_SAMPLE => {
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
        aud_ctl::CAP_START => {
            usize::from(crate::audio_capture_mut().start_capture(a1 as u16).is_err())
        }
        aud_ctl::CAP_READ => {
            let out = a1 as *mut i16;
            let max = a2 as u16;
            if !out.is_null() && max > 0 {
                let slice = unsafe { core::slice::from_raw_parts_mut(out, max as usize) };
                crate::audio_capture_mut().read_samples(slice)
            } else {
                0
            }
        }
        aud_ctl::CAP_STOP => {
            crate::audio_capture_mut().stop_capture();
            0
        }
        aud_ctl::IS_CAP => crate::audio_capture_mut().is_capturing() as usize,
        _ => 0,
    }
}
