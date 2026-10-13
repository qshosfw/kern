//! Modular UART Driver implementation backing kernel UartService

use bsp::Board;

pub struct HardwareUart;

impl HardwareUart {
    pub fn new() -> Self {
        Self
    }
}

impl HardwareUart {
    pub fn write(&mut self, data: &[u8]) {
        Board::uart_write_bytes(data);
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        for byte in buf.iter_mut() {
            if let Some(b) = Board::uart_try_read_byte() {
                *byte = b;
                count += 1;
            } else {
                break;
            }
        }
        count
    }
}

impl kernel::service::UartService for HardwareUart {
    fn write(&mut self, data: &[u8]) {
        py32_hal::uart::mark_activity();
        self.write(data);
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        let count = self.read(buf);
        if count > 0 {
            py32_hal::uart::mark_activity();
        }
        count
    }
}
