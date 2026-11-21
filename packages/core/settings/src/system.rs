//! Live Device Info & System Recovery Actions (Reset Settings, Restart Radio).

use kern_sdk::{KrxApiTable, format_u16_into, nth_packed_slice, prop};

use crate::ui;

pub const MODAL_RESET_SETTINGS: u8 = 41;
pub const MODAL_REBOOT_RADIO: u8 = 42;
pub const INFO_ROWS: u8 = 5;

const INFO_LABELS: &[u8] = b"Battery\0Charge\0MCU Temp\0CPU Clock\0Free Stack\0";

#[inline(never)]
pub fn render_device_info(api: &KrxApiTable, sel: u8, scroll_y: i16, tick: u8) {
    let mut val_buf = [0u8; 16];
    let mut idx = 0u8;
    while idx < INFO_ROWS {
        let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - scroll_y;
        let (num, dec, suf): (u16, u8, &[u8]) = match idx {
            0 => ((((api.battery_mv() as u32) * 52_429) >> 19) as u16, 2, b"V"),
            1 => (api.battery_pct() as u16, 0, b"%"),
            2 => {
                let t = api.mcu_temp_deci_c();
                (if t < 0 { (-t) as u16 } else { t as u16 }, 1, b"C")
            }
            3 => ((((api.cpu_hz() >> 12) * 1074) >> 18) as u16, 0, b"MHz"),
            _ => (api.stack_free() as u16, 0, b"B"),
        };
        let val = format_u16_into(num, dec, suf, &mut val_buf);
        ui::draw_scrolled_row(
            api,
            y,
            nth_packed_slice(INFO_LABELS, idx),
            val,
            0,
            idx == sel,
            false,
            0,
            tick,
        );
        idx += 1;
    }
    ui::finish_list_frame(api, scroll_y, INFO_ROWS);
}

#[inline(never)]
pub fn request_reset(api: &KrxApiTable) {
    api.show_confirm(
        MODAL_RESET_SETTINGS,
        b"Reset All?",
        b"Restore defaults?",
        b"Hold",
        b"Cancel",
        1200,
    );
}

#[inline(never)]
pub fn request_reboot(api: &KrxApiTable) {
    api.show_confirm(
        MODAL_REBOOT_RADIO,
        b"Restart?",
        b"Save and restart?",
        b"Hold",
        b"Cancel",
        800,
    );
}

#[inline(never)]
pub fn apply_reset_defaults(api: &KrxApiTable) {
    const DEFAULTS: [(u8, u8); 14] = [
        (prop::BACKLIGHT_TIME, 2),
        (prop::BACKLIGHT_MAX, 8),
        (prop::BACKLIGHT_MIN, 0),
        (prop::CONTRAST, 50),
        (prop::INVERT_PIXELS, 0),
        (prop::UI_ANIMATIONS, 1),
        (prop::VOLUME, 5),
        (prop::KEY_BEEP, 1),
        (prop::BEEP_GAIN, 5),
        (prop::ROGER_BEEP, 0),
        (prop::AUTO_LOCK, 0),
        (prop::POWER_PROFILE, 1),
        (prop::LAUNCHER_STYLE, 0),
        (prop::CPU_CLOCK, 3),
    ];
    let mut i = 0usize;
    while i < DEFAULTS.len() {
        let (id, val) = unsafe { *DEFAULTS.get_unchecked(i) };
        let _ = api.pref_set(id, val);
        i += 1;
    }
    let _ = api.flush();
    api.show_toast(b"Defaults Restored", 1200);
}
