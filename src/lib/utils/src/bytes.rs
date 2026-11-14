//! Bounded zero-allocation byte reader and writer utilities.

/// Bounded byte slice writer with cursor tracking.
#[derive(Debug)]
pub struct ByteWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> ByteWriter<'a> {
    #[inline(always)]
    pub const fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline(always)]
    pub const fn position(&self) -> usize {
        self.pos
    }

    #[inline(always)]
    pub const fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        self.buf.len()
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.pos]
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buf[..self.pos]
    }

    /// Core non-inlined byte slice write to minimize code size.
    #[inline(never)]
    pub fn write_bytes(&mut self, src: &[u8]) -> bool {
        let len = src.len();
        if self.remaining() >= len {
            self.buf[self.pos..self.pos + len].copy_from_slice(src);
            self.pos += len;
            true
        } else {
            false
        }
    }

    #[inline(never)]
    pub fn write_u8(&mut self, val: u8) -> bool {
        if self.pos < self.buf.len() {
            self.buf[self.pos] = val;
            self.pos += 1;
            true
        } else {
            false
        }
    }

    #[inline(never)]
    fn write_le32(&mut self, mut val: u32, n: usize) -> bool {
        if self.remaining() >= n {
            for i in 0..n {
                self.buf[self.pos + i] = val as u8;
                val >>= 8;
            }
            self.pos += n;
            true
        } else {
            false
        }
    }

    #[inline(never)]
    pub fn write_u16_le(&mut self, val: u16) -> bool {
        self.write_le32(val as u32, 2)
    }

    #[inline(never)]
    pub fn write_u32_le(&mut self, val: u32) -> bool {
        self.write_le32(val, 4)
    }

    #[inline(never)]
    pub fn write_u64_le(&mut self, val: u64) -> bool {
        self.write_bytes(&val.to_le_bytes())
    }

    #[inline(always)]
    pub fn write_i16_le(&mut self, val: i16) -> bool {
        self.write_u16_le(val as u16)
    }

    #[inline(always)]
    pub fn write_i32_le(&mut self, val: i32) -> bool {
        self.write_u32_le(val as u32)
    }

    #[inline(never)]
    pub fn write_zeroes(&mut self, count: usize) -> bool {
        if self.remaining() >= count {
            self.buf[self.pos..self.pos + count].fill(0);
            self.pos += count;
            true
        } else {
            false
        }
    }
}

/// Bounded byte slice reader with cursor tracking.
#[derive(Debug, Clone, Copy)]
pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    #[inline(always)]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline(always)]
    pub const fn position(&self) -> usize {
        self.pos
    }

    #[inline(always)]
    pub const fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    #[inline(always)]
    pub fn remaining_slice(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    #[inline(never)]
    pub fn read_bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.remaining() >= len {
            let slice = &self.buf[self.pos..self.pos + len];
            self.pos += len;
            Some(slice)
        } else {
            None
        }
    }

    #[inline(never)]
    pub fn read_u8(&mut self) -> Option<u8> {
        if self.pos < self.buf.len() {
            let val = self.buf[self.pos];
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }

    #[inline(never)]
    pub fn read_u16_le(&mut self) -> Option<u16> {
        let bytes = self.read_bytes(2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    #[inline(never)]
    pub fn read_u32_le(&mut self) -> Option<u32> {
        let bytes = self.read_bytes(4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    #[inline(never)]
    pub fn read_u64_le(&mut self) -> Option<u64> {
        let bytes = self.read_bytes(8)?;
        Some(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    #[inline(always)]
    pub fn read_i16_le(&mut self) -> Option<i16> {
        self.read_u16_le().map(|v| v as i16)
    }

    #[inline(always)]
    pub fn read_i32_le(&mut self) -> Option<i32> {
        self.read_u32_le().map(|v| v as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_writer_basic() {
        let mut buf = [0u8; 16];
        let mut w = ByteWriter::new(&mut buf);
        assert!(w.write_u8(0x42));
        assert!(w.write_u16_le(0x1234));
        assert!(w.write_u32_le(0xDEADBEEF));
        assert_eq!(w.position(), 7);
        assert_eq!(
            &w.as_slice()[..7],
            &[0x42, 0x34, 0x12, 0xEF, 0xBE, 0xAD, 0xDE]
        );
    }

    #[test]
    fn test_byte_reader_basic() {
        let data = [0x42, 0x34, 0x12, 0xEF, 0xBE, 0xAD, 0xDE];
        let mut r = ByteReader::new(&data);
        assert_eq!(r.read_u8(), Some(0x42));
        assert_eq!(r.read_u16_le(), Some(0x1234));
        assert_eq!(r.read_u32_le(), Some(0xDEADBEEF));
        assert_eq!(r.read_u8(), None);
    }
}
