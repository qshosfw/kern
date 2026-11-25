//! RigWire v2 Core Channel (`0x00`) Dispatcher: HELLO, PING, TIME_SYNC, RUNLEVEL, CAPS & TABLES.

use super::WireDriver;
use kern_platform_traits::time::TimeSource;
use kernel::service::SerialPort;
use kernel::wire::*;

/// Wire-ready `CORE::CAPS` reply: `[gen: u32, next_cursor: u16, count: u8, SemanticCap[7]]`.
///
/// Little-endian packed directly in `.rodata` (`7 + 7*16 = 119 B`, fits in one 128 B frame,
/// zero runtime serialization loops or stack buffers).
#[repr(C, packed)]
struct CapsReplyWire {
    generation: u32,
    next_cursor: u16,
    count: u8,
    caps: [SemanticCap; 7],
}

const CAPS_REPLY_WIRE: CapsReplyWire = CapsReplyWire {
    generation: 1,
    next_cursor: 0xFFFF,
    count: 7,
    caps: [
        SemanticCap::new(*b"TUNE", *b"BK48", 0, 1, 0x0001, 0x0000_000F),
        SemanticCap::new(*b"TXPA", *b"BK48", 0, 1, 0x0001, 0x0000_0007),
        SemanticCap::new(*b"AUDI", *b"PA00", 0, 1, 0x0004, 0x0000_0003),
        SemanticCap::new(*b"DISP", *b"ST75", 0, 1, 0x0005, 0x0000_0003),
        SemanticCap::new(*b"INPT", *b"KMAT", 0, 1, 0x0006, 0x0000_0001),
        SemanticCap::new(*b"POWR", *b"ADC0", 0, 1, 0x0007, 0x0000_0003),
        SemanticCap::new(*b"STOR", *b"KF21", 0, 1, 0x0009, 0x0000_0003),
    ],
};

/// Wire-ready `CORE::TABLES` reply: `[count: u8, TableDesc[6]]` (`1 + 6*12 = 73 B`).
#[repr(C, packed)]
struct TablesReplyWire {
    count: u8,
    tables: [TableDesc; 6],
}

const TABLES_REPLY_WIRE: TablesReplyWire = TablesReplyWire {
    count: 6,
    tables: [
        TableDesc::new(
            *b"CHAN",
            64,
            512,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"CONT",
            32,
            256,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"BNDP",
            32,
            64,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"ZONE",
            32,
            64,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"HITS",
            32,
            128,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
        TableDesc::new(
            *b"FMBC",
            16,
            64,
            1,
            TableDesc::FLAG_WRITABLE | TableDesc::FLAG_HASHABLE,
        ),
    ],
};

#[inline(always)]
const fn as_raw_bytes<T>(val: &'static T) -> &'static [u8] {
    unsafe {
        ::core::slice::from_raw_parts(val as *const T as *const u8, ::core::mem::size_of::<T>())
    }
}

#[inline(never)]
fn read_le_u32(buf: &[u8], off: usize) -> u32 {
    if off + 4 > buf.len() {
        return 0;
    }
    unsafe {
        let p = buf.as_ptr().add(off);
        u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)])
    }
}

impl<S: SerialPort, T: TimeSource> WireDriver<S, T> {
    #[inline(never)]
    pub(super) fn handle_core_operation(
        &mut self,
        header: &WireHeader,
        raw_len: usize,
        uptime_ms: u32,
    ) {
        let covered = HEADER_LEN + header.len as usize;
        if covered > raw_len {
            return;
        }
        let payload = &self.rx_buf[HEADER_LEN..covered];
        let mut words = [0u32; 5];
        let mut override_slice: Option<&[u8]> = None;
        let mut reboot_mode: u8 = 0xFF;

        let (status, resp_len) = match header.op {
            OP_CORE_HELLO => {
                if payload.len() < 8 {
                    (Status::BadLength, 0)
                } else {
                    let epoch = read_le_u32(payload, 2);
                    let req_mtu = u16::from_le_bytes([payload[6], payload[7]]);
                    if epoch == 0 {
                        (Status::InvalidArg, 0)
                    } else {
                        let mtu = req_mtu.clamp(32, 128);
                        self.session.begin_session(epoch, mtu);
                        let tp = self.transport_profile as u32;
                        words[0] = ((mtu as u32) << 8) | (1 << 24);
                        words[1] = tp << 24;
                        words[2] = (tp >> 8) | (1 << 8);
                        words[3] = 2 << 8;
                        (Status::Ok, 17)
                    }
                }
            }

            OP_CORE_PING => {
                if payload.len() < 4 {
                    (Status::BadLength, 0)
                } else {
                    words[0] = read_le_u32(payload, 0);
                    words[1] = uptime_ms;
                    (Status::Ok, 8)
                }
            }

            OP_CORE_TIME_SYNC => {
                let now = self.time.now_us();
                words[0] = read_le_u32(payload, 0);
                words[1] = now;
                words[2] = now;
                words[3] = uptime_ms;
                (Status::Ok, 16)
            }

            OP_CORE_GET_TIME => {
                words[0] = self.time.now_us();
                words[1] = uptime_ms;
                (Status::Ok, 8)
            }

            OP_CORE_RUNLEVEL if !self.session.is_active() => (Status::BadSession, 0),
            OP_CORE_RUNLEVEL => {
                reboot_mode = payload.first().copied().unwrap_or(0);
                (Status::Ok, 0)
            }

            OP_CORE_CAPS => {
                override_slice = Some(as_raw_bytes(&CAPS_REPLY_WIRE));
                (Status::Ok, 0)
            }

            OP_CORE_TABLES => {
                override_slice = Some(as_raw_bytes(&TABLES_REPLY_WIRE));
                (Status::Ok, 0)
            }

            _ => (Status::Unsupported, 0),
        };

        let resp_bytes = unsafe { ::core::slice::from_raw_parts(words.as_ptr() as *const u8, 20) };
        let resp_slice = override_slice.unwrap_or(&resp_bytes[..resp_len]);
        let _ = self.send_response(header, status, resp_slice);

        if reboot_mode != 0xFF {
            // Drain TX FIFO / USB endpoint before resetting so the host receives the ACK.
            self.port.flush();
            kernel::power::PowerControl::reboot_with(kernel::power::RebootMode::from_u8(
                reboot_mode,
            ));
        }
    }
}
