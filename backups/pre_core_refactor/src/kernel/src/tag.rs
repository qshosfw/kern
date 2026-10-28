//! Normative FourCC Identification Tag (`Tag([u8; 4])`).

/// 4-byte FourCC / Format Identifier Tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Tag(pub [u8; 4]);

impl Tag {
    pub const fn new(b0: u8, b1: u8, b2: u8, b3: u8) -> Self {
        Self([b0, b1, b2, b3])
    }

    pub const fn from_bytes(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }

    pub const fn as_u32_be(&self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    pub const fn as_u32_le(&self) -> u32 {
        u32::from_le_bytes(self.0)
    }
    pub fn as_str<'a>(&'a self, buf: &'a mut [u8; 4]) -> &'a str {
        *buf = self.0;
        core::str::from_utf8(buf).unwrap_or("????")
    }
}
