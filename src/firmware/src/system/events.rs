//! Lock-free system event queue shared by ISRs, RigWire, syscalls, and the main loop.

use crate::EVENT_QUEUE;
use kernel::event::Event;

#[inline(always)]
pub fn enqueue_event(event: Event) -> bool {
    EVENT_QUEUE.push(event)
}

#[inline(always)]
pub fn dequeue_event() -> Option<Event> {
    EVENT_QUEUE.pop()
}
