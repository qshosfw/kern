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
        let mut resp_uninit = ::core::mem::MaybeUninit::<[u32; 5]>::uninit();
        let resp_buf =
            unsafe { ::core::slice::from_raw_parts_mut(resp_uninit.as_mut_ptr() as *mut u8, 20) };
        let mut override_slice: Option<&[u8]> = None;
        let mut do_reset = false;
        let mut do_reboot = false;

        let (status, resp_len) = match header.opcode {
            OP_CORE_HELLO => {
                if payload.len() < 8 {
                    (WireStatus::BadLength, 0)
                } else {
                    let min_minor = payload[0];
                    let max_minor = payload[1];
                    let host_session =
                        u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
                    let req_payload = u16::from_le_bytes([payload[6], payload[7]]);

                    if min_minor > 1 || max_minor < 1 || host_session == 0 {
                        resp_buf[0] = WireStatus::BadVersion.to_u8();
                        resp_buf[1] = 1;
                        resp_buf[2] = 1;
                        do_reset = true;
                        (WireStatus::BadVersion, 3)
                    } else {
                        self.session.active_session_id = host_session;
                        self.session.expected_sequence = 1;
                        self.session.last_sequence = 0;
                        self.session.negotiated_max_payload = req_payload.min(128).max(32);

                        let max_p = self.session.negotiated_max_payload.to_le_bytes();
                        let tp = self.transport_profile.to_le_bytes();
                        resp_buf[0] = 1; // minor version = 1
                        resp_buf[1] = max_p[0];
                        resp_buf[2] = max_p[1];
                        resp_buf[3] = 1; // catalogue generation = 1
                        resp_buf[4] = 0;
                        resp_buf[5] = 0;
                        resp_buf[6] = 0;
                        resp_buf[7] = tp[0];
                        resp_buf[8] = tp[1];
                        resp_buf[9] = 1; // UV-K1 (0x0001_0001)
                        resp_buf[10] = 0;
                        resp_buf[11] = 1;
                        resp_buf[12] = 0;
                        resp_buf[13] = 0; // KERN 1.0 (0x0001_0000)
                        resp_buf[14] = 0;
                        resp_buf[15] = 1;
                        resp_buf[16] = 0;
                        (WireStatus::Ok, 17)
                    }
                }
            }

            OP_CORE_PING => {
                if payload.len() < 4 {
                    return;
                }
                resp_buf[0..4].copy_from_slice(&payload[0..4]);
                resp_buf[4..8].copy_from_slice(&uptime_ms.to_le_bytes());
                (WireStatus::Ok, 8)
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
                override_slice = Some(&CAPS_REPLY);
                (WireStatus::Ok, 35)
            }

            OP_CORE_GET_STATUS => {
                resp_buf[0] = 0;
                resp_buf[1] = 0;
                resp_buf[2] = 0xD0;
                resp_buf[3] = 0x20; // nominal=8400mV
                resp_buf[4..8].copy_from_slice(&uptime_ms.to_le_bytes());
                (WireStatus::Ok, 8)
            }

            OP_CORE_GET_RESULT => {
                resp_buf[0] = WireStatus::Ok.to_u8();
                resp_buf[1..5].copy_from_slice(&header.sequence.to_le_bytes());
                (WireStatus::Ok, 5)
            }

            OP_CORE_REBOOT => {
                do_reboot = true;
                (WireStatus::Ok, 0)
            }

            OP_CORE_TIME_SYNC => {
                let host_tx_us = if payload.len() >= 4 {
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]])
                } else {
                    0
                };
                let dev_rx_us = py32_hal::time::now_us();
                let dev_tx_us = py32_hal::time::now_us();

                resp_buf[0..4].copy_from_slice(&host_tx_us.to_le_bytes());
                resp_buf[4..8].copy_from_slice(&dev_rx_us.to_le_bytes());
                resp_buf[8..12].copy_from_slice(&dev_tx_us.to_le_bytes());
                resp_buf[12..16].copy_from_slice(&uptime_ms.to_le_bytes());
                (WireStatus::Ok, 16)
            }

            OP_CORE_GET_TIME => {
                let now_us = py32_hal::time::now_us();
                resp_buf[0..4].copy_from_slice(&now_us.to_le_bytes());
                resp_buf[4..8].copy_from_slice(&uptime_ms.to_le_bytes());
                (WireStatus::Ok, 8)
            }

            _ => (WireStatus::Unsupported, 0),
        };

        let resp_slice = override_slice.unwrap_or(&resp_buf[..resp_len]);
        let _ = self.send_response(header, status, resp_slice);

        if do_reset {
            self.session.reset();
        }
        if do_reboot {
            cortex_m::peripheral::SCB::sys_reset();
        }
    }
}
