//! Zero-copy single-register wire payload cursor for RigWire v2.

use abi::Status;

pub struct WireCursor<'a> {
    pub inp: &'a [u8],
    pub out: &'a mut [u8],
    pub pos: usize,
    pub max_out: usize,
}

impl<'a> WireCursor<'a> {
    #[inline(always)]
    pub fn new(inp: &'a [u8], out: &'a mut [u8], max_out: usize) -> Self {
        let cap = out.len().min(max_out);
        Self {
            inp,
            out,
            pos: 0,
            max_out: cap,
        }
    }

    #[inline(never)]
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Status> {
        if self.inp.len() < n {
            return Err(Status::BadLength);
        }
        let head = unsafe { core::slice::from_raw_parts(self.inp.as_ptr(), n) };
        self.inp =
            unsafe { core::slice::from_raw_parts(self.inp.as_ptr().add(n), self.inp.len() - n) };
        Ok(head)
    }

    #[inline(never)]
    pub fn rd_le(&mut self, n: usize) -> Result<u32, Status> {
        let s = self.take(n)?;
        let mut v = 0u32;
        let mut i = n;
        while i > 0 {
            i -= 1;
            v = (v << 8) | unsafe { *s.get_unchecked(i) } as u32;
        }
        Ok(v)
    }

    #[inline(never)]
    pub fn rd_path(&mut self) -> Result<&'a str, Status> {
        let len = self.rd_u8()? as usize;
        utils::format::ascii_str(self.take(len)?).ok_or(Status::BadLength)
    }

    #[inline(always)]
    pub fn rd_u8(&mut self) -> Result<u8, Status> {
        Ok(self.rd_le(1)? as u8)
    }

    #[inline(always)]
    pub fn rd_u16(&mut self) -> Result<u16, Status> {
        Ok(self.rd_le(2)? as u16)
    }

    #[inline(always)]
    pub fn rd_u32(&mut self) -> Result<u32, Status> {
        self.rd_le(4)
    }

    #[inline(never)]
    pub fn put(&mut self, data: &[u8]) {
        let n = data.len().min(self.max_out.saturating_sub(self.pos));
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), self.out.as_mut_ptr().add(self.pos), n);
        }
        self.pos += n;
    }

    #[inline(never)]
    pub fn zeroes(&mut self, len: usize) {
        let n = len.min(self.max_out.saturating_sub(self.pos));
        unsafe {
            core::ptr::write_bytes(self.out.as_mut_ptr().add(self.pos), 0, n);
        }
        self.pos += n;
    }

    #[inline(never)]
    pub fn wr_le(&mut self, mut v: u32, n: usize) {
        let count = n.min(self.max_out.saturating_sub(self.pos));
        let p = unsafe { self.out.as_mut_ptr().add(self.pos) };
        let mut i = 0usize;
        while i < count {
            unsafe { *p.add(i) = v as u8 };
            v >>= 8;
            i += 1;
        }
        self.pos += count;
    }

    #[inline(always)]
    pub fn wr_u8(&mut self, v: u8) {
        self.wr_le(v as u32, 1);
    }

    #[inline(always)]
    pub fn wr_u16(&mut self, v: u16) {
        self.wr_le(v as u32, 2);
    }

    #[inline(always)]
    pub fn wr_u32(&mut self, v: u32) {
        self.wr_le(v, 4);
    }
}
