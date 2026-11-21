//! Dual-VFO Monochrome Transceiver HUD Rendering Primitives.

use kern_sdk::{
    ALIGN_LEFT, ALIGN_RIGHT, COLOR_CLEAR, COLOR_FILL, FONT_MUMATRIX, FONT_TOMTHUMB,
    KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_HAS_SIGNAL, KRX_STATUS_VISIBLE,
    KrxApiTable, KrxRadioState, KrxStatusBar, div_rem_10, div_rem_10_u16, nth_packed_slice,
};

const MOD_NAMES: &[u8] = b"FM\0AM\0USB\0";
const STEP_NAMES: &[u8] = b"2.5k\05k\06.25k\010k\012.5k\025k\050k\0";

/// Format `freq_hz` as `"145.500.00"` (10 ASCII bytes) without division or `core::fmt`.
#[inline(never)]
pub fn format_freq_hz(freq_hz: u32, out: &mut [u8; 10]) {
    // Convert Hz to 10 Hz units via fixed-point reciprocal: (hz * 3435973837) >> 35 ≈ hz / 10
    // Simpler exact u32 split: extract MHz, kHz, and 10Hz via subtraction loops (at most ~13 iters total).
    let mut rem = freq_hz;
    let mut mhz = 0u16;
    while rem >= 100_000_000 {
        rem -= 100_000_000;
        mhz += 100;
    }
    while rem >= 10_000_000 {
        rem -= 10_000_000;
        mhz += 10;
    }
    while rem >= 1_000_000 {
        rem -= 1_000_000;
        mhz += 1;
    }
    let mut khz = 0u16;
    while rem >= 100_000 {
        rem -= 100_000;
        khz += 100;
    }
    while rem >= 10_000 {
        rem -= 10_000;
        khz += 10;
    }
    while rem >= 1_000 {
        rem -= 1_000;
        khz += 1;
    }
    let mut tens = 0u8;
    while rem >= 100 {
        rem -= 100;
        tens += 10;
    }
    while rem >= 10 {
        rem -= 10;
        tens += 1;
    }

    let (m_hi, m0) = div_rem_10_u16(mhz);
    let (m2, m1) = div_rem_10(m_hi as u8);
    let (k_hi, k0) = div_rem_10_u16(khz);
    let (k2, k1) = div_rem_10(k_hi as u8);
    let (t1, t0) = div_rem_10(tens);

    unsafe {
        *out.get_unchecked_mut(0) = if m2 > 0 { b'0' + m2 } else { b' ' };
        *out.get_unchecked_mut(1) = if m2 > 0 || m1 > 0 { b'0' + m1 } else { b' ' };
        *out.get_unchecked_mut(2) = b'0' + m0;
        *out.get_unchecked_mut(3) = b'.';
        *out.get_unchecked_mut(4) = b'0' + k2;
        *out.get_unchecked_mut(5) = b'0' + k1;
        *out.get_unchecked_mut(6) = b'0' + k0;
        *out.get_unchecked_mut(7) = b'.';
        *out.get_unchecked_mut(8) = b'0' + t1;
        *out.get_unchecked_mut(9) = b'0' + t0;
    }
}

/// Update custom status bar with TX/RX/MON badge and live signal bars.
#[inline(never)]
pub fn sync_status_bar(api: &KrxApiTable, st: &KrxRadioState) {
    let mut words = [0u32; 16];
    let ptr = words.as_mut_ptr() as *mut u8;
    let monitor = (st._reserved & 1) != 0;
    let bars = if st.squelch_open != 0 || monitor {
        let shifted = ((st.rssi_dbm as i32 + 125).max(0) >> 4) as u8;
        shifted.clamp(1, 4)
    } else {
        0
    };
    unsafe {
        *ptr = KRX_STATUS_VISIBLE
            | KRX_STATUS_DIVIDER
            | KRX_STATUS_CUSTOM_TITLE
            | if bars > 0 { KRX_STATUS_HAS_SIGNAL } else { 0 };
        *ptr.add(1) = bars;
        // Title at offset 12: "Radio"
        let title = b"Radio";
        let mut i = 0usize;
        while i < title.len() {
            *ptr.add(12 + i) = *title.get_unchecked(i);
            i += 1;
        }
        // Badge at offset 28: TX / RX / MON
        let badge: &[u8] = if st.ptt_active != 0 {
            b"TX"
        } else if st.squelch_open != 0 {
            b"RX"
        } else if monitor {
            b"MON"
        } else {
            b""
        };
        let mut j = 0usize;
        while j < badge.len() {
            *ptr.add(28 + j) = *badge.get_unchecked(j);
            j += 1;
        }
        api.set_status_bar(&*(ptr as *const KrxStatusBar));
    }
}

#[inline(never)]
pub fn render_hud(
    api: &KrxApiTable,
    active_slot: u8,
    freqs: &[u32; 2],
    mod_mode: u8,
    step_idx: u8,
    squelch: u8,
    entry_digits: &[u8; 6],
    entry_len: u8,
    st: &KrxRadioState,
) {
    api.clear();

    // Primary VFO header badge + mode/step/squelch tags
    let tag: &[u8] = if active_slot == 0 { b"VFO A" } else { b"VFO B" };
    api.fill_rect(2, 11, 28, 9, COLOR_FILL);
    api.draw_text(4, 18, tag, FONT_TOMTHUMB, COLOR_CLEAR, ALIGN_LEFT);

    let mod_s = nth_packed_slice(MOD_NAMES, mod_mode);
    api.draw_text(34, 18, mod_s, FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    let step_s = nth_packed_slice(STEP_NAMES, step_idx);
    api.draw_text(60, 18, step_s, FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    let mut sq_buf = [b'S', b'Q', b'0' + (squelch.min(9))];
    api.draw_text(125, 18, &sq_buf, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    // Primary Frequency or Direct Entry
    let mut fbuf = [b' '; 10];
    if entry_len > 0 {
        let mut pos = 0usize;
        let mut i = 0usize;
        while i < 6 {
            if i == 3 {
                unsafe { *fbuf.get_unchecked_mut(pos) = b'.' };
                pos += 1;
            }
            let ch = if (i as u8) < entry_len {
                b'0' + unsafe { *entry_digits.get_unchecked(i) }
            } else {
                b'_'
            };
            unsafe { *fbuf.get_unchecked_mut(pos) = ch };
            pos += 1;
            i += 1;
        }
        api.draw_text(16, 33, &fbuf[..7], FONT_MUMATRIX, COLOR_FILL, ALIGN_LEFT);
    } else {
        let primary = unsafe { *freqs.get_unchecked((active_slot & 1) as usize) };
        format_freq_hz(primary, &mut fbuf);
        api.draw_text(12, 33, &fbuf, FONT_MUMATRIX, COLOR_FILL, ALIGN_LEFT);
    }
    api.draw_text(124, 33, b"MHz", FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    // Live RSSI S-meter bar (y = 39..44)
    api.draw_rect(2, 39, 124, 5, COLOR_FILL);
    let rssi_norm = ((st.rssi_dbm as i32 + 127).clamp(0, 80) as u32 * 120) >> 6;
    let fill_w = (rssi_norm as u8).min(120);
    if fill_w > 0 {
        api.fill_rect(4, 41, fill_w, 1, COLOR_FILL);
    }

    // Secondary VFO row (y = 53)
    let other_slot = (active_slot ^ 1) as usize;
    let other_tag: &[u8] = if other_slot == 0 { b"A:" } else { b"B:" };
    api.draw_text(3, 55, other_tag, FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);
    let secondary = unsafe { *freqs.get_unchecked(other_slot) };
    format_freq_hz(secondary, &mut fbuf);
    api.draw_text(15, 55, &fbuf, FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    let hint: &[u8] = if st.ptt_active != 0 {
        b"TX ACTIVE"
    } else {
        b"#VFO *STEP"
    };
    api.draw_text(125, 55, hint, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    // Suppress unused mut warning on sq_buf
    let _ = &mut sq_buf;
    api.commit();
}
