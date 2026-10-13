//! Application Bundle (`.app` / `.krx`) Format Specification.
//!
//! Provides metadata and executable descriptor headers for installable application
//! packages on KernFS, specifying required hardware capability tokens, memory limits,
//! and entry-point offsets.

pub use super::file_header::{FILE_ID_APP_BUNDLE, FILE_ID_APP_LEGACY, FILE_ID_KRX};

/// Standard 48-byte header for `.app` and `.krx` executable packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct AppHeader {
    pub ident: u32,
    pub version: u16,
    pub flags: u16,
    pub capabilities: u32,
    pub code_offset: u32,
    pub code_size: u32,
    pub data_size: u32,
    pub entry_point: u32,
    pub name: [u8; 16],
    pub reserved: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<AppHeader>() == 48);

impl AppHeader {
    pub const SIZE: usize = 48;

    // Capability requirement bitflags matching KERN capability tokens
    pub const CAP_RADIO: u32 = 1 << 0;
    pub const CAP_CRYPTO: u32 = 1 << 1;
    pub const CAP_AUDIO: u32 = 1 << 2;
    pub const CAP_STORAGE: u32 = 1 << 3;
    pub const CAP_UART: u32 = 1 << 4;
    pub const CAP_DISPLAY: u32 = 1 << 5;
    pub const CAP_KEYPAD: u32 = 1 << 6;
    pub const CAP_POWER: u32 = 1 << 7;

    pub const fn empty() -> Self {
        Self {
            ident: FILE_ID_KRX,
            version: 0x0100,
            flags: 0,
            capabilities: 0,
            code_offset: Self::SIZE as u32,
            code_size: 0,
            data_size: 0,
            entry_point: 0,
            name: [0; 16],
            reserved: [0; 4],
        }
    }

    pub fn new(name: &str, capabilities: u32, code_size: u32) -> Self {
        let mut hdr = Self::empty();
        hdr.capabilities = capabilities;
        hdr.code_size = code_size;
        hdr.set_name(name);
        hdr
    }

    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        (self.ident == FILE_ID_KRX || self.ident == FILE_ID_APP_LEGACY)
            && self.code_offset >= Self::SIZE as u32
    }

    pub fn name_as_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..len]).unwrap_or("")
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 16];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(15);
        self.name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 48] {
        unsafe { &*(self as *const Self as *const [u8; 48]) }
    }

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 48] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 48]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 48]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}
