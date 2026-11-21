#![no_std]
#![no_main]

use kern_sdk::{
    KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_MODAL, KRX_OP_EVENT, KRX_OP_EXIT, KRX_OP_RENDER,
    KRX_OP_START, KrxApiTable, KrxEvent, MODAL_RESULT_CONFIRMED, format_u16_into, icon, key,
    krx_entry, nth_packed_slice, prop, rel_strings,
};

const PAGE_ROOT: u8 = 0;
const PAGE_DISPLAY: u8 = 1;
const PAGE_AUDIO: u8 = 2;
const PAGE_BUTTONS: u8 = 3;
const PAGE_BATTERY: u8 = 4;
const PAGE_DEVICE: u8 = 5;
const PAGE_APPS: u8 = 6;
const PAGE_SYSTEM: u8 = 7;

const KIND_SUBMENU: u8 = 0;
const KIND_TOGGLE: u8 = 1;
const KIND_RANGE: u8 = 2;
const KIND_CHOICE: u8 = 3;
const KIND_METRIC: u8 = 4;
const KIND_ACTION: u8 = 5;

const METRIC_BATT_V: u8 = 0;
const METRIC_BATT_PCT: u8 = 1;
const METRIC_BATT_TEMP: u8 = 2;

const ACT_FLUSH: u8 = 0;
const ACT_REBOOT: u8 = 1;
const TAG_CONFIRM_REBOOT: u8 = 1;

/// Compact 4-byte PIC row descriptor (`0` pointer relocations in `.rodata`).
#[derive(Clone, Copy)]
#[repr(C, align(4))]
struct RowDef {
    label_idx: u8,
    kind: u8,
    prop_or_id: u8,
    aux: u8,
}

/// `(start, len)` span into `ALL_ROWS` for each `PAGE_*` (`0..=7`).
static PAGE_SPANS: [[u8; 2]; 8] = [
    [0, 7],   // PAGE_ROOT
    [7, 7],   // PAGE_DISPLAY
    [14, 7],  // PAGE_AUDIO
    [21, 7],  // PAGE_BUTTONS
    [28, 7],  // PAGE_BATTERY
    [35, 3],  // PAGE_DEVICE
    [38, 4],  // PAGE_APPS
    [42, 4],  // PAGE_SYSTEM
];

static ALL_ROWS: [RowDef; 46] = [
    // Root (0..7)
    RowDef { label_idx: 0, kind: KIND_SUBMENU, prop_or_id: PAGE_DISPLAY, aux: icon::DISPLAY },
    RowDef { label_idx: 1, kind: KIND_SUBMENU, prop_or_id: PAGE_AUDIO, aux: icon::AUDIO },
    RowDef { label_idx: 2, kind: KIND_SUBMENU, prop_or_id: PAGE_BUTTONS, aux: icon::BUTTONS },
    RowDef { label_idx: 3, kind: KIND_SUBMENU, prop_or_id: PAGE_BATTERY, aux: icon::BATTERY },
    RowDef { label_idx: 4, kind: KIND_SUBMENU, prop_or_id: PAGE_DEVICE, aux: icon::DEVICE },
    RowDef { label_idx: 5, kind: KIND_SUBMENU, prop_or_id: PAGE_APPS, aux: icon::APPS },
    RowDef { label_idx: 6, kind: KIND_SUBMENU, prop_or_id: PAGE_SYSTEM, aux: icon::SYSTEM },
    // Display (7..14)
    RowDef { label_idx: 7, kind: KIND_CHOICE, prop_or_id: prop::BACKLIGHT_TIME, aux: 0 },
    RowDef { label_idx: 8, kind: KIND_RANGE, prop_or_id: prop::BACKLIGHT_MAX, aux: 0 },
    RowDef { label_idx: 9, kind: KIND_RANGE, prop_or_id: prop::BACKLIGHT_MIN, aux: 0 },
    RowDef { label_idx: 10, kind: KIND_RANGE, prop_or_id: prop::CONTRAST, aux: 1 },
    RowDef { label_idx: 11, kind: KIND_TOGGLE, prop_or_id: prop::INVERT_PIXELS, aux: 0 },
    RowDef { label_idx: 12, kind: KIND_CHOICE, prop_or_id: prop::SHADING_MODE, aux: 1 },
    RowDef { label_idx: 13, kind: KIND_CHOICE, prop_or_id: prop::UI_ANIMATIONS, aux: 2 },
    // Audio (14..21)
    RowDef { label_idx: 14, kind: KIND_RANGE, prop_or_id: prop::VOLUME, aux: 0 },
    RowDef { label_idx: 15, kind: KIND_RANGE, prop_or_id: prop::BEEP_GAIN, aux: 0 },
    RowDef { label_idx: 16, kind: KIND_CHOICE, prop_or_id: prop::KEY_BEEP, aux: 3 },
    RowDef { label_idx: 17, kind: KIND_RANGE, prop_or_id: prop::MIC_GAIN, aux: 2 },
    RowDef { label_idx: 18, kind: KIND_TOGGLE, prop_or_id: prop::ROGER_BEEP, aux: 0 },
    RowDef { label_idx: 19, kind: KIND_CHOICE, prop_or_id: prop::BOOT_JINGLE, aux: 4 },
    RowDef { label_idx: 20, kind: KIND_CHOICE, prop_or_id: prop::AUDIO_PROFILE, aux: 5 },
    // Buttons (21..28)
    RowDef { label_idx: 21, kind: KIND_CHOICE, prop_or_id: prop::PTT_MODE, aux: 6 },
    RowDef { label_idx: 22, kind: KIND_CHOICE, prop_or_id: prop::AUTO_LOCK, aux: 7 },
    RowDef { label_idx: 23, kind: KIND_CHOICE, prop_or_id: prop::F_KEY_TIMEOUT, aux: 8 },
    RowDef { label_idx: 24, kind: KIND_TOGGLE, prop_or_id: prop::FAST_SCROLL, aux: 0 },
    RowDef { label_idx: 25, kind: KIND_CHOICE, prop_or_id: prop::HOLD_DELAY, aux: 9 },
    RowDef { label_idx: 26, kind: KIND_CHOICE, prop_or_id: prop::REPEAT_RATE, aux: 10 },
    RowDef { label_idx: 27, kind: KIND_TOGGLE, prop_or_id: prop::F_DRAWER, aux: 0 },
    // Battery (28..35)
    RowDef { label_idx: 28, kind: KIND_CHOICE, prop_or_id: prop::BATTERY_FORMAT, aux: 11 },
    RowDef { label_idx: 29, kind: KIND_CHOICE, prop_or_id: prop::AUTO_POWER_OFF, aux: 12 },
    RowDef { label_idx: 30, kind: KIND_TOGGLE, prop_or_id: prop::CHARGE_DETECT, aux: 0 },
    RowDef { label_idx: 31, kind: KIND_CHOICE, prop_or_id: prop::POWER_PROFILE, aux: 13 },
    RowDef { label_idx: 32, kind: KIND_METRIC, prop_or_id: METRIC_BATT_V, aux: 0 },
    RowDef { label_idx: 33, kind: KIND_METRIC, prop_or_id: METRIC_BATT_PCT, aux: 0 },
    RowDef { label_idx: 34, kind: KIND_METRIC, prop_or_id: METRIC_BATT_TEMP, aux: 0 },
    // Device (35..38)
    RowDef { label_idx: 35, kind: KIND_CHOICE, prop_or_id: prop::MODEL, aux: 18 },
    RowDef { label_idx: 36, kind: KIND_CHOICE, prop_or_id: prop::BOOT_SPLASH_ANIM, aux: 14 },
    RowDef { label_idx: 37, kind: KIND_CHOICE, prop_or_id: prop::BOOT_SPLASH_DURATION, aux: 15 },
    // Apps (38..42)
    RowDef { label_idx: 38, kind: KIND_CHOICE, prop_or_id: prop::LAUNCHER_STYLE, aux: 16 },
    RowDef { label_idx: 39, kind: KIND_TOGGLE, prop_or_id: prop::SHOW_NUMBERS, aux: 0 },
    RowDef { label_idx: 40, kind: KIND_TOGGLE, prop_or_id: prop::SHOW_LABELS, aux: 0 },
    RowDef { label_idx: 41, kind: KIND_CHOICE, prop_or_id: prop::STARTUP_APP, aux: 17 },
    // System (42..46)
    RowDef { label_idx: 42, kind: KIND_CHOICE, prop_or_id: prop::CPU_CLOCK, aux: 19 },
    RowDef { label_idx: 43, kind: KIND_CHOICE, prop_or_id: prop::UART_BAUD, aux: 20 },
    RowDef { label_idx: 44, kind: KIND_ACTION, prop_or_id: ACT_FLUSH, aux: 0 },
    RowDef { label_idx: 45, kind: KIND_ACTION, prop_or_id: ACT_REBOOT, aux: 0 },
];

rel_strings!(static ROW_LABELS = [
    b"Display",
    b"Audio",
    b"Buttons",
    b"Battery",
    b"Device",
    b"Apps",
    b"System",
    b"Backlight Time",
    b"Max Brightness",
    b"Min Brightness",
    b"Contrast",
    b"Invert Pixels",
    b"Shading Mode",
    b"Animations",
    b"AF Gain",
    b"Beep Vol",
    b"Key Beep",
    b"Mic Gain",
    b"Roger Beep",
    b"Boot Jingle",
    b"RX Profile",
    b"PTT Mode",
    b"Auto Lock",
    b"Sticky F-Key",
    b"Fast Scroll",
    b"Hold Delay",
    b"Repeat Rate",
    b"F Drawer",
    b"Status Bar",
    b"Deep Sleep",
    b"Charge Detect",
    b"Power Profile",
    b"Voltage",
    b"Charge",
    b"Core Temp",
    b"Model",
    b"Splash Anim",
    b"Splash Time",
    b"Style",
    b"Key Badges",
    b"App Titles",
    b"Startup App",
    b"CPU Clock",
    b"UART Baud",
    b"Save Settings",
    b"Restart Radio",
];);

rel_strings!(static CHOICE_BLOBS = [
    prop::options::BACKLIGHT_TIME.as_bytes(),
    prop::options::SHADING.as_bytes(),
    prop::options::ANIMATION.as_bytes(),
    prop::options::KEY_BEEP.as_bytes(),
    prop::options::BOOT_JINGLE.as_bytes(),
    prop::options::AUDIO_PROFILE.as_bytes(),
    prop::options::PTT_MODE.as_bytes(),
    prop::options::AUTO_LOCK.as_bytes(),
    prop::options::F_KEY_TIMEOUT.as_bytes(),
    prop::options::HOLD_DELAY.as_bytes(),
    prop::options::REPEAT_RATE.as_bytes(),
    prop::options::BATTERY_FORMAT.as_bytes(),
    prop::options::AUTO_POWER_OFF.as_bytes(),
    prop::options::POWER_PROFILE.as_bytes(),
    prop::options::BOOT_ANIM.as_bytes(),
    prop::options::SPLASH_TIME.as_bytes(),
    prop::options::LAUNCHER_STYLE.as_bytes(),
    prop::options::STARTUP_APP.as_bytes(),
    b"UV-K1\0UV-K5 V3\0Custom\0",
    prop::options::CPU_CLOCK_72.as_bytes(),
    prop::options::UART_BAUD.as_bytes(),
];);

rel_strings!(static UI_STRS = [
    b"Off",
    b"On",
    b"",
    b"%",
    b" dB",
    b" V",
    b" C",
    b"Settings Saved",
    b"Restart Radio?",
    b"Save & restart now?",
    b"Hold",
    b"Cancel",
];);

struct SettingsState {
    page: u8,
    sel: u8,
    scroll: u8,
    root_sel: u8,
    root_scroll: u8,
    editing: bool,
    saved_val: u8,
}

static mut STATE: SettingsState = SettingsState {
    page: PAGE_ROOT,
    sel: 0,
    scroll: 0,
    root_sel: 0,
    root_scroll: 0,
    editing: false,
    saved_val: 0,
};

#[inline(always)]
fn page_span(page: u8) -> (usize, usize) {
    let idx = if page < 8 { page as usize } else { 0 };
    let pair = unsafe { *PAGE_SPANS.get_unchecked(idx) };
    (pair[0] as usize, pair[1] as usize)
}

#[inline(always)]
fn row_at(start: usize, offset: usize) -> RowDef {
    unsafe { *ALL_ROWS.get_unchecked(start + offset) }
}

fn prop_bounds(id: u8) -> (u8, u8, u8) {
    prop::lookup_bounds(id).unwrap_or((0, 1, 1))
}

fn step_row(api: &KrxApiTable, row: RowDef, up: bool, preview_only: bool) {
    let id = row.prop_or_id;
    let (min, max, step) = prop_bounds(id);
    let cur = api.prop_get(id);
    let next = if row.kind == KIND_CHOICE {
        if up {
            if cur < max { cur + 1 } else { min }
        } else if cur > min {
            cur - 1
        } else {
            max
        }
    } else if up {
        cur.saturating_add(step).min(max)
    } else {
        cur.saturating_sub(step).max(min)
    };
    if preview_only {
        if !api.prop_set(id, next) {
            let _ = api.pref_set(id, next);
        }
    } else {
        let _ = api.pref_set(id, next);
    }
    if id == prop::KEY_BEEP && next > 0 {
        api.play_beep(next);
    }
}

fn move_cursor(st: &mut SettingsState, count: u8, up: bool) {
    if count == 0 {
        return;
    }
    if up {
        st.sel = if st.sel > 0 { st.sel - 1 } else { count - 1 };
    } else {
        st.sel = if st.sel + 1 < count { st.sel + 1 } else { 0 };
    }
    if st.sel < st.scroll {
        st.scroll = st.sel;
    } else if st.sel >= st.scroll + 4 {
        st.scroll = st.sel - 3;
    }
}

fn handle_key(api: &KrxApiTable, st: &mut SettingsState, code: u8) -> u32 {
    let (start, count_usize) = page_span(st.page);
    let count = count_usize as u8;
    if st.editing {
        let row = row_at(start, (st.sel as usize).min(count_usize.saturating_sub(1)));
        match code {
            key::UP | key::KEY2 => {
                step_row(api, row, true, true);
                return 1;
            }
            key::DOWN | key::KEY8 => {
                step_row(api, row, false, true);
                return 1;
            }
            key::MENU | key::KEY5 => {
                let cur = api.prop_get(row.prop_or_id);
                let _ = api.pref_set(row.prop_or_id, cur);
                let _ = api.flush();
                st.editing = false;
                return 1;
            }
            key::EXIT => {
                let _ = api.pref_set(row.prop_or_id, st.saved_val);
                st.editing = false;
                return 1;
            }
            _ => return 1,
        }
    }

    match code {
        key::UP | key::KEY2 => {
            move_cursor(st, count, true);
            1
        }
        key::DOWN | key::KEY8 => {
            move_cursor(st, count, false);
            1
        }
        key::EXIT => {
            if st.page != PAGE_ROOT {
                let _ = api.flush();
                st.page = PAGE_ROOT;
                st.sel = st.root_sel;
                st.scroll = st.root_scroll;
                1
            } else {
                let _ = api.flush();
                0
            }
        }
        key::MENU | key::KEY5 => {
            if (st.sel as usize) >= count_usize {
                return 1;
            }
            let row = row_at(start, st.sel as usize);
            match row.kind {
                KIND_SUBMENU => {
                    st.root_sel = st.sel;
                    st.root_scroll = st.scroll;
                    st.page = row.prop_or_id;
                    st.sel = 0;
                    st.scroll = 0;
                }
                KIND_TOGGLE => {
                    let cur = api.prop_get(row.prop_or_id);
                    let _ = api.pref_set(row.prop_or_id, u8::from(cur == 0));
                }
                KIND_RANGE | KIND_CHOICE => {
                    st.saved_val = api.prop_get(row.prop_or_id);
                    st.editing = true;
                }
                KIND_ACTION => {
                    if row.prop_or_id == ACT_FLUSH {
                        let _ = api.flush();
                        api.show_toast(UI_STRS.get(7), 900);
                    } else if row.prop_or_id == ACT_REBOOT {
                        let _ = api.flush();
                        api.show_confirm(
                            TAG_CONFIRM_REBOOT,
                            UI_STRS.get(8),
                            UI_STRS.get(9),
                            UI_STRS.get(10),
                            UI_STRS.get(11),
                            800,
                        );
                    }
                }
                _ => {}
            }
            1
        }
        _ => 0,
    }
}

fn format_row_value<'a>(
    api: &KrxApiTable,
    row: RowDef,
    buf: &'a mut [u8; 16],
) -> (&'a [u8], bool) {
    match row.kind {
        KIND_SUBMENU => (&[], true),
        KIND_TOGGLE => {
            let on = api.prop_get(row.prop_or_id) != 0;
            (UI_STRS.get(usize::from(on)), false)
        }
        KIND_RANGE => {
            let val = api.prop_get(row.prop_or_id) as u16;
            let unit = UI_STRS.get(2 + row.aux as usize);
            (format_u16_into(val, 0, unit, buf), false)
        }
        KIND_CHOICE => {
            let val = api.prop_get(row.prop_or_id);
            let blob = CHOICE_BLOBS.get(row.aux as usize);
            (nth_packed_slice(blob, val), false)
        }
        KIND_METRIC => match row.prop_or_id {
            METRIC_BATT_V => {
                let mv = api.battery_mv();
                let cv = (((mv as u32) * 52_429) >> 19) as u16;
                (format_u16_into(cv, 2, UI_STRS.get(5), buf), false)
            }
            METRIC_BATT_PCT => {
                let pct = api.battery_pct() as u16;
                (format_u16_into(pct, 0, UI_STRS.get(3), buf), false)
            }
            _ => {
                let temp = api.mcu_temp_deci_c().max(0) as u16;
                (format_u16_into(temp, 1, UI_STRS.get(6), buf), false)
            }
        },
        _ => (&[], false),
    }
}

fn render(api: &KrxApiTable, st: &SettingsState) {
    api.clear();
    let (start_base, total) = page_span(st.page);
    let start = st.scroll as usize;
    let end = (start + 4).min(total);
    let mut val_buf = [0u8; 16];

    let mut vis = 0usize;
    while start + vis < end {
        let idx = start + vis;
        let row = row_at(start_base, idx);
        let y = 9 + (vis as i16) * 13;
        let is_sel = idx == st.sel as usize;
        let (val_slice, has_chevron) = format_row_value(api, row, &mut val_buf);
        let mut flags = u8::from(is_sel);
        if has_chevron {
            flags |= 2;
        }
        if is_sel && st.editing {
            flags |= 4;
        }
        let icon_id = if st.page == PAGE_ROOT { row.aux } else { 0 };
        api.draw_menu_item(y, ROW_LABELS.get(row.label_idx as usize), val_slice, icon_id, flags);
        vis += 1;
    }

    if total > 4 {
        api.draw_scrollbar(125, 9, 52, st.scroll as u16, total as u16, 4);
    }
    api.commit();
}

fn handle(op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    let st = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match op {
        KRX_OP_START => {
            *st = SettingsState {
                page: PAGE_ROOT,
                sel: 0,
                scroll: 0,
                root_sel: 0,
                root_scroll: 0,
                editing: false,
                saved_val: 0,
            };
            0
        }
        KRX_OP_EVENT => {
            if arg.is_null() {
                return 0;
            }
            let ev = unsafe { &*(arg as *const KrxEvent) };
            match ev.kind {
                KRX_EV_KEY if ev.payload == key::STATE_PRESSED => handle_key(api, st, ev.key),
                KRX_EV_ENCODER => {
                    let dir = ev.payload as i32;
                    if dir != 0 {
                        let code = if dir < 0 { key::UP } else { key::DOWN };
                        return handle_key(api, st, code);
                    }
                    0
                }
                KRX_EV_MODAL => {
                    if ev.key == TAG_CONFIRM_REBOOT && ev.flags == MODAL_RESULT_CONFIRMED {
                        api.reboot(0);
                    }
                    1
                }
                _ => 0,
            }
        }
        KRX_OP_RENDER => {
            render(api, st);
            0
        }
        KRX_OP_EXIT => {
            let _ = api.flush();
            0
        }
        _ => 0,
    }
}

krx_entry!(handle);
