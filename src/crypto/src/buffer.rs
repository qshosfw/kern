//! Volatile Zeroization for Secret Buffers
//!
//! Provides generic `SecretBuf<const N: usize>` that explicitly overwrites contents with zeroes
//! when dropped, using `core::ptr::write_volatile` to prevent compiler optimization.

#[inline(never)]
pub fn zeroize_bytes(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        unsafe {
            core::ptr::write_volatile(byte, 0);
        }
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Fixed-size secret buffer that zeroizes its contents on drop.
pub struct SecretBuf<const N: usize> {
    data: [u8; N],
}

impl<const N: usize> SecretBuf<N> {
    pub const fn new() -> Self {
        Self { data: [0u8; N] }
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u8; N] {
        &mut self.data
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[u8; N] {
        &self.data
    }

    pub fn zeroize(&mut self) {
        zeroize_bytes(&mut self.data);
    }
}

impl<const N: usize> Default for SecretBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Drop for SecretBuf<N> {
    fn drop(&mut self) {
        zeroize_bytes(&mut self.data);
    }
}

impl<const N: usize> Clone for SecretBuf<N> {
    fn clone(&self) -> Self {
        let mut new_buf = Self::new();
        new_buf.data.copy_from_slice(&self.data);
        new_buf
    }
}
