//! Volatile Zeroization for Secret Buffers
//!
//! Provides `SecretBuffer` and `SecretBuffer16` that explicitly overwrite contents with zeroes
//! when dropped, using `core::ptr::write_volatile` to prevent compiler optimization.

/// A 32-byte secret buffer (used for 256-bit keys and master secrets) that zeroizes on drop.
pub struct SecretBuffer {
    data: [u8; 32],
}

impl SecretBuffer {
    pub const fn new() -> Self {
        Self { data: [0u8; 32] }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8; 32] {
        &mut self.data
    }

    pub fn as_slice(&self) -> &[u8; 32] {
        &self.data
    }

    pub fn zeroize(&mut self) {
        unsafe {
            for byte in self.data.iter_mut() {
                core::ptr::write_volatile(byte, 0);
            }
        }
    }
}

impl Drop for SecretBuffer {
    fn drop(&mut self) {
        unsafe {
            for byte in self.data.iter_mut() {
                core::ptr::write_volatile(byte, 0);
            }
        }
    }
}

impl Clone for SecretBuffer {
    fn clone(&self) -> Self {
        let mut new_buf = Self::new();
        new_buf.data.copy_from_slice(&self.data);
        new_buf
    }
}

/// A 16-byte secret buffer (used for 128-bit Ascon keys) that zeroizes on drop.
pub struct SecretBuffer16 {
    data: [u8; 16],
}

impl SecretBuffer16 {
    pub const fn new() -> Self {
        Self { data: [0u8; 16] }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8; 16] {
        &mut self.data
    }

    pub fn as_slice(&self) -> &[u8; 16] {
        &self.data
    }

    pub fn zeroize(&mut self) {
        unsafe {
            for byte in self.data.iter_mut() {
                core::ptr::write_volatile(byte, 0);
            }
        }
    }
}

impl Drop for SecretBuffer16 {
    fn drop(&mut self) {
        unsafe {
            for byte in self.data.iter_mut() {
                core::ptr::write_volatile(byte, 0);
            }
        }
    }
}

impl Clone for SecretBuffer16 {
    fn clone(&self) -> Self {
        let mut new_buf = Self::new();
        new_buf.data.copy_from_slice(&self.data);
        new_buf
    }
}
