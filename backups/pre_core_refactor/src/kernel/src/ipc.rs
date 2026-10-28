//! Lock-Free Inter-Process Communication (IPC) primitives for the Microkernel.

use heapless::spsc::Queue;

/// A lock-free Single-Producer Single-Consumer (SPSC) message queue.
/// Safe for interrupt contexts and kernel task communication.
pub struct MessageQueue<T, const N: usize> {
    queue: Queue<T, N>,
}

impl<T, const N: usize> MessageQueue<T, N> {
    pub const fn new() -> Self {
        Self {
            queue: Queue::new(),
        }
    }

    /// Tries to push an item to the queue. Returns the item if the queue is full.
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        self.queue.enqueue(item)
    }

    /// Tries to pop an item from the queue.
    pub fn pop(&mut self) -> Option<T> {
        self.queue.dequeue()
    }

    /// Checks if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Checks if the queue is full.
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }

    /// Returns the number of items in the queue.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Total capacity of the queue.
    pub const fn capacity(&self) -> usize {
        N
    }
}

pub struct EventQueue<T, const N: usize> {
    inner: heapless::spsc::Queue<T, N>,
}

impl<T, const N: usize> EventQueue<T, N> {
    pub const fn new() -> Self {
        Self {
            inner: heapless::spsc::Queue::new(),
        }
    }

    /// Split into producer (for ISRs) and consumer (for main loop).
    pub fn split(
        &mut self,
    ) -> (
        heapless::spsc::Producer<'_, T, N>,
        heapless::spsc::Consumer<'_, T, N>,
    ) {
        self.inner.split()
    }

    /// Capacity of the event queue.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Number of items currently enqueued.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether queue is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// Generic lock-free inter-process communication event payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysEvent {
    pub target: u8,
    pub command: u8,
    pub arg: u16,
}

/// Bitmask-filtered publish/subscribe topic flags (§13.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Topic(pub u16);

impl Topic {
    pub const ALL: Self = Self(0xFFFF);
    pub const SYSTEM: Self = Self(1 << 0);
    pub const RADIO: Self = Self(1 << 1);
    pub const UI: Self = Self(1 << 2);
    pub const AUDIO: Self = Self(1 << 3);
    pub const POWER: Self = Self(1 << 4);
    pub const STORAGE: Self = Self(1 << 5);
    pub const NETWORK: Self = Self(1 << 6);
    pub const SENSORS: Self = Self(1 << 7);

    #[inline(always)]
    pub const fn matches(self, filter: Self) -> bool {
        (self.0 & filter.0) != 0
    }
}

/// Topic-filtered broadcast bus event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusEvent {
    pub topic: Topic,
    pub sender: u8,
    pub event_id: u8,
    pub data: u32,
}
