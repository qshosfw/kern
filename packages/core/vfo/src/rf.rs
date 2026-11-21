//! RF Syscall Facade for External VFO Transceiver Application.

use kern_sdk::{KrxApiTable, KrxRadioState, Op, sub};

/// Tune active receiver frequency in Hz (`Op::RfTune`).
#[inline(never)]
pub fn tune(api: &KrxApiTable, freq_hz: u32) {
    let _ = api.call_op(Op::RfTune, freq_hz as usize, 0, 0);
}

/// Query current receiver frequency in Hz (`Op::RfGet(FREQ)`).
#[inline(never)]
pub fn get_freq(api: &KrxApiTable) -> u32 {
    api.call_op(Op::RfGet, sub::rf_get::FREQ as usize, 0, 0) as u32
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

/// Select active VFO slot (`0 = A`, `1 = B`).
#[inline(never)]
pub fn select_vfo(api: &KrxApiTable, slot: u8) {
    let packed = (slot as usize) | (0xFFusize << 8) | (0xFFusize << 16) | (0xFFusize << 24);
    let _ = api.call_op(Op::RfVfo, sub::rf_vfo::WATCH_MODE as usize, packed, 0);
}

/// Toggle or set monitor squelch defeat (`0 = off`, `1 = on`, `2 = toggle`).
#[inline(never)]
pub fn toggle_monitor(api: &KrxApiTable) {
    let _ = api.call_op(Op::RfVfo, sub::rf_vfo::MONITOR as usize, 2, 0);
}

/// Set modulation mode (`0 = FM`, `1 = AM`, `2 = USB`).
#[inline(never)]
pub fn set_modulation(api: &KrxApiTable, mode: u8) {
    let _ = api.call_op(Op::RfSet, sub::rf_set::MODULATION as usize, mode as usize, 0);
}

/// Set squelch level (`0..=9`).
#[inline(never)]
pub fn set_squelch(api: &KrxApiTable, level: u8) {
    let _ = api.call_op(Op::RfSet, sub::rf_set::SQUELCH as usize, level as usize, 0);
}

/// Key (`true`) or unkey (`false`) transmitter PTT.
#[inline(never)]
pub fn set_tx(api: &KrxApiTable, active: bool) {
    let _ = api.call_op(Op::RfSet, sub::rf_set::TX as usize, usize::from(active), 0);
}
