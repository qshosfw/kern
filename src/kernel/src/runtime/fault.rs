//! Contained Application Exception Descriptors.
//!
//! Captures hardware faults (`HardFault` inside guest code) and runtime contract
//! violations (`Denied`, `BadPtr`, `BadArg`, `UnleasedBus`, `NoOp`, `Stack`, `Hang`)
//! so the firmware can unwind the offending KRX process, revoke its leases, and
//! display an interactive Exception Dialog without rebooting the radio.

use crate::proc::Pid;

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

utils::static_bytes!(
    static EXCEPTION_LABELS =
        b"OK\0HardFault\0Stack\0Hang\0Cap Denied\0Bad Ptr\0Bad Arg\0Unleased\0Bad Op\0";
);

utils::static_bytes!(
    static EXCEPTION_DESCS = b"\
ok.\0\
crashed.\nInvalid memory/code.\0\
stack overflow.\0\
hung & closed.\0\
permission denied.\0\
bad pointer.\0\
invalid call arg.\0\
unleased hardware.\0\
unknown API call.\0";
);

impl Exception {
    #[inline(always)]
    pub fn label(self) -> &'static str {
        utils::format::nth_cstr(&EXCEPTION_LABELS, self as u8)
    }

    #[inline(always)]
    pub fn description(self) -> &'static str {
        utils::format::nth_cstr(&EXCEPTION_DESCS, self as u8)
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

    /// Formats a human-readable exception summary (e.g.
    /// `"scanner crashed unexpectedly.\nInvalid memory or code.\nAt +0x01A4"`)
    /// into `out` without heap allocation or `core::fmt`.
    #[inline(never)]
    pub fn format_message<'a>(&self, app_name: &str, out: &'a mut [u8; 96]) -> &'a str {
        let mut pos = 0usize;
        let name = if app_name.is_empty() { "App" } else { app_name };
        for s in [
            name.as_bytes(),
            b" " as &[u8],
            self.kind.description().as_bytes(),
        ] {
            let n = s.len().min(out.len() - pos);
            out[pos..pos + n].copy_from_slice(&s[..n]);
            pos += n;
        }
        let is_hw_fault = matches!(self.kind, Exception::Fault);
        let val = if is_hw_fault {
            self.pc_off
        } else {
            self.op as u16
        };
        let detail_prefix: &[u8] = if is_hw_fault {
            b"\nAt +0x"
        } else {
            b"\nCall 0x"
        };
        if val != 0 && pos + detail_prefix.len() + 4 <= out.len() {
            let hex = utils::format::HEX_CHARS;
            out[pos..pos + detail_prefix.len()].copy_from_slice(detail_prefix);
            pos += detail_prefix.len();
            let mut shift = 12i8;
            while shift >= 0 {
                out[pos] = hex[((val >> shift) & 0xF) as usize];
                pos += 1;
                shift -= 4;
            }
        }

        // SAFETY: all pushed bytes are valid UTF-8 (`name` is &str, descriptions/hex are ASCII).
        unsafe { core::str::from_utf8_unchecked(&out[..pos]) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_message_renders_fault_and_contract_details() {
        let mut buf = [0u8; 96];
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
            "scanner crashed.\nInvalid memory/code.\nAt +0x01A4"
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
            "demo permission denied.\nCall 0x0042"
        );
        assert!(!AppFault::NONE.is_active());
    }
}
