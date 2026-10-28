//! Core Protocol Operations (HELLO, PING, GET_CAPS, REBOOT).

use super::WireDriver;
use kernel::service::SerialPort;
use kernel::wire::*;

impl<S: SerialPort> WireDriver<S> {
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

        match header.opcode {
            OP_CORE_HELLO => {
                if payload.len() < 8 {
                    let _ = self.send_response(
                        SERVICE_CORE,
                        OP_CORE_HELLO,
                        header.sequence,
                        WireStatus::BadLength,
                        &[],
                    );
                    return;
                }

                let min_minor = payload[0];
                let max_minor = payload[1];
                let host_session =
                    u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
                let req_payload = u16::from_le_bytes([payload[6], payload[7]]);

                if min_minor > 1 || max_minor < 1 || host_session == 0 {
                    let err_payload = [WireStatus::BadVersion.to_u8(), 1, 1];
                    let _ = self.send_response(
                        SERVICE_CORE,
                        OP_CORE_HELLO,
                        header.sequence,
                        WireStatus::BadVersion,
                        &err_payload,
                    );
                    self.session.reset();
                    return;
                }

                self.session.active_session_id = host_session;
                self.session.expected_sequence = 1;
                self.session.last_sequence = 0;
                self.session.negotiated_max_payload = req_payload.min(128).max(32);

                let mut reply = [0u8; 17];
                reply[0] = 1; // minor version = 1
                reply[1..3].copy_from_slice(&self.session.negotiated_max_payload.to_le_bytes());
                reply[3..7].copy_from_slice(&1u32.to_le_bytes()); // catalogue generation = 1
                reply[7..9].copy_from_slice(&self.transport_profile.to_le_bytes());
                reply[9..13].copy_from_slice(&0x0001_0001u32.to_le_bytes()); // hardware profile = UV-K1
                reply[13..17].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // firmware profile = KERN 1.0

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_HELLO,
                    header.sequence,
                    WireStatus::Ok,
                    &reply,
                );
            }

            OP_CORE_PING => {
                if payload.len() < 4 {
                    return;
                }
                let host_cookie =
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);

                let mut ping_reply = [0u8; 8];
                ping_reply[0..4].copy_from_slice(&host_cookie.to_le_bytes());
                ping_reply[4..8].copy_from_slice(&uptime_ms.to_le_bytes());

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_PING,
                    header.sequence,
                    WireStatus::Ok,
                    &ping_reply,
                );
            }

            OP_CORE_GET_CAPS => {
                let mut caps_reply = [0u8; 35];
                caps_reply[0..4].copy_from_slice(&1u32.to_le_bytes()); // generation = 1
                caps_reply[4..6].copy_from_slice(&0xFFFFu16.to_le_bytes()); // next cursor = 0xFFFF (end)
                caps_reply[6] = 7; // 7 components

                let components = [
                    (1, COMPONENT_CLASS_RADIO),
                    (2, COMPONENT_CLASS_MCU),
                    (3, COMPONENT_CLASS_DISPLAY),
                    (4, COMPONENT_CLASS_KEYPAD),
                    (5, COMPONENT_CLASS_AUDIO),
                    (6, COMPONENT_CLASS_POWER),
                    (7, COMPONENT_CLASS_STORAGE),
                ];

                let mut off = 7;
                for (id, class) in components {
                    caps_reply[off] = 1; // type 1 = Component Descriptor
                    caps_reply[off + 1] = 2; // len = 2
                    caps_reply[off + 2] = id;
                    caps_reply[off + 3] = class;
                    off += 4;
                }

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_GET_CAPS,
                    header.sequence,
                    WireStatus::Ok,
                    &caps_reply,
                );
            }

            OP_CORE_REBOOT => {
                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_REBOOT,
                    header.sequence,
                    WireStatus::Ok,
                    &[],
                );
                cortex_m::peripheral::SCB::sys_reset();
            }

            _ => {
                let _ = self.send_response(
                    SERVICE_CORE,
                    header.opcode,
                    header.sequence,
                    WireStatus::Unsupported,
                    &[],
                );
            }
        }
    }
}
