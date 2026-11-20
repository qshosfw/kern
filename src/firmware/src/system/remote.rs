//! Remote-control liveness: RigWire activity tracking, sleep inhibition and the
//! link-loss transmit failsafe for unattended nodes.

use crate::hardware::SystemHardware;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Hold-off window (ms) after the last valid RigWire frame during which Deep Sleep and APO
/// are inhibited so a remote operator over USB-C or 2-pin UART cannot lock themselves out.
const REMOTE_SLEEP_INHIBIT_MS: u32 = 30_000;
/// A remotely keyed transmitter is dropped once the link has been silent this long.
pub const REMOTE_TX_LINK_TIMEOUT_MS: u32 = 5_000;

static LAST_WIRE_ACTIVITY_MS: AtomicU32 = AtomicU32::new(0);
static WIRE_TX_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Record a valid inbound RigWire frame at `now`.
#[inline(always)]
pub fn note_wire_activity(now: u32) {
    LAST_WIRE_ACTIVITY_MS.store(now.max(1), Ordering::Relaxed);
}

/// Mark whether the transmitter is currently keyed by the wire link (not by PTT).
#[inline(always)]
pub fn set_wire_tx(active: bool) {
    WIRE_TX_ACTIVE.store(active, Ordering::Relaxed);
}

#[inline(always)]
pub(super) fn should_inhibit_deep_sleep(
    headless: bool,
    usb_connected: bool,
    wire_session_id: u32,
    last_wire_ms: u32,
    now: u32,
) -> bool {
    if headless || usb_connected {
        return true;
    }
    wire_session_id != 0
        && last_wire_ms != 0
        && now.wrapping_sub(last_wire_ms) < REMOTE_SLEEP_INHIBIT_MS
}

/// `true` when a wire-keyed transmission has outlived its link.
#[inline(always)]
pub(super) fn tx_link_lost(wire_tx: bool, transmitting: bool, last_wire_ms: u32, now: u32) -> bool {
    wire_tx && transmitting && now.wrapping_sub(last_wire_ms) >= REMOTE_TX_LINK_TIMEOUT_MS
}

#[allow(unused_variables)]
pub(super) fn is_remote_control_active(hw: &SystemHardware, now: u32) -> bool {
    #[cfg(feature = "usb")]
    let usb_connected = hw.usb.is_connected();
    #[cfg(not(feature = "usb"))]
    let usb_connected = false;

    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    let session_id = hw.wire.session.active_session_id;
    #[cfg(not(any(feature = "uart", feature = "usb_cdc")))]
    let session_id = 0u32;

    should_inhibit_deep_sleep(
        super::is_headless(),
        usb_connected,
        session_id,
        LAST_WIRE_ACTIVITY_MS.load(Ordering::Relaxed),
        now,
    )
}

/// Failsafe: unkey a wire-keyed transmitter whose controlling link went silent.
#[inline(never)]
pub(super) fn enforce_tx_failsafe(hw: &mut SystemHardware, now: u32) {
    let coord = &mut crate::radio_state().coord;
    if tx_link_lost(
        WIRE_TX_ACTIVE.load(Ordering::Relaxed),
        coord.is_transmitting,
        LAST_WIRE_ACTIVITY_MS.load(Ordering::Relaxed),
        now,
    ) {
        coord.stop_tx(&mut hw.radio);
        set_wire_tx(false);
    } else if !coord.is_transmitting {
        set_wire_tx(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_sleep_inhibit_window() {
        assert!(should_inhibit_deep_sleep(false, true, 0, 0, 1000));
        assert!(!should_inhibit_deep_sleep(false, false, 0, 0, 1000));
        assert!(should_inhibit_deep_sleep(
            false,
            false,
            0x4B45_524E,
            10_000,
            39_999
        ));
        assert!(!should_inhibit_deep_sleep(
            false,
            false,
            0x4B45_524E,
            10_000,
            40_000
        ));
        assert!(should_inhibit_deep_sleep(
            false,
            false,
            0x4B45_524E,
            u32::MAX - 5_000,
            10_000
        ));
        assert!(!should_inhibit_deep_sleep(
            false,
            false,
            0x4B45_524E,
            u32::MAX - 5_000,
            30_000
        ));
    }

    #[test]
    fn headless_node_never_sleeps_itself_off_the_link() {
        // No USB, no session, long idle: an attended radio may sleep, a headless node never.
        assert!(!should_inhibit_deep_sleep(false, false, 0, 0, 3_600_000));
        assert!(should_inhibit_deep_sleep(true, false, 0, 0, 3_600_000));
    }

    #[test]
    fn wire_keyed_tx_drops_after_link_silence() {
        assert!(!tx_link_lost(true, true, 10_000, 14_999));
        assert!(tx_link_lost(true, true, 10_000, 15_000));
        // PTT-keyed (not wire) transmissions are never touched by the failsafe.
        assert!(!tx_link_lost(false, true, 10_000, 60_000));
        assert!(!tx_link_lost(true, false, 10_000, 60_000));
        // Wraparound.
        assert!(tx_link_lost(true, true, u32::MAX - 1_000, 4_000));
    }
}
