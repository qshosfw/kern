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
}

/// Generic lock-free inter-process communication event payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysEvent {
    pub target: u8,
    pub command: u8,
    pub arg: u16,
}
