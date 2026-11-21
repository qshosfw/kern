//! Kern OS Radio (Dual-VFO Transceiver) — External Native `.krx` Application.
//!
//! Decoupled from resident ROM firmware: mediates all RF tuning, modulation,
//! squelch, monitor, RSSI telemetry, and PTT through microkernel `Op::Rf*` syscalls.
#![no_std]
#![no_main]

mod rf;
mod ui;

use kern_sdk::{
    KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_TICK, KRX_OP_EVENT, KRX_OP_EXIT, KRX_OP_RENDER,
    KRX_OP_START, KrxApiTable, KrxEvent, KrxRadioState, key, krx_entry,
};

const MIN_FREQ_HZ: u32 = 18_000_000;
const MAX_FREQ_HZ: u32 = 1_300_000_000;

const STEPS_HZ: [u32; 7] = [2_500, 5_000, 6_250, 10_000, 12_500, 25_000, 50_000];

struct VfoState {
    active_slot: u8,
    freqs: [u32; 2],
    mod_mode: u8,
    step_idx: u8,
    squelch: u8,
    entry_digits: [u8; 6],
    entry_len: u8,
    radio: KrxRadioState,
}

impl VfoState {
    const fn new() -> Self {
        Self {
            active_slot: 0,
            freqs: [145_500_000, 433_500_000],
            mod_mode: 0,
            step_idx: 4, // 12.5 kHz
            squelch: 3,
            entry_digits: [0; 6],
            entry_len: 0,
            radio: KrxRadioState {
                active_vfo: 0,
                listening_vfo: 0,
                dual_watch: 0,
                cross_band: 0,
                bclo_policy: 0,
                ptt_active: 0,
                squelch_open: 0,
                subaudible_matched: 0,
                tot_locked_out: 0,
                lock_active: 0,
                tx_perm: 0,
                _reserved: 0,
                tot_remaining_s: 0,
                rssi_dbm: -120,
            },
        }
    }

    #[inline(never)]
    fn on_start(&mut self, api: &KrxApiTable) {
        self.entry_len = 0;
        let cur = rf::get_freq(api);
        if (MIN_FREQ_HZ..=MAX_FREQ_HZ).contains(&cur) {
            unsafe { *self.freqs.get_unchecked_mut((self.active_slot & 1) as usize) = cur };
        } else {
            rf::tune(api, self.freqs[0]);
        }
        rf::get_state(api, &mut self.radio);
        ui::sync_status_bar(api, &self.radio);
    }

    #[inline(never)]
    fn step_freq(&mut self, api: &KrxApiTable, up: bool) {
        self.entry_len = 0;
        let step = unsafe { *STEPS_HZ.get_unchecked((self.step_idx as usize).min(6)) };
        let slot = (self.active_slot & 1) as usize;
        let cur = unsafe { *self.freqs.get_unchecked(slot) };
        let next = if up {
            cur.saturating_add(step).min(MAX_FREQ_HZ)
        } else {
            cur.saturating_sub(step).max(MIN_FREQ_HZ)
        };
        unsafe { *self.freqs.get_unchecked_mut(slot) = next };
        rf::tune(api, next);
    }

    #[inline(never)]
    fn push_digit(&mut self, api: &KrxApiTable, digit: u8) {
        let len = self.entry_len as usize;
        if len < 6 {
            unsafe { *self.entry_digits.get_unchecked_mut(len) = digit };
            self.entry_len += 1;
        }
        if self.entry_len == 6 {
            let mut mhz_khz = 0u32;
            let mut i = 0usize;
            while i < 6 {
                mhz_khz = mhz_khz * 10 + (unsafe { *self.entry_digits.get_unchecked(i) } as u32);
                i += 1;
            }
            self.entry_len = 0;
            let hz = mhz_khz.saturating_mul(1_000);
            if (MIN_FREQ_HZ..=MAX_FREQ_HZ).contains(&hz) {
                let slot = (self.active_slot & 1) as usize;
                unsafe { *self.freqs.get_unchecked_mut(slot) = hz };
                rf::tune(api, hz);
            } else {
                api.show_toast(b"Out of Range", 1000);
            }
        }
    }

    #[inline(never)]
    fn on_event(&mut self, api: &KrxApiTable, ev: &KrxEvent) -> u32 {
        match ev.kind {
            KRX_EV_TICK => {
                rf::get_state(api, &mut self.radio);
                ui::sync_status_bar(api, &self.radio);
                1
            }
            KRX_EV_ENCODER => {
                let delta = ev.payload as i32;
                if delta != 0 {
                    self.step_freq(api, delta > 0);
                    return 1;
                }
                0
            }
            KRX_EV_KEY => {
                if ev.key == key::PTT {
                    if ev.payload == key::STATE_PRESSED {
                        rf::set_tx(api, true);
                    } else if ev.payload == key::STATE_RELEASED {
                        rf::set_tx(api, false);
                    }
                    rf::get_state(api, &mut self.radio);
                    ui::sync_status_bar(api, &self.radio);
                    return 1;
                }
                if ev.payload != key::STATE_PRESSED {
                    return 0;
                }
                if ev.key >= key::KEY0 && ev.key <= key::KEY9 {
                    self.push_digit(api, ev.key - key::KEY0);
                    return 1;
                }
                match ev.key {
                    key::UP => {
                        self.step_freq(api, true);
                        1
                    }
                    key::DOWN => {
                        self.step_freq(api, false);
                        1
                    }
                    key::HASH => {
                        self.entry_len = 0;
                        self.active_slot ^= 1;
                        rf::select_vfo(api, self.active_slot);
                        let f = unsafe { *self.freqs.get_unchecked((self.active_slot & 1) as usize) };
                        rf::tune(api, f);
                        1
                    }
                    key::STAR => {
                        self.step_idx = if self.step_idx + 1 < 7 {
                            self.step_idx + 1
                        } else {
                            0
                        };
                        1
                    }
                    key::MENU => {
                        self.mod_mode = if self.mod_mode + 1 < 3 {
                            self.mod_mode + 1
                        } else {
                            0
                        };
                        rf::set_modulation(api, self.mod_mode);
                        1
                    }
                    key::SIDE1 => {
                        rf::toggle_monitor(api);
                        1
                    }
                    key::SIDE2 => {
                        self.squelch = if self.squelch < 9 { self.squelch + 1 } else { 0 };
                        rf::set_squelch(api, self.squelch);
                        1
                    }
                    key::EXIT => {
                        if self.entry_len > 0 {
                            self.entry_len = 0;
                            1
                        } else {
                            0
                        }
                    }
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    #[inline(never)]
    fn render(&self, api: &KrxApiTable) {
        ui::render_hud(
            api,
            self.active_slot,
            &self.freqs,
            self.mod_mode,
            self.step_idx,
            self.squelch,
            &self.entry_digits,
            self.entry_len,
            &self.radio,
        );
    }
}

static mut STATE: VfoState = VfoState::new();

fn app_entry(op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    let state = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match op {
        KRX_OP_START => {
            state.on_start(api);
            1
        }
        KRX_OP_EVENT => {
            if arg.is_null() {
                return 0;
            }
            let ev = unsafe { &*(arg as *const KrxEvent) };
            state.on_event(api, ev)
        }
        KRX_OP_RENDER => {
            state.render(api);
            1
        }
        KRX_OP_EXIT => {
            rf::set_tx(api, false);
            0
        }
        _ => 0,
    }
}

krx_entry!(app_entry);
