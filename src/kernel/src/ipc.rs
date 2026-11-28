//! Lock-Free Inter-Process Communication (IPC) & Event Ring Primitives for the Microkernel.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};

/// Single-Consumer, ISR-Safe Bounded Ring (`N` must be a power of two `<= 128`).
///
/// Replaces `Mutex<RefCell<Deque>>`, `MessageQueue`, and `EventQueue` with one
/// zero-panic ring buffer: `push` masks interrupts for 4 instructions so ISR and
/// thread producers never race on `head`, while single-consumer `pop` in thread
/// mode is 100% lock-free (`Acquire`/`Release`).
pub struct EventRing<T, const N: usize = 16> {
    head: AtomicU8,
    tail: AtomicU8,
    slots: UnsafeCell<[MaybeUninit<T>; N]>,
}

unsafe impl<T: Send, const N: usize> Sync for EventRing<T, N> {}

impl<T: Copy, const N: usize> EventRing<T, N> {
    const MASK: u8 = {
        assert!(
            N > 1 && N <= 128 && N.is_power_of_two(),
            "EventRing N must be 2^k <= 128"
        );
        (N - 1) as u8
    };

    pub const fn new() -> Self {
        Self {
            head: AtomicU8::new(0),
            tail: AtomicU8::new(0),
            slots: UnsafeCell::new([const { MaybeUninit::uninit() }; N]),
        }
    }

    pub fn push(&self, item: T) -> bool {
        #[cfg(target_arch = "arm")]
        {
            cortex_m::interrupt::free(|_| unsafe { self.push_inner(item) })
        }
        #[cfg(not(target_arch = "arm"))]
        {
            unsafe { self.push_inner(item) }
        }
    }

    #[inline(always)]
    unsafe fn push_inner(&self, item: T) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next = (head.wrapping_add(1)) & Self::MASK;
        if next == self.tail.load(Ordering::Acquire) {
            return false;
        }
        unsafe {
            (*self.slots.get())[head as usize].write(item);
        }
        self.head.store(next, Ordering::Release);
        true
    }

    #[inline(always)]
    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }
        let item = unsafe { (*self.slots.get())[tail as usize].assume_init_read() };
        self.tail
            .store((tail.wrapping_add(1)) & Self::MASK, Ordering::Release);
        Some(item)
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.tail.load(Ordering::Relaxed) == self.head.load(Ordering::Acquire)
    }
}

/// Generic lock-free inter-process communication event payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(4))]
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

impl BusEvent {
    #[inline(always)]
    pub const fn new(topic: Topic, sender: u8, event_id: u8, data: u32) -> Self {
        Self {
            topic,
            sender,
            event_id,
            data,
        }
    }

    #[inline(always)]
    pub const fn is_radio(&self) -> bool {
        self.topic.matches(Topic::RADIO)
    }

    #[inline(always)]
    pub const fn is_system(&self) -> bool {
        self.topic.matches(Topic::SYSTEM)
    }

    #[inline(always)]
    pub const fn is_network(&self) -> bool {
        self.topic.matches(Topic::NETWORK)
    }
}

/// Standard Radio Subsystem Event Identifiers for BusEvent (Topic::RADIO).
pub mod radio_events {
    pub const CARRIER_OPENED: u8 = 1;
    pub const CARRIER_CLOSED: u8 = 2;
    pub const DUAL_WATCH_SWITCHED: u8 = 3;
    pub const FSK_SYNC_DETECTED: u8 = 4;
    pub const FSK_FRAME_FINISHED: u8 = 5;
    pub const FSK_LOCK_RELEASED: u8 = 6;
    pub const LOCK_ACQUIRED: u8 = 7;
    pub const LOCK_RELEASED: u8 = 8;
    pub const LOCK_PREEMPTED: u8 = 9;
    pub const LOCK_RESTORED: u8 = 10;
    pub const PACKET_RECEIVED: u8 = 11;
    pub const TX_ENDED: u8 = 12;
    pub const TOT_WARNING: u8 = 13;
    pub const TOT_EXPIRED: u8 = 14;
}

// ── Structured Zero-Heap Microkernel IPC ─────────────────────────────────────

pub use ::abi::types::{KrxIpcMessage, ipc_flags, ipc_msg_type, ipc_target};

/// Zero-heap multi-process IPC Channel and Message Router (§8.4 / KRX 3.2).
/// Manages up to N concurrent 32-byte messages in strict FIFO order.
pub struct IpcChannel<const N: usize = 8> {
    len: usize,
    next_seq: u16,
    messages: [KrxIpcMessage; N],
}

impl<const N: usize> Default for IpcChannel<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> IpcChannel<N> {
    pub const fn new() -> Self {
        assert!(N > 0, "IpcChannel capacity must be non-zero");
        Self {
            len: 0,
            next_seq: 0,
            messages: [const { KrxIpcMessage::empty() }; N],
        }
    }

    /// Enqueue a message into the IPC channel, overriding `sender_pid`.
    /// If seq is 0, auto-assigns the next monotonic sequence number.
    pub fn send_from(&mut self, msg: &KrxIpcMessage, sender_pid: u8) -> bool {
        if self.len >= N {
            return false;
        }
        let slot = &mut self.messages[self.len];
        *slot = *msg;
        slot.sender_pid = sender_pid;
        if slot.seq == 0 {
            self.next_seq = self.next_seq.wrapping_add(1).max(1);
            slot.seq = self.next_seq;
        }
        self.len += 1;
        true
    }

    /// Enqueue a message into the IPC channel.
    /// If seq is 0, auto-assigns the next monotonic sequence number.
    pub fn send(&mut self, msg: KrxIpcMessage) -> bool {
        self.send_from(&msg, msg.sender_pid)
    }

    /// Poll for the oldest pending message targeted to `receiver_pid`.
    pub fn poll(&mut self, receiver_pid: u8) -> Option<KrxIpcMessage> {
        self.poll_filtered(receiver_pid, None)
    }

    /// Poll directly into `out`, avoiding stack/return copies of `KrxIpcMessage`.
    pub fn poll_into(&mut self, receiver_pid: u8, out: &mut KrxIpcMessage) -> bool {
        self.poll_filtered_into(receiver_pid, None, out)
    }

    /// Poll with optional filter on `msg_type`.
    pub fn poll_filtered(
        &mut self,
        receiver_pid: u8,
        filter_type: Option<u16>,
    ) -> Option<KrxIpcMessage> {
        let mut out = core::mem::MaybeUninit::<KrxIpcMessage>::uninit();
        if self.poll_filtered_into_ptr(receiver_pid, filter_type, out.as_mut_ptr()) {
            Some(unsafe { out.assume_init() })
        } else {
            None
        }
    }

    /// Poll with optional filter directly into `out`.
    pub fn poll_filtered_into(
        &mut self,
        receiver_pid: u8,
        filter_type: Option<u16>,
        out: &mut KrxIpcMessage,
    ) -> bool {
        self.poll_filtered_into_ptr(receiver_pid, filter_type, out as *mut KrxIpcMessage)
    }

    fn poll_filtered_into_ptr(
        &mut self,
        receiver_pid: u8,
        filter_type: Option<u16>,
        out_ptr: *mut KrxIpcMessage,
    ) -> bool {
        for idx in 0..self.len {
            let msg = &self.messages[idx];
            let is_recipient = msg.target_pid == receiver_pid
                || (receiver_pid != 0 && msg.target_pid == ipc_target::FOREGROUND_APP);
            let type_ok = filter_type.is_none_or(|t| msg.msg_type == t);
            if is_recipient && type_ok {
                unsafe {
                    core::ptr::copy_nonoverlapping(&self.messages[idx], out_ptr, 1);
                    for k in idx..self.len - 1 {
                        core::ptr::copy_nonoverlapping(
                            &self.messages[k + 1],
                            &mut self.messages[k],
                            1,
                        );
                    }
                }
                self.len -= 1;
                return true;
            }
        }
        false
    }

    /// Check if pending messages exist for `receiver_pid`.
    pub fn has_pending(&self, receiver_pid: u8) -> bool {
        for idx in 0..self.len {
            let msg = &self.messages[idx];
            if msg.target_pid == receiver_pid
                || (receiver_pid != 0 && msg.target_pid == ipc_target::FOREGROUND_APP)
            {
                return true;
            }
        }
        false
    }

    /// Reply directly to a received request message.
    pub fn reply(&mut self, req: &KrxIpcMessage, payload: &[u8], is_error: bool) -> bool {
        let rep = req.reply_to(payload, is_error);
        self.send(rep)
    }

    /// Purge any messages sent to or from a terminated process PID.
    pub fn purge_pid(&mut self, pid: u8) -> usize {
        let old_len = self.len;
        let mut write = 0usize;
        for read in 0..old_len {
            let msg = &self.messages[read];
            if msg.sender_pid != pid && msg.target_pid != pid {
                if write != read {
                    // `write < read`, so the two array slots cannot overlap.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            &self.messages[read],
                            &mut self.messages[write],
                            1,
                        );
                    }
                }
                write += 1;
            }
        }
        self.len = write;
        old_len - write
    }

    /// Number of occupied message slots.
    pub fn pending_count(&self) -> usize {
        self.len
    }

    /// Non-destructive peek at queued IPC slot `idx` for kernel observability.
    pub fn peek_at(&self, idx: usize) -> Option<&KrxIpcMessage> {
        if idx < self.len {
            Some(&self.messages[idx])
        } else {
            None
        }
    }
}

/// Typed cross-application intent kind for decoupled userland routing (Phase 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntentKind {
    None = 0,
    LaunchPackage = 1,
    OpenFile = 2,
    TuneFreq = 3,
    ShowSettingsPage = 4,
}

/// Fixed-size zero-allocation cross-app intent descriptor (`32 B` POD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Intent {
    pub kind: IntentKind,
    pub sender_pid: u8,
    pub payload_len: u8,
    pub flags: u8,
    pub arg_u32: u32,
    pub payload: [u8; 24],
}

const _: () = assert!(core::mem::size_of::<Intent>() == 32);

impl Intent {
    pub const fn tune_freq(sender_pid: u8, freq_hz: u32) -> Self {
        Self {
            kind: IntentKind::TuneFreq,
            sender_pid,
            payload_len: 0,
            flags: 0,
            arg_u32: freq_hz,
            payload: [0; 24],
        }
    }

    pub fn for_path(kind: IntentKind, sender_pid: u8, path: &str) -> Self {
        let mut payload = [0u8; 24];
        let b = path.as_bytes();
        let n = b.len().min(24);
        payload[..n].copy_from_slice(&b[..n]);
        Self {
            kind,
            sender_pid,
            payload_len: n as u8,
            flags: 0,
            arg_u32: 0,
            payload,
        }
    }

    #[inline(always)]
    pub fn payload_str(&self) -> &str {
        let n = (self.payload_len as usize).min(24);
        core::str::from_utf8(&self.payload[..n]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_message_size_and_geometry() {
        assert_eq!(core::mem::size_of::<KrxIpcMessage>(), 32);
        let msg = KrxIpcMessage::new(2, 5, ipc_msg_type::PING, b"HELLO");
        assert_eq!(msg.sender_pid, 2);
        assert_eq!(msg.target_pid, 5);
        assert_eq!(msg.msg_type, ipc_msg_type::PING);
        assert_eq!(msg.payload_len, 5);
        assert_eq!(&msg.payload[..5], b"HELLO");
        assert!(!msg.is_reply());
        assert!(!msg.is_broadcast());

        let reply = msg.reply_to(b"WORLD", false);
        assert_eq!(reply.sender_pid, 5);
        assert_eq!(reply.target_pid, 2);
        assert_eq!(reply.seq, msg.seq);
        assert!(reply.is_reply());
        assert!(!reply.is_error());
        assert_eq!(&reply.payload[..5], b"WORLD");
    }

    #[test]
    fn test_ipc_channel_send_poll_and_reply() {
        let mut mb = IpcChannel::<4>::new();
        assert_eq!(mb.pending_count(), 0);

        let msg1 = KrxIpcMessage::new(3, 7, ipc_msg_type::SERVICE_REQUEST, b"GET_STATUS");
        assert!(mb.send(msg1));
        assert_eq!(mb.pending_count(), 1);
        assert!(mb.has_pending(7));
        assert!(!mb.has_pending(8));

        // Poll for other PID returns None
        assert_eq!(mb.poll(8), None);

        // Poll for target PID 7 retrieves message
        let recv = mb.poll(7).expect("expected msg for PID 7");
        assert_eq!(recv.sender_pid, 3);
        assert_eq!(recv.target_pid, 7);
        assert_eq!(recv.msg_type, ipc_msg_type::SERVICE_REQUEST);
        assert_eq!(&recv.payload[..10], b"GET_STATUS");
        assert_eq!(mb.pending_count(), 0);

        // Reply to message
        assert!(mb.reply(&recv, b"STATUS_OK", false));
        assert_eq!(mb.pending_count(), 1);
        assert!(mb.has_pending(3));

        // Sender PID 3 polls reply
        let rep = mb.poll(3).expect("expected reply for PID 3");
        assert_eq!(rep.sender_pid, 7);
        assert_eq!(rep.target_pid, 3);
        assert_eq!(rep.seq, recv.seq);
        assert!(rep.is_reply());
        assert_eq!(&rep.payload[..9], b"STATUS_OK");
    }

    #[test]
    fn test_ipc_channel_saturation_and_purge() {
        let mut mb = IpcChannel::<4>::new();

        // Queue saturation
        for i in 0..4 {
            let m = KrxIpcMessage::new(10, 20 + i, 0x100, &[]);
            assert!(mb.send(m));
        }
        assert_eq!(mb.pending_count(), 4);
        assert!(!mb.send(KrxIpcMessage::empty())); // Full!

        // Purge PID 10
        assert_eq!(mb.purge_pid(10), 4);
        assert_eq!(mb.pending_count(), 0);
    }

    #[test]
    fn test_topic_matching_and_filtering() {
        assert!(Topic::RADIO.matches(Topic::RADIO));
        assert!(Topic::ALL.matches(Topic::RADIO));
        assert!(!Topic::UI.matches(Topic::RADIO));

        let bus_ev = BusEvent::new(Topic::RADIO, 2, radio_events::CARRIER_OPENED, 0);
        assert!(bus_ev.is_radio());
        assert!(!bus_ev.is_system());
        assert_eq!(bus_ev.sender, 2);
        assert_eq!(bus_ev.event_id, radio_events::CARRIER_OPENED);
    }

    #[test]
    fn test_ipc_channel_preserves_fifo_across_interleaved_polls() {
        let mut mb = IpcChannel::<4>::new();
        assert!(mb.send(KrxIpcMessage::new(1, 2, 1, b"A")));
        assert!(mb.send(KrxIpcMessage::new(1, 1, 2, b"B")));
        assert!(mb.send(KrxIpcMessage::new(1, 2, 3, b"C")));
        assert_eq!(mb.poll(1).unwrap().payload[0], b'B');
        assert!(mb.send(KrxIpcMessage::new(1, 2, 4, b"D")));
        assert!(mb.send(KrxIpcMessage::new(1, 2, 5, b"E")));

        assert_eq!(mb.poll(2).unwrap().payload[0], b'A');
        assert_eq!(mb.poll(2).unwrap().payload[0], b'C');
        assert_eq!(mb.poll(2).unwrap().payload[0], b'D');
        assert_eq!(mb.poll(2).unwrap().payload[0], b'E');
        assert_eq!(mb.poll(2), None);
    }

    #[test]
    fn test_intent_roundtrip_and_size() {
        let tune = Intent::tune_freq(3, 145_500_000);
        assert_eq!(tune.kind, IntentKind::TuneFreq);
        assert_eq!(tune.arg_u32, 145_500_000);
        assert_eq!(tune.payload_str(), "");

        let launch = Intent::for_path(IntentKind::LaunchPackage, 2, "/apps/calib.mod");
        assert_eq!(launch.kind, IntentKind::LaunchPackage);
        assert_eq!(launch.payload_str(), "/apps/calib.mod");
    }
}
