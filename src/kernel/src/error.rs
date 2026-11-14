//! High-assurance microkernel error codes and taxonomy.

/// Standard microkernel error code returned across kernel substrates and system calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KernelError {
    /// Generic unspecified error
    Unspecified = 1,
    /// Permission or capability denied
    AccessDenied = 2,
    /// Resource or object not found
    NotFound = 3,
    /// Resource exhausted or full
    OutOfResources = 4,
    /// Hardware / peripheral timeout or bus error
    HardwareTimeout = 5,
    /// Memory corrupt or bad CRC
    CorruptData = 6,
    /// Invalid argument or parameter
    InvalidArgument = 7,
    /// Stack boundary breach or margin depleted
    StackDepleted = 8,
    /// Execution deadline or watchdog exceeded
    DeadlineExceeded = 9,
    /// Process or actor stopped
    Stopped = 10,
}

const ERROR_NAMES: &[u8] = b"\
Unspecified\0\
Access Denied\0\
Not Found\0\
Out of Res\0\
HW Timeout\0\
Corrupt Data\0\
Invalid Arg\0\
Stack Low\0\
Deadline Exceeded\0\
Stopped\0";

impl KernelError {
    pub const fn to_code(self) -> u8 {
        self as u8
    }

    pub const fn from_code(code: u8) -> Self {
        match code {
            2 => Self::AccessDenied,
            3 => Self::NotFound,
            4 => Self::OutOfResources,
            5 => Self::HardwareTimeout,
            6 => Self::CorruptData,
            7 => Self::InvalidArgument,
            8 => Self::StackDepleted,
            9 => Self::DeadlineExceeded,
            10 => Self::Stopped,
            _ => Self::Unspecified,
        }
    }

    pub const fn as_str(self) -> &'static str {
        utils::format::nth_cstr(ERROR_NAMES, (self as u8) - 1)
    }
}
