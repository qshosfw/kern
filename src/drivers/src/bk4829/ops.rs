//! Declarative `RegPatch` Tables, Non-Blocking `SweepCursor`, and `TIM2` `SymbolQueue`.
//!
//! Eliminates imperative register-write boilerplate and blocking delays during spectrum
//! sweeps and CW/DTMF/5-Tone/MDC1200 symbol synthesis.

/// Declarative masked register write descriptor (`4 B` naturally aligned).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct RegPatch {
    pub reg: u8,
    pub _pad: u8,
    pub mask: u16,
    pub val: u16,
}

impl RegPatch {
    pub const fn set(reg: u8, val: u16) -> Self {
        Self {
            reg,
            _pad: 0,
            mask: 0xFFFF,
            val,
        }
    }

    pub const fn masked(reg: u8, mask: u16, val: u16) -> Self {
        Self {
            reg,
            _pad: 0,
            mask,
            val,
        }
    }
}

/// Non-blocking single-step RF spectrum/scanner sweep cursor (1 frequency step per tick).
#[derive(Debug, Clone, Copy)]
pub struct SweepCursor {
    pub start_hz: u32,
    pub step_hz: u32,
    pub total_steps: u16,
    pub cur_step: u16,
    pub active: bool,
}

impl SweepCursor {
    pub const fn new() -> Self {
        Self {
            start_hz: 0,
            step_hz: 0,
            total_steps: 0,
            cur_step: 0,
            active: false,
        }
    }

    pub fn start(&mut self, start_hz: u32, step_hz: u32, total_steps: u16) {
        self.start_hz = start_hz;
        self.step_hz = step_hz;
        self.total_steps = total_steps;
        self.cur_step = 0;
        self.active = total_steps > 0;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Return the current step `(index, freq_hz)` and advance the cursor by one step.
    pub fn next_step(&mut self) -> Option<(u16, u32)> {
        if !self.active || self.cur_step >= self.total_steps {
            self.active = false;
            return None;
        }
        let idx = self.cur_step;
        let freq = self
            .start_hz
            .saturating_add((idx as u32).saturating_mul(self.step_hz));
        self.cur_step += 1;
        if self.cur_step >= self.total_steps {
            self.active = false;
        }
        Some((idx, freq))
    }
}

/// Timed RF tone/CW symbol for zero-jitter `TIM2` ISR-paced synthesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TimedSymbol {
    pub freq_hz: u16,
    pub duration_ms: u16,
}

/// Fixed-capacity zero-heap ring buffer for `TIM2`-paced CW/DTMF/5-Tone/MDC1200 symbols.
pub struct SymbolQueue<const N: usize> {
    buf: [TimedSymbol; N],
    head: u8,
    tail: u8,
    count: u8,
    remaining_ms: u16,
}

impl<const N: usize> SymbolQueue<N> {
    pub const fn new() -> Self {
        Self {
            buf: [TimedSymbol {
                freq_hz: 0,
                duration_ms: 0,
            }; N],
            head: 0,
            tail: 0,
            count: 0,
            remaining_ms: 0,
        }
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.count == 0 && self.remaining_ms == 0
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
        self.remaining_ms = 0;
    }

    pub fn push(&mut self, sym: TimedSymbol) -> bool {
        if (self.count as usize) >= N {
            return false;
        }
        self.buf[self.head as usize] = sym;
        self.head = ((self.head as usize + 1) % N) as u8;
        self.count += 1;
        true
    }

    /// Advance by `dt_ms`; returns `Some(freq_hz)` whenever the active symbol changes (`0` = silence).
    pub fn tick(&mut self, dt_ms: u16) -> Option<u16> {
        if self.remaining_ms > 0 {
            self.remaining_ms = self.remaining_ms.saturating_sub(dt_ms);
            if self.remaining_ms > 0 {
                return None;
            }
        }
        if self.count == 0 {
            return None;
        }
        let sym = self.buf[self.tail as usize];
        self.tail = ((self.tail as usize + 1) % N) as u8;
        self.count -= 1;
        self.remaining_ms = sym.duration_ms;
        Some(sym.freq_hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_cursor_and_symbol_queue_step_deterministically() {
        let mut sw = SweepCursor::new();
        sw.start(144_000_000, 25_000, 3);
        assert_eq!(sw.next_step(), Some((0, 144_000_000)));
        assert_eq!(sw.next_step(), Some((1, 144_025_000)));
        assert_eq!(sw.next_step(), Some((2, 144_050_000)));
        assert_eq!(sw.next_step(), None);

        let mut q = SymbolQueue::<4>::new();
        assert!(q.push(TimedSymbol {
            freq_hz: 700,
            duration_ms: 60
        }));
        assert!(q.push(TimedSymbol {
            freq_hz: 0,
            duration_ms: 40
        }));
        assert_eq!(q.tick(10), Some(700));
        assert_eq!(q.tick(30), None);
        assert_eq!(q.tick(30), Some(0));
        assert_eq!(q.tick(40), None);
        assert!(q.is_empty());
    }
}
