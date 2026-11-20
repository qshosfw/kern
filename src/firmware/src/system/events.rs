//! Interrupt-safe system event queue shared by ISRs, the wire link and the main loop.

use crate::EVENT_QUEUE;
use kernel::event::Event;

#[inline(never)]
pub fn enqueue_event(event: Event) {
    #[cfg(target_arch = "arm")]
    cortex_m::interrupt::free(|cs| {
        let _ = EVENT_QUEUE.borrow(cs).borrow_mut().push_back(event);
    });
    #[cfg(not(target_arch = "arm"))]
    {
        let cs = unsafe { cortex_m::interrupt::CriticalSection::new() };
        let _ = EVENT_QUEUE.borrow(&cs).borrow_mut().push_back(event);
    }
}

#[inline(never)]
pub fn dequeue_event() -> Option<Event> {
    #[cfg(target_arch = "arm")]
    {
        cortex_m::interrupt::free(|cs| EVENT_QUEUE.borrow(cs).borrow_mut().pop_front())
    }
    #[cfg(not(target_arch = "arm"))]
    {
        let cs = unsafe { cortex_m::interrupt::CriticalSection::new() };
        EVENT_QUEUE.borrow(&cs).borrow_mut().pop_front()
    }
}
