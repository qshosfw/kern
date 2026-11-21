//! Modular UART Driver implementation backing kernel UartService

use kern_platform_traits::serial::SerialBackend;

#[derive(Clone, Copy, Default)]
pub struct HardwareUart<S> {
    backend: S,
}

impl<S: SerialBackend> HardwareUart<S> {
    pub const fn new(backend: S) -> Self {
        Self { backend }
    }

    pub fn write(&mut self, data: &[u8]) {
        self.backend.write_bytes(data);
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        for byte in buf.iter_mut() {
            if let Some(b) = self.backend.try_read_byte() {
                *byte = b;
                count += 1;
            } else {
                break;
            }
        }
        count
    }
}

impl<S: SerialBackend> kernel::service::SerialPort for HardwareUart<S> {
    fn write(&mut self, data: &[u8]) {
        self.write(data);
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        self.read(buf)
    }
}

impl<S: SerialBackend> kernel::service::UartService for HardwareUart<S> {}
