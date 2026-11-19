//! Core Protocol Operations (HELLO, PING, GET_CAPS, REBOOT).

use super::WireDriver;
use kern_platform_traits::time::TimeSource;
use kernel::service::SerialPort;
use kernel::wire::*;

const CAPS_REPLY: [u8; 35] = [
    1,
    0,
    0,
    0, // generation = 1
    0xFF,
    0xFF, // next cursor = 0xFFFF (end)
    7,    // 7 components
    1,
    2,
    1,
    COMPONENT_CLASS_RADIO,
    1,
    2,
    2,
    COMPONENT_CLASS_MCU,
    1,
    2,
    3,
    COMPONENT_CLASS_DISPLAY,
    1,
    2,
    4,
    COMPONENT_CLASS_KEYPAD,
    1,
    2,
    5,
    COMPONENT_CLASS_AUDIO,
    1,
    2,
    6,
    COMPONENT_CLASS_POWER,
    1,
    2,
    7,
    COMPONENT_CLASS_STORAGE,
];

impl<S: SerialPort, T: TimeSource> WireDriver<S, T> {
    #[inline(never)]
    pub(super) fn handle_core_operation(
        &mut self,
        header: &WireHeader,
        raw_len: usize,
        uptime_ms: u32,
    ) {
        let covered = HEADER_LEN + header.payload_len as usize;
        if covered > raw_len {
            return;
        }
        let payload = &self.rx_buf[HEADER_LEN..covered];
        let mut words = [0u32; 5];
        let mut override_slice: Option<&[u8]> = None;
        let mut post_op = 0u8; // 1 = reset session, 2 = reboot

        let (status, resp_len) = match header.opcode {
            OP_CORE_HELLO => {
                if payload.len() < 8 {
                    (WireStatus::BadLength, 0)
                } else {
                    let host_session =
                        u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
                    let req_payload = u16::from_le_bytes([payload[6], payload[7]]);

                    if payload[0] > 1 || payload[1] < 1 || host_session == 0 {
                        words[0] = (WireStatus::BadVersion.to_u8() as u32) | (1 << 8) | (1 << 16);
                        post_op = 1;
                        (WireStatus::BadVersion, 3)
                    } else {
                        self.session
                            .begin_session(host_session, req_payload.clamp(32, 128));
                        let max_p = self.session.negotiated_max_payload as u32;
                        let tp = self.transport_profile as u32;
                        words[0] = 1 | (max_p << 8) | (1 << 24);
                        words[1] = tp << 24;
                        words[2] = (tp >> 8) | (1 << 8) | (1 << 24);
                        words[3] = 1 << 24;
                        (WireStatus::Ok, 17)
                    }
                }
            }

            OP_CORE_PING => {
                if payload.len() < 4 {
                    return;
                }
                words[0] = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                words[1] = uptime_ms;
                (WireStatus::Ok, 8)
            }

            OP_CORE_GET_CAPS => {
                override_slice = Some(&CAPS_REPLY);
                (WireStatus::Ok, CAPS_REPLY.len())
            }

            OP_CORE_GET_STATUS => {
                words[0] = 8400u32 << 16; // nominal = 8400 mV
                words[1] = uptime_ms;
                (WireStatus::Ok, 8)
            }

            OP_CORE_GET_RESULT => {
                words[0] = (WireStatus::Ok.to_u8() as u32) | (header.sequence << 8);
                words[1] = header.sequence >> 24;
                (WireStatus::Ok, 5)
            }

            OP_CORE_REBOOT => {
                post_op = 2;
                (WireStatus::Ok, 0)
            }

            OP_CORE_TIME_SYNC => {
                words[0] = if payload.len() >= 4 {
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]])
                } else {
                    0
                };
                words[1] = self.time.now_us();
                words[2] = self.time.now_us();
                words[3] = uptime_ms;
                (WireStatus::Ok, 16)
            }

            OP_CORE_GET_TIME => {
                words[0] = self.time.now_us();
                words[1] = uptime_ms;
                (WireStatus::Ok, 8)
            }

            _ => (WireStatus::Unsupported, 0),
        };

        let resp_bytes = unsafe { core::slice::from_raw_parts(words.as_ptr() as *const u8, 20) };
        let resp_slice = override_slice.unwrap_or(&resp_bytes[..resp_len]);
        let _ = self.send_response(header, status, resp_slice);

        if post_op == 1 {
            self.session.reset();
        } else if post_op == 2 {
            kernel::power::PowerControl::reboot();
        }
    }
}
