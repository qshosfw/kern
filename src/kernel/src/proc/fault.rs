//! Contained Application Exception Descriptors.
//!
//! Captures hardware faults (`HardFault` inside guest code) and runtime contract
//! violations (`Denied`, `BadPtr`, `BadArg`, `UnleasedBus`, `NoOp`, `Stack`, `Hang`)
//! so the firmware can unwind the offending KRX process, revoke its leases, and
//! display an interactive Exception Dialog without rebooting the radio.

use super::Pid;

/// Sentinel return value produced when a guest invocation unwinds via fault landing.
pub const KRX_FAULT_SENTINEL: u32 = 0xFFFF_FFFE;

/// Classification of contained KRX application exceptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exception {
    None = 0,
    Fault = 1,
    Stack = 2,
    Hang = 3,
    Denied = 4,
    BadPtr = 5,
    BadArg = 6,
    UnleasedBus = 7,
    NoOp = 8,
}

const EXCEPTION_LABELS: &[u8] =
    b"OK\0HardFault\0Stack\0Hang\0Cap Denied\0Bad Ptr\0Bad Arg\0Unleased\0Bad Op\0";

impl Exception {
    #[inline(always)]
    pub fn label(self) -> &'static str {
        utils::format::nth_cstr(EXCEPTION_LABELS, self as u8)
    }
}

/// Compact fault record captured when a KRX application triggers a hardware or contract exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct AppFault {
    pub pid: Pid,
    pub kind: Exception,
    pub op: u8,
    pub _pad: u8,
    pub pc_off: u16,
    pub _pad2: u16,
    pub addr: u32,
}

impl AppFault {
    pub const NONE: Self = Self {
        pid: Pid(0),
        kind: Exception::None,
        op: 0,
        _pad: 0,
        pc_off: 0,
        _pad2: 0,
        addr: 0,
    };

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        !matches!(self.kind, Exception::None)
    }

    /// Formats a concise multi-line exception summary (e.g. `"myapp: HardFault\n+0x01A4"`)
    /// into `out` without heap allocation or `core::fmt`.
    #[inline(never)]
    pub fn format_message<'a>(&self, app_name: &str, out: &'a mut [u8; 64]) -> &'a str {
        let mut pos = 0usize;
        let name = if app_name.is_empty() { "App" } else { app_name };
        for s in [
            name.as_bytes(),
            b": " as &[u8],
            self.kind.label().as_bytes(),
        ] {
            for &b in s {
                if pos < out.len() {
                    out[pos] = b;
                    pos += 1;
                }
            }
        }
        if (self.pc_off != 0 || self.op != 0) && pos + 8 <= out.len() {
            let hex = utils::format::HEX_CHARS;
            out[pos] = b'\n';
            pos += 1;
            if matches!(self.kind, Exception::Fault) {
                let off = self.pc_off;
                out[pos] = b'+';
                out[pos + 1] = b'0';
                out[pos + 2] = b'x';
                out[pos + 3] = hex[((off >> 12) & 0xF) as usize];
                out[pos + 4] = hex[((off >> 8) & 0xF) as usize];
                out[pos + 5] = hex[((off >> 4) & 0xF) as usize];
                out[pos + 6] = hex[(off & 0xF) as usize];
                pos += 7;
            } else {
                let op = self.op;
                out[pos] = b'o';
                out[pos + 1] = b'p';
                out[pos + 2] = b' ';
                out[pos + 3] = b'0';
                out[pos + 4] = b'x';
                out[pos + 5] = hex[((op >> 4) & 0xF) as usize];
                out[pos + 6] = hex[(op & 0xF) as usize];
                pos += 7;
            }
        }

        // SAFETY: all pushed bytes are valid UTF-8 (`name` is &str, labels/hex are ASCII).
        unsafe { core::str::from_utf8_unchecked(&out[..pos]) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_message_renders_fault_and_contract_details() {
        let mut buf = [0u8; 64];
        let f = AppFault {
            pid: Pid(5),
            kind: Exception::Fault,
            op: 0,
            _pad: 0,
            pc_off: 0x01A4,
            _pad2: 0,
            addr: 0x2000_01A4,
        };
        assert!(f.is_active());
        assert_eq!(
            f.format_message("scanner", &mut buf),
            "scanner: HardFault\n+0x01A4"
        );

        let c = AppFault {
            pid: Pid(6),
            kind: Exception::Denied,
            op: 0x42,
            _pad: 0,
            pc_off: 0,
            _pad2: 0,
            addr: 0,
        };
        assert_eq!(
            c.format_message("demo", &mut buf),
            "demo: Cap Denied\nop 0x42"
        );
        assert!(!AppFault::NONE.is_active());
    }
}
