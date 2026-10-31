//! Normative 12 Kernel Primitive Registry (§8.2 / KRX-ABI 2.5.0).
//!
//! Provides the exhaustive, zero-allocation microkernel primitive interface traits:
//! 1. `DisplayPrimitive` (CapDisplay)
//! 2. `RadioPrimitive`   (CapRadio)
//! 3. `AudioPrimitive`   (CapAudio)
//! 4. `StoragePrimitive` (CapStorage)
//! 5. `KeypadPrimitive`  (CapKeypad)
//! 6. `PowerPrimitive`   (CapPower)
//! 7. `SerialPrimitive`  (CapSerial)
//! 8. `TimerPrimitive`   (CapTimer)
//! 9. `UiPrimitive`      (CapUi)
//! 10. `MathPrimitive`   (CapMath)
//! 11. `FmPrimitive`     (CapFm)
//! 12. `ServicePrimitive`(CapService)

use crate::caps::{Cap, CapabilitySet};

// ── 1. Display Primitive ────────────────────────────────────────────────────
pub trait DisplayPrimitive {
    fn clear(&mut self);
    fn draw_pixel(&mut self, x: i16, y: i16, color: u8);
    fn draw_line(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, color: u8);
    fn draw_rect(&mut self, x: i16, y: i16, w: i16, h: i16, color: u8);
    fn fill_rect(&mut self, x: i16, y: i16, w: i16, h: i16, color: u8);
    fn draw_bitmap(&mut self, x: i16, y: i16, w: u8, h: u8, data: &[u8]);
    fn draw_str(&mut self, x: i16, y: i16, text: &str, font_id: u8, color: u8);
    fn draw_str_max(&mut self, x: i16, y: i16, text: &str, font_id: u8, color: u8, max_w: i16);
    fn draw_str_align(&mut self, x: i16, y: i16, text: &str, font_id: u8, color: u8, align: u8);
    fn set_inverted(&mut self, inverted: bool);
    fn set_contrast(&mut self, contrast: u8);
    fn commit_frame(&mut self);
    fn commit_rows(&mut self, row_mask: u8);
    fn get_framebuffer_mut(&mut self) -> &mut [u8; 1024];
}

// ── 2. Radio Primitive ──────────────────────────────────────────────────────
pub trait RadioPrimitive {
    fn tune(&mut self, freq_hz: u32, bw_hz: u32, step_hz: u32);
    fn get_frequency(&mut self) -> u32;
    fn get_bandwidth(&mut self) -> u32;
    fn get_modulation(&mut self) -> u8;
    fn set_modulation(&mut self, mod_type: u8);
    fn set_squelch(&mut self, level: u8);
    fn get_rssi_dbm(&mut self) -> i16;
    fn get_glitch(&mut self) -> u8;
    fn is_carrier_detected(&mut self) -> bool;
    fn get_tx_state(&mut self) -> u8;
    fn set_tx(&mut self, active: bool);
    fn set_tx_carrier(&mut self, on: bool);
    fn set_tx_power(&mut self, level: u8);
    fn set_af_mode(&mut self, mode: u8);
    fn set_agc(&mut self, table_idx: u8);
    fn set_subaudible_ctcss(&mut self, freq_tenths_hz: u16);
    fn set_subaudible_dcs(&mut self, code: u16, inverted: bool);
    fn send_tail(&mut self);
    fn mute(&mut self, muted: bool);
    fn mute_mic(&mut self, muted: bool);
    fn read_register(&mut self, reg: u8) -> u16;
    fn write_register(&mut self, reg: u8, val: u16);
    fn fsk_init(&mut self, baud: u16);
    fn fsk_send(&mut self, data: &[u8]) -> Result<(), ()>;
    fn fsk_receive(&mut self, buf: &mut [u8]) -> usize;
    fn sweep(&mut self, start_hz: u32, step_hz: u32, out_rssi: &mut [i16]) -> usize;
    fn emit_symbols(&mut self, symbols: &[crate::proc::abi::KrxSymbol]) -> Result<(), ()>;
    fn set_dual_tones_raw(
        &mut self,
        tone1_word: u16,
        tone1_gain: u8,
        tone2_word: u16,
        tone2_gain: u8,
    );
}

// ── 3. Audio Primitive ──────────────────────────────────────────────────────
pub trait AudioPrimitive {
    fn play_tone(&mut self, freq_hz: u16, duration_ms: u16);
    fn play_tone_sequence(&mut self, notes: &[(u16, u16)]);
    fn play_beep(&mut self, beep_type: u8);
    fn set_volume(&mut self, volume: u8);
    fn get_volume(&self) -> u8;
    fn set_audio_path(&mut self, on: bool);
    fn stop_audio(&mut self);
    fn is_audio_playing(&self) -> bool;
    fn set_mic_gain(&mut self, gain: u8);
    fn read_mic_sample(&mut self) -> u16;
    fn capture_start(&mut self, rate_hz: u16) -> Result<(), ()>;
    fn capture_read(&mut self, out_samples: &mut [i16]) -> usize;
    fn capture_stop(&mut self);
}

// ── 4. Storage Primitive ────────────────────────────────────────────────────
pub trait StoragePrimitive {
    fn read_file(&mut self, path: &str, buf: &mut [u8]) -> Result<usize, ()>;
    fn read_file_at(&mut self, path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, ()> {
        let _ = (path, offset, buf);
        Err(())
    }
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), ()>;
    fn append_file(&mut self, path: &str, data: &[u8]) -> Result<(), ()>;
    fn remove_file(&mut self, path: &str) -> Result<(), ()>;
    fn file_size(&mut self, path: &str) -> Result<usize, ()>;
    fn exists(&mut self, path: &str) -> bool;
    fn load_config(&mut self, app_name: &str, buf: &mut [u8]) -> Result<usize, ()>;
    fn save_config(&mut self, app_name: &str, data: &[u8]) -> Result<(), ()>;
    fn read_asset(&mut self, asset_id: u16, offset: u32, buf: &mut [u8]) -> Result<usize, ()>;
    fn list_dir(
        &mut self,
        dir_path: &str,
        out_entries: &mut [crate::proc::abi::KrxDirEntry],
    ) -> Result<usize, ()> {
        let _ = (dir_path, out_entries);
        Ok(0)
    }
}

// ── 5. Keypad Primitive ─────────────────────────────────────────────────────
pub trait KeypadPrimitive {
    fn poll_key(&mut self) -> Option<u8>;
    fn clear_key_buffer(&mut self);
    fn is_key_pressed(&self, key_code: u8) -> bool;
    fn get_key_matrix(&self) -> u32;
    fn is_keypad_locked(&self) -> bool;
    fn is_f_active(&self) -> bool;
    fn is_ptt_pressed(&self) -> bool;
    fn is_side1_pressed(&self) -> bool;
    fn is_side2_pressed(&self) -> bool;
}

// ── 6. Power Primitive ──────────────────────────────────────────────────────
pub trait PowerPrimitive {
    fn battery_mv(&self) -> u16;
    fn battery_pct(&self) -> u8;
    fn battery_ma(&self) -> u16 {
        0
    }
    fn battery_type(&self) -> u8 {
        0
    }
    fn raw_battery_adc(&self) -> u16 {
        0
    }
    fn mcu_temp_deci_c(&self) -> i16 {
        250
    }
    fn is_charging(&self) -> bool;
    fn is_low(&self) -> bool {
        false
    }
    fn is_critical(&self) -> bool {
        false
    }
    fn set_backlight(&mut self, mode: u8, timeout_s: u8);
    fn set_backlight_brightness(&mut self, max: u8, min: u8) {
        let _ = (max, min);
    }
    fn wake_backlight(&mut self);
    fn set_flashlight(&mut self, on: bool);
}

// ── 7. Serial Primitive ─────────────────────────────────────────────────────
pub trait SerialPrimitive {
    fn write(&mut self, data: &[u8]);
    fn read(&mut self, buf: &mut [u8]) -> usize;
    fn is_usb_active(&self) -> bool;
}

// ── 8. Timer Primitive ──────────────────────────────────────────────────────
pub trait TimerPrimitive {
    fn uptime_secs(&self) -> u32;
    fn uptime_ms(&self) -> u32;
    fn delay_ms(&self, ms: u32);
}

// ── 9. UI Modal Primitive ───────────────────────────────────────────────────
pub trait UiPrimitive {
    fn show_popup(&mut self, tag: u8, title: &'static str, message: &str);
    fn show_confirm(&mut self, tag: u8, title: &'static str, message: &'static str);
    fn prompt_text(&mut self, tag: u8, title: &'static str, initial: &str, max_len: usize);
    fn close_modal(&mut self);
    fn has_active_modal(&self) -> bool;
}

// ── 10. Math Primitive ──────────────────────────────────────────────────────
pub trait MathPrimitive {
    fn crc32(&self, data: &[u8]) -> u32;
    fn crc16(&self, data: &[u8]) -> u16;
    fn rand32(&mut self) -> u32;
    fn idivmod(&self, n: i32, d: i32) -> (i32, i32);
    fn uidivmod(&self, n: u32, d: u32) -> (u32, u32);
}

// ── 11. FM Broadcast Tuner Primitive ────────────────────────────────────────
pub trait FmPrimitive {
    fn fm_tune(&mut self, freq_100khz: u16);
    fn fm_set_volume(&mut self, volume: u8);
    fn fm_mute(&mut self, mute: bool);
    fn fm_get_rssi(&mut self) -> u8;
}

// ── 12. Service & IPC Primitive ─────────────────────────────────────────────
pub trait ServicePrimitive {
    fn post_event(&mut self, event_id: u32) -> bool;
    fn publish_event(&mut self, topic: u16, data: &[u8]) -> bool;
    fn get_stack_free_now(&self) -> u16;
    fn get_stack_free_min(&self) -> u16;
}

// ── 13. Low-Level Hardware & Bus Primitive ──────────────────────────────────
pub trait HardwarePrimitive {
    fn i2c_read(&mut self, addr: u8, reg: u8, buf: &mut [u8]) -> Result<usize, ()>;
    fn i2c_write(&mut self, addr: u8, reg: u8, data: &[u8]) -> Result<(), ()>;
    fn spi_transfer(&mut self, cs_pin: u8, tx: &[u8], rx: &mut [u8]) -> Result<(), ()>;
    fn gpio_read(&self, pin: u8) -> bool;
    fn gpio_write(&mut self, pin: u8, high: bool);
    fn raw_flash_read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), ()>;
}

// ── Universal Kernel Primitive Capability Check ─────────────────────────────

/// Validates whether an actor with `caps` is authorized to invoke a primitive method.
#[inline(always)]
pub fn check_primitive_access(caps: CapabilitySet, cap: Cap) -> bool {
    caps.contains(cap)
}
