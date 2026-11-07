//! Zero-copy packet framing and serialization views for embedded telemetry & IPC.

/// Marker trait for zero-copy plain-old-data (POD) binary payloads.
///
/// # Safety
/// Implementing types must have a deterministic memory layout (`#[repr(C)]` or `#[repr(transparent)]`)
/// with no internal padding bytes or pointer references.
pub unsafe trait ZeroCopyPod: Copy + 'static {}

/// Safe zero-copy view over a slice of bytes.
pub struct PacketView<'a, T: ZeroCopyPod> {
    data: &'a T,
}

impl<'a, T: ZeroCopyPod> PacketView<'a, T> {
    /// Reinterpret a byte buffer as a typed payload reference without copying.
    pub fn from_bytes(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < core::mem::size_of::<T>() {
            return None;
        }

        // Align check
        let ptr = bytes.as_ptr();
        if !(ptr as usize).is_multiple_of(core::mem::align_of::<T>()) {
            return None;
        }

        let data = unsafe { &*(ptr as *const T) };
        Some(Self { data })
    }

    pub fn payload(&self) -> &T {
        self.data
    }
}

// Implement ZeroCopyPod for basic primitive types
unsafe impl ZeroCopyPod for u8 {}
unsafe impl ZeroCopyPod for u16 {}
unsafe impl ZeroCopyPod for u32 {}
unsafe impl ZeroCopyPod for i8 {}
unsafe impl ZeroCopyPod for i16 {}
unsafe impl ZeroCopyPod for i32 {}
