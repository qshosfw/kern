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

// ── Structured Zero-Heap Microkernel IPC (§8.4 / KRX 3.2 / ORDS-01) ─────────

pub mod ipc_flags {
    pub const NONE: u8 = 0;
    pub const REPLY: u8 = 1 << 0;
    pub const URGENT: u8 = 1 << 1;
    pub const NEED_ACK: u8 = 1 << 2;
    pub const ERROR: u8 = 1 << 3;
}

pub mod ipc_target {
    pub const FOREGROUND_APP: u8 = 0x00;
    pub const KERNEL: u8 = 0x01;
    pub const BROADCAST: u8 = 0xFF;
}

pub mod ipc_msg_type {
    pub const RAW_DATA: u16 = 0x0000;
    pub const PING: u16 = 0x0001;
    pub const PONG: u16 = 0x0002;
    pub const DAEMON_START: u16 = 0x0010;
    pub const DAEMON_STOP: u16 = 0x0011;
    pub const DAEMON_STATUS: u16 = 0x0012;
    pub const SERVICE_REQUEST: u16 = 0x0020;
    pub const SERVICE_RESPONSE: u16 = 0x0021;
    pub const APP_SWITCH: u16 = 0x0030;
    pub const APP_DATA_TRANSFER: u16 = 0x0031;
    pub const TELEMETRY_FRAME: u16 = 0x0040;
}

/// Normative 32-byte Zero-Heap Inter-Process Communication Message (§8.4 / KRX 3.2).
/// Follows ORDS-01 $O(1)$ power-of-two geometry ($2^5 = 32$ bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxIpcMessage {
    /// Originating process PID (0 = Kernel/Supervisor)
    pub sender_pid: u8,
    /// Destination process PID (0 = Active Foreground App, 0xFF = Broadcast to all actors)
    pub target_pid: u8,
    /// Application/Service message opcode or command identifier
    pub msg_type: u16,
    /// Number of active bytes in `payload` (0..=24)
    pub payload_len: u8,
    /// IPC status and transaction flags (e.g. `ipc_flags::REPLY`, `ipc_flags::URGENT`)
    pub flags: u8,
    /// Correlation sequence number for matching request-reply transactions
    pub seq: u16,
    /// Bounded message payload (24 bytes)
    pub payload: [u8; 24],
}

const _: () = assert!(core::mem::size_of::<KrxIpcMessage>() == 32);

impl KrxIpcMessage {
    pub const fn empty() -> Self {
        Self {
            sender_pid: 0,
            target_pid: 0,
            msg_type: 0,
            payload_len: 0,
            flags: 0,
            seq: 0,
            payload: [0u8; 24],
        }
    }

    pub fn new(sender_pid: u8, target_pid: u8, msg_type: u16, payload_data: &[u8]) -> Self {
        let mut msg = Self::empty();
        msg.sender_pid = sender_pid;
        msg.target_pid = target_pid;
        msg.msg_type = msg_type;
        let len = payload_data.len().min(24);
        msg.payload[..len].copy_from_slice(&payload_data[..len]);
        msg.payload_len = len as u8;
        msg
    }

    pub fn reply_to(&self, payload_data: &[u8], is_error: bool) -> Self {
        let mut rep = Self::empty();
        rep.sender_pid = self.target_pid;
        rep.target_pid = self.sender_pid;
        rep.msg_type = self.msg_type;
        rep.seq = self.seq;
        rep.flags = ipc_flags::REPLY | if is_error { ipc_flags::ERROR } else { 0 };
        let len = payload_data.len().min(24);
        rep.payload[..len].copy_from_slice(&payload_data[..len]);
        rep.payload_len = len as u8;
        rep
    }

    #[inline(always)]
    pub const fn is_reply(&self) -> bool {
        (self.flags & ipc_flags::REPLY) != 0
    }

    #[inline(always)]
    pub const fn is_error(&self) -> bool {
        (self.flags & ipc_flags::ERROR) != 0
    }

    #[inline(always)]
    pub const fn is_broadcast(&self) -> bool {
        self.target_pid == ipc_target::BROADCAST
    }
}

/// Zero-heap multi-process IPC Channel and Message Router (§8.4 / KRX 3.2).
/// Manages up to N concurrent 32-byte messages in strict FIFO order.
pub struct IpcChannel<const N: usize = 8> {
    messages: [KrxIpcMessage; N],
    len: usize,
    next_seq: u16,
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
            messages: [const { KrxIpcMessage::empty() }; N],
            len: 0,
            next_seq: 0,
        }
    }

    /// Enqueue a message into the IPC channel.
    /// If seq is 0, auto-assigns the next monotonic sequence number.
    pub fn send(&mut self, mut msg: KrxIpcMessage) -> bool {
        if self.len >= N {
            return false;
        }
        if msg.seq == 0 {
            self.next_seq = self.next_seq.wrapping_add(1).max(1);
            msg.seq = self.next_seq;
        }
        self.messages[self.len] = msg;
        self.len += 1;
        true
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
}
