//! Audio Subsystem System Call Handler (`Op::Aud`, domain `0x5`).

pub unsafe fn op_aud(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::aud;
    match a0 as u8 {
        aud::TONE => {
            let freq_or_beep = (a0 >> 8) as u16;
            let dur = a1 as u16;
            if dur == 0xFFFF {
                crate::services::play_beep(crate::services::melody::BeepType::from_u8(
                    freq_or_beep as u8,
                ));
            } else {
                crate::services::play_tone(freq_or_beep, dur);
            }
            0
        }
        aud::CAP_START => usize::from(crate::audio_capture_mut().start_capture(a1 as u16).is_err()),
        aud::CAP_READ => {
            let max = a2 as u16 as usize;
            if (a1 & 1) == 0
                && let Some(bytes) = unsafe { super::ptr_slice_mut(a1 as *mut u8, max * 2) }
            {
                let slice =
                    unsafe { core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut i16, max) };
                crate::audio_capture_mut().read_samples(slice)
            } else {
                0
            }
        }
        aud::CAP_STOP => {
            crate::audio_capture_mut().stop_capture();
            0
        }
        _ => 0,
    }
}
