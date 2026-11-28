//! Lock-Free `SeqLock<RadioView>` (32 B) for Zero-Contention RF Telemetry.
//!
//! Updated inside `RadioCoordinator::tick_10ms`; read in $O(1)$ by UI status bars,
//! `LauncherApp`, `Op::SysProp(0xE0..=0xFE)`, and RigWire with zero `BK4829` bus contention.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

/// 32-byte (`2^5`) naturally-aligned live transceiver telemetry snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(4))]
pub struct RadioView {
    pub vfo_a_hz: u32,       // 0..4
    pub vfo_b_hz: u32,       // 4..8
    pub active_freq_hz: u32, // 8..12
    pub rssi_dbm: i16,       // 12..14
    pub raw_rssi: u16,       // 14..16
    pub noise: u8,           // 16
    pub glitch: u8,          // 17
    pub snr_db: i8,          // 18
    pub s_meter: u8,         // 19 (0..=9)
    pub active_vfo: u8,      // 20 (0=A, 1=B, 2=C)
    pub listening_vfo: u8,   // 21
    pub flags: u8,           // 22 (bit0=tx, bit1=rx_open, bit2=subaudible_match, bit3=dual_watch)
    pub modulation: u8,      // 23
    pub squelch_level: u8,   // 24
    pub tx_power: u8,        // 25
    pub band_idx: u8,        // 26
    pub _pad: [u8; 5],       // 27..32
}

const _: () = {
    assert!(core::mem::size_of::<RadioView>() == 32);
    assert!(core::mem::align_of::<RadioView>() == 4);
};

pub const RADIO_FLAG_TX: u8 = 1 << 0;
pub const RADIO_FLAG_RX_OPEN: u8 = 1 << 1;
pub const RADIO_FLAG_SUBAUDIBLE: u8 = 1 << 2;
pub const RADIO_FLAG_DUAL_WATCH: u8 = 1 << 3;

impl RadioView {
    pub const fn empty() -> Self {
        Self {
            vfo_a_hz: 145_500_000,
            vfo_b_hz: 433_500_000,
            active_freq_hz: 145_500_000,
            rssi_dbm: -120,
            raw_rssi: 0,
            noise: 0,
            glitch: 0,
            snr_db: 0,
            s_meter: 0,
            active_vfo: 0,
            listening_vfo: 0,
            flags: 0,
            modulation: 0,
            squelch_level: 3,
            tx_power: 0,
            band_idx: 2,
            _pad: [0; 5],
        }
    }

    #[inline(always)]
    pub const fn is_tx(&self) -> bool {
        (self.flags & RADIO_FLAG_TX) != 0
    }

    #[inline(always)]
    pub const fn is_rx_open(&self) -> bool {
        (self.flags & RADIO_FLAG_RX_OPEN) != 0
    }
}

/// Single-writer, multi-reader lock-free sequence lock for `Copy` telemetry snapshots.
pub struct SeqLock<T: Copy> {
    seq: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T: Copy + Send> Sync for SeqLock<T> {}
unsafe impl<T: Copy + Send> Send for SeqLock<T> {}

impl<T: Copy> SeqLock<T> {
    pub const fn new(initial: T) -> Self {
        Self {
            seq: AtomicU32::new(0),
            data: UnsafeCell::new(initial),
        }
    }

    /// Publish a fresh snapshot (single-writer only).
    #[inline]
    pub fn write(&self, val: T) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Release);
        unsafe {
            core::ptr::write_volatile(self.data.get(), val);
        }
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Lock-free consistent read; retries if a concurrent write occurred mid-copy.
    #[inline]
    pub fn read(&self) -> T {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if (s1 & 1) != 0 {
                continue;
            }
            let val = unsafe { core::ptr::read_volatile(self.data.get()) };
            core::sync::atomic::fence(Ordering::Acquire);
            let s2 = self.seq.load(Ordering::Relaxed);
            if s1 == s2 {
                return val;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seqlock_radio_view_roundtrip() {
        let lock = SeqLock::new(RadioView::empty());
        let mut v = lock.read();
        assert_eq!(v.vfo_a_hz, 145_500_000);
        v.rssi_dbm = -82;
        v.flags = RADIO_FLAG_RX_OPEN;
        lock.write(v);
        let out = lock.read();
        assert_eq!(out.rssi_dbm, -82);
        assert!(out.is_rx_open());
        assert!(!out.is_tx());
    }
}
