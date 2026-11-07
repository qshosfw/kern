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

                let mut reply = [
                    1, // minor version = 1
                    0, 0, // negotiated max payload (filled below)
                    1, 0, 0, 0, // catalogue generation = 1
                    0, 0, // transport profile (filled below)
                    1, 0, 1, 0, // hardware profile = UV-K1 (0x0001_0001)
                    0, 0, 1, 0, // firmware profile = KERN 1.0 (0x0001_0000)
                ];
                reply[1..3].copy_from_slice(&self.session.negotiated_max_payload.to_le_bytes());
                reply[7..9].copy_from_slice(&self.transport_profile.to_le_bytes());

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

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_GET_CAPS,
                    header.sequence,
                    WireStatus::Ok,
                    &CAPS_REPLY,
                );
            }

            OP_CORE_GET_STATUS => {
                let mut resp = [0, 0, 0xD0, 0x20, 0, 0, 0, 0]; // state=0, flags=0, nominal=8400mV
                resp[4..8].copy_from_slice(&uptime_ms.to_le_bytes());

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_GET_STATUS,
                    header.sequence,
                    WireStatus::Ok,
                    &resp,
                );
            }

            OP_CORE_GET_RESULT => {
                let mut resp = [0u8; 5];
                resp[0] = WireStatus::Ok.to_u8();
                resp[1..5].copy_from_slice(&header.sequence.to_le_bytes());

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_GET_RESULT,
                    header.sequence,
                    WireStatus::Ok,
                    &resp,
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

            OP_CORE_TIME_SYNC => {
                let host_tx_us = if payload.len() >= 4 {
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]])
                } else {
                    0
                };
                let dev_rx_us = py32_hal::time::now_us();
                let dev_tx_us = py32_hal::time::now_us();

                let mut resp = [0u8; 16];
                resp[0..4].copy_from_slice(&host_tx_us.to_le_bytes());
                resp[4..8].copy_from_slice(&dev_rx_us.to_le_bytes());
                resp[8..12].copy_from_slice(&dev_tx_us.to_le_bytes());
                resp[12..16].copy_from_slice(&uptime_ms.to_le_bytes());

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_TIME_SYNC,
                    header.sequence,
                    WireStatus::Ok,
                    &resp,
                );
            }

            OP_CORE_GET_TIME => {
                let now_us = py32_hal::time::now_us();
                let mut resp = [0u8; 8];
                resp[0..4].copy_from_slice(&now_us.to_le_bytes());
                resp[4..8].copy_from_slice(&uptime_ms.to_le_bytes());

                let _ = self.send_response(
                    SERVICE_CORE,
                    OP_CORE_GET_TIME,
                    header.sequence,
                    WireStatus::Ok,
                    &resp,
                );
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
