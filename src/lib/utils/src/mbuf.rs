use core::cell::UnsafeCell;

pub const MBUF_SLAB_SIZE: usize = 128;
pub const MBUF_POOL_CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MBufToken {
    pub index: u8,
    pub length: u16,
}

struct SlabSlot {
    busy: UnsafeCell<bool>,
    data: UnsafeCell<[u8; MBUF_SLAB_SIZE]>,
}

unsafe impl Sync for SlabSlot {}

/// Static MBuf Slab Allocator for zero-copy IPC and hardware frame buffers
pub struct MBufPool {
    slots: [SlabSlot; MBUF_POOL_CAPACITY],
}

impl MBufPool {
    pub const fn new() -> Self {
        #[allow(clippy::declare_interior_mutable_const)]
        const UNINIT_SLOT: SlabSlot = SlabSlot {
            busy: UnsafeCell::new(false),
            data: UnsafeCell::new([0u8; MBUF_SLAB_SIZE]),
        };
        Self {
            slots: [UNINIT_SLOT; MBUF_POOL_CAPACITY],
        }
    }

    /// Allocate an MBuf slot from the slab pool
    pub fn alloc(&self) -> Option<MBufToken> {
        cortex_m::interrupt::free(|_| {
            for (idx, slot) in self.slots.iter().enumerate() {
                unsafe {
                    let busy = &mut *slot.busy.get();
                    if !*busy {
                        *busy = true;
                        return Some(MBufToken {
                            index: idx as u8,
                            length: 0,
                        });
                    }
                }
            }
            None
        })
    }

    /// Free an MBuf slot back to the slab pool
    pub fn free(&self, token: MBufToken) {
        cortex_m::interrupt::free(|_| {
            let idx = token.index as usize;
            if idx < MBUF_POOL_CAPACITY {
                unsafe {
                    *self.slots[idx].busy.get() = false;
                }
            }
        });
    }

    /// Count how many free slots are available in the slab pool
    pub fn free_count(&self) -> usize {
        cortex_m::interrupt::free(|_| {
            let mut count = 0;
            for slot in self.slots.iter() {
                unsafe {
                    if !*slot.busy.get() {
                        count += 1;
                    }
                }
            }
            count
        })
    }

    /// Read data from an allocated MBuf slot into a buffer
    pub fn read(&self, token: MBufToken, out: &mut [u8]) -> Result<usize, ()> {
        let idx = token.index as usize;
        if idx >= MBUF_POOL_CAPACITY {
            return Err(());
        }
        unsafe {
            if !*self.slots[idx].busy.get() {
                return Err(());
            }
            let copy_len = (token.length as usize).min(out.len()).min(MBUF_SLAB_SIZE);
            let src = &*self.slots[idx].data.get();
            out[..copy_len].copy_from_slice(&src[..copy_len]);
            Ok(copy_len)
        }
    }

    /// Write data into an allocated MBuf slot
    pub fn write(&self, token: &mut MBufToken, data: &[u8]) -> Result<(), ()> {
        let idx = token.index as usize;
        if idx >= MBUF_POOL_CAPACITY {
            return Err(());
        }
        unsafe {
            if !*self.slots[idx].busy.get() {
                return Err(());
            }
            let write_len = data.len().min(MBUF_SLAB_SIZE);
            let dst = &mut *self.slots[idx].data.get();
            dst[..write_len].copy_from_slice(&data[..write_len]);
            token.length = write_len as u16;
            Ok(())
        }
    }
}
