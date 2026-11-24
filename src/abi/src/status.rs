//! Canonical single-byte microkernel status & error code space (`#[repr(u8)]`).
//!
//! Shared across `abi`, `kernel`, `kernfs`, `drivers`, RigWire v2, and `kern-sdk` so
//! every layer speaks one uniform error vocabulary with zero translation ladders.

/// Unified microkernel operation status (`0 = Ok`, `1..=17 = Error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    /// Operation completed successfully.
    Ok = 0,
    /// Invalid argument, parameter, or out-of-range value.
    InvalidArg = 1,
    /// Null, misaligned, or out-of-bounds guest pointer.
    BadPtr = 2,
    /// Buffer or payload length invalid or truncated.
    BadLength = 3,
    /// Required capability token or permission denied.
    PermissionDenied = 4,
    /// Resource or lease is currently held by another owner.
    Busy = 5,
    /// Requested file, process, service, or entry does not exist.
    NotFound = 6,
    /// Target already exists.
    AlreadyExists = 7,
    /// Flash, RAM, slot table, or queue capacity exhausted.
    NoSpace = 8,
    /// Checksum, MAC, or format validation failed.
    CorruptData = 9,
    /// Low-level hardware or storage I/O failure.
    IoError = 10,
    /// Hardware response or execution deadline timed out.
    Timeout = 11,
    /// Operation refused due to low battery or brownout guard.
    LowPower = 12,
    /// RF transmission or destructive action blocked by safety policy.
    SafetyBlocked = 13,
    /// Unknown or unsupported operation, profile, or feature.
    Unsupported = 14,
    /// Missing, expired, or mismatched transport session.
    BadSession = 15,
    /// Package ABI version does not match the running kernel.
    IncompatibleAbi = 16,
    /// Execution fault or stack boundary depletion.
    Faulted = 17,
}

impl Status {
    /// Highest assigned discriminant.
    pub const MAX_CODE: u8 = Self::Faulted as u8;

    #[inline(always)]
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }

    #[inline(always)]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// Encode as a signed `kern_call` return value (`0` for `Ok`, `-code` for errors).
    #[inline(always)]
    pub const fn to_ret(self) -> isize {
        -(self as u8 as isize)
    }

    /// Encode as a raw `usize` `kern_call` return (`0` for `Ok`, `(-code) as usize` for errors).
    #[inline(always)]
    pub const fn to_raw(self) -> usize {
        self.to_ret() as usize
    }

    /// True if `raw` lies in the reserved ABI error band `[-MAX_CODE ..= -1]`.
    #[inline(always)]
    pub const fn is_err_raw(raw: usize) -> bool {
        let s = raw as isize;
        s < 0 && s >= -(Self::MAX_CODE as isize)
    }

    /// Decode a raw `kern_call` return value into `Ok(value)` or `Err(Status)`.
    #[inline(always)]
    pub const fn from_raw(raw: usize) -> Result<usize, Self> {
        let s = raw as isize;
        if s < 0 && s >= -(Self::MAX_CODE as isize) {
            Err(Self::from_code((-s) as u8))
        } else {
            Ok(raw)
        }
    }

    #[inline(always)]
    pub const fn from_code(code: u8) -> Self {
        if code <= Self::MAX_CODE {
            // SAFETY: Discriminants `0..=MAX_CODE` are contiguous `#[repr(u8)]` variants.
            unsafe { core::mem::transmute::<u8, Self>(code) }
        } else {
            Self::IoError
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_raw_roundtrip() {
        assert_eq!(Status::Ok.to_raw(), 0);
        assert_eq!(Status::from_raw(0), Ok(0));
        assert_eq!(Status::from_raw(42), Ok(42));
        for code in 1..=Status::MAX_CODE {
            let st = Status::from_code(code);
            let raw = st.to_raw();
            assert!(Status::is_err_raw(raw));
            assert_eq!(Status::from_raw(raw), Err(st));
        }
        // Values outside the reserved negative band are preserved as Ok raw u32 values.
        assert!(!Status::is_err_raw((-100isize) as usize));
        assert_eq!(Status::from_raw(0xDEAD_BEEF_u32 as usize), Ok(0xDEAD_BEEF_u32 as usize));
    }
}
