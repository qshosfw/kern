//! RF Syscall Facade for External VFO Transceiver Application.

use kern_sdk::{KrxApiTable, KrxRadioState, Op, prop, sub};

/// Tune active receiver frequency, bandwidth (`0=12.5k, 1=25k, 2=6.25k`), and step in Hz (`Op::RfTune`).
#[inline(never)]
pub fn tune(api: &KrxApiTable, freq_hz: u32, bw_code: u8, step_hz: u32) {
    let _ = api.call_op(Op::RfTune, freq_hz as usize, bw_code as usize, step_hz as usize);
}

/// Snapshot a VFO slot's 64-byte `ChannelRecord` (`Op::RfVfo(GET_CHANNEL)`).
#[inline(never)]
pub fn get_vfo_channel(api: &KrxApiTable, slot: u8, out_words: &mut [u32; 16]) -> bool {
    api.call_op(
        Op::RfVfo,
        sub::rf_vfo::GET_CHANNEL as usize,
        slot as usize,
        out_words.as_mut_ptr() as usize,
    ) != 1
}

/// Snapshot live coordinator telemetry (`Op::RfVfo(STATE)`).
#[inline(never)]
pub fn get_state(api: &KrxApiTable, out: &mut KrxRadioState) {
    let _ = api.call_op(
        Op::RfVfo,
        sub::rf_vfo::STATE as usize,
        out as *mut KrxRadioState as usize,
        0,
    );
}

/// Select active VFO slot (`0 = A`, `1 = B`, `2 = C`).
#[inline(never)]
pub fn select_vfo(api: &KrxApiTable, slot: u8) {
    let packed = (slot as u32) | (0xFFu32 << 8) | (0xFFu32 << 16) | (0xFFu32 << 24);
    let _ = api.prop_set32(prop::RF_WATCH_MODE, packed);
}

/// Toggle or set monitor squelch defeat (`0 = off`, `1 = on`, `2 = toggle`).
#[inline(never)]
pub fn toggle_monitor(api: &KrxApiTable) {
    let _ = api.prop_set32(prop::RF_MONITOR, 2);
}

/// Set modulation mode (`0 = FM`, `1 = AM`, `2 = USB`).
#[inline(never)]
pub fn set_modulation(api: &KrxApiTable, mode: u8) {
    let _ = api.prop_set(prop::RF_MODULATION, mode);
}

/// Set squelch level (`0..=9`).
#[inline(never)]
pub fn set_squelch(api: &KrxApiTable, level: u8) {
    let _ = api.prop_set(prop::RF_SQUELCH, level);
}

/// Key (`true`) or unkey (`false`) transmitter PTT.
#[inline(never)]
pub fn set_tx(api: &KrxApiTable, active: bool) {
    let _ = api.prop_set(prop::RF_TX, u8::from(active));
}
