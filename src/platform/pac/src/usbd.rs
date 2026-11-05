//! USBD (USB 2.0 Full-Speed Device) peripheral registers for PY32F071.
//!
//! Address mapping:
//! - USBD Registers: `0x4000_5C00` (APB1)
//! - USBD SRAM/FIFO: `0x4000_6000` (APB1)
//!
//! Pin mapping:
//! - PA11: USB_DM (D-)
//! - PA12: USB_DP (D+)

use crate::APB1_BASE;

pub const USBD_BASE: u32 = APB1_BASE + 0x5C00;
pub const USBD_SRAM_BASE: u32 = APB1_BASE + 0x6000;

// ── Register Offsets ────────────────────────────────────────────────────────

pub const USBD_CR_OFFSET: u32 = 0x00;
pub const USBD_INTR_OFFSET: u32 = 0x04;
pub const USBD_INTRE_OFFSET: u32 = 0x08;
pub const USBD_FRAME_OFFSET: u32 = 0x0C;
pub const USBD_EP0CSR_OFFSET: u32 = 0x10;
pub const USBD_INEPXCSR_OFFSET: u32 = 0x14;
pub const USBD_OUTEPXCSR_OFFSET: u32 = 0x18;
pub const USBD_OUTCOUNT_OFFSET: u32 = 0x1C;
pub const USBD_FIFODATA_OFFSET: u32 = 0x20;

// ── Register Addresses ──────────────────────────────────────────────────────

pub const USBD_CR: *mut u32 = USBD_BASE as *mut u32;
pub const USBD_INTR: *mut u32 = (USBD_BASE + USBD_INTR_OFFSET) as *mut u32;
pub const USBD_INTRE: *mut u32 = (USBD_BASE + USBD_INTRE_OFFSET) as *mut u32;
pub const USBD_FRAME: *const u32 = (USBD_BASE + USBD_FRAME_OFFSET) as *const u32;
pub const USBD_EP0CSR: *mut u32 = (USBD_BASE + USBD_EP0CSR_OFFSET) as *mut u32;
pub const USBD_INEPXCSR: *mut u32 = (USBD_BASE + USBD_INEPXCSR_OFFSET) as *mut u32;
pub const USBD_OUTEPXCSR: *mut u32 = (USBD_BASE + USBD_OUTEPXCSR_OFFSET) as *mut u32;
pub const USBD_OUTCOUNT: *const u32 = (USBD_BASE + USBD_OUTCOUNT_OFFSET) as *const u32;

// ── Byte-level and 16-bit MUSB register access ──────────────────────────────
pub const USBD_ADDR_U8: *mut u8 = USBD_BASE as *mut u8;
pub const USBD_POWER_U8: *mut u8 = (USBD_BASE + 0x01) as *mut u8;
pub const USBD_INT_USB_U8: *mut u8 = (USBD_BASE + 0x04) as *mut u8;
pub const USBD_INT_OUT1_U8: *const u8 = (USBD_BASE + 0x05) as *const u8;
pub const USBD_INT_IN1_U8: *const u8 = (USBD_BASE + 0x06) as *mut u8;
pub const USBD_INT_USBE_U8: *mut u8 = (USBD_BASE + 0x08) as *mut u8;
pub const USBD_INT_OUT1E_U8: *mut u8 = (USBD_BASE + 0x09) as *mut u8;
pub const USBD_INT_IN1E_U8: *mut u8 = (USBD_BASE + 0x0A) as *mut u8;
pub const USBD_FRAME_U16: *const u16 = (USBD_BASE + 0x0C) as *const u16;
pub const USBD_INDEX_U8: *mut u8 = (USBD_BASE + 0x0E) as *mut u8;

pub const USBD_EP0_CSR_U8: *mut u8 = (USBD_BASE + 0x10) as *mut u8;
pub const USBD_EP0_COUNT_U8: *const u8 = (USBD_BASE + 0x11) as *const u8;
pub const USBD_IN_CSR2_U8: *mut u8 = (USBD_BASE + 0x14) as *mut u8;
pub const USBD_IN_CSR1_U8: *mut u8 = (USBD_BASE + 0x15) as *mut u8;
pub const USBD_MAX_PKT_IN_U8: *mut u8 = (USBD_BASE + 0x16) as *mut u8;
pub const USBD_OUT_CSR2_U8: *mut u8 = (USBD_BASE + 0x18) as *mut u8;
pub const USBD_OUT_CSR1_U8: *mut u8 = (USBD_BASE + 0x19) as *mut u8;
pub const USBD_MAX_PKT_OUT_U8: *mut u8 = (USBD_BASE + 0x1A) as *mut u8;
pub const USBD_OUT_COUNT_U16: *const u16 = (USBD_BASE + 0x1C) as *const u16;

// ── CR Bit Definitions ─────────────────────────────────────────────────────

pub const CR_ADD_MASK: u32 = 0x7F;
pub const CR_UPDATE: u32 = 1 << 7;
pub const CR_ENABLE_SUSPEND: u32 = 1 << 8;
pub const CR_SUSPEND_MODE: u32 = 1 << 9;
pub const CR_RESUME: u32 = 1 << 10;
pub const CR_RESET: u32 = 1 << 11;
pub const CR_ISO_UPDATE: u32 = 1 << 15;

// ── MUSB INT_USB (0x04) Bit Definitions ─────────────────────────────────────
pub const USB_INTR_SUSPEND: u8 = 0x01;
pub const USB_INTR_RESUME: u8 = 0x02;
pub const USB_INTR_RESET: u8 = 0x04;
pub const USB_INTR_SOF: u8 = 0x08;

// ── MUSB POWER (0x01) Bit Definitions ───────────────────────────────────────
pub const USB_POWER_SUSPENDENB: u8 = 0x01;
pub const USB_POWER_SUSPENDMODE: u8 = 0x02;
pub const USB_POWER_RESUME: u8 = 0x04;
pub const USB_POWER_RESET: u8 = 0x08;
pub const USB_POWER_HSMODE: u8 = 0x10;
pub const USB_POWER_HSENAB: u8 = 0x20;
pub const USB_POWER_SOFTCONN: u8 = 0x40;
pub const USB_POWER_ISOUPDATE: u8 = 0x80;

// ── EP0 CSR (0x10) Bit Definitions ──────────────────────────────────────────
pub const EP0_RXPKTRDY: u8 = 0x01;
pub const EP0_TXPKTRDY: u8 = 0x02;
pub const EP0_SENTSTALL: u8 = 0x04;
pub const EP0_DATAEND: u8 = 0x08;
pub const EP0_SETUPEND: u8 = 0x10;
pub const EP0_SENDSTALL: u8 = 0x20;
pub const EP0_SVRXPKTRDY: u8 = 0x40;
pub const EP0_SVSETUPEND: u8 = 0x80;

// ── IN CSR1 / CSR2 (0x15 / 0x14) Bit Definitions ───────────────────────────
pub const INCSR1_INPKTRDY: u8 = 0x01;
pub const INCSR1_FIFONOTEMPTY: u8 = 0x02;
pub const INCSR1_FLUSHFIFO: u8 = 0x08;
pub const INCSR1_SENDSTALL: u8 = 0x10;
pub const INCSR1_SENTSTALL: u8 = 0x20;
pub const INCSR1_CLRDT: u8 = 0x40;

pub const INCSR2_DMAENAB: u8 = 0x10;
pub const INCSR2_MODE: u8 = 0x20;
pub const INCSR2_ISO: u8 = 0x40;
pub const INCSR2_AUTOSET: u8 = 0x80;

// ── OUT CSR1 / CSR2 (0x19 / 0x18) Bit Definitions ──────────────────────────
pub const OUTCSR1_OUTPKTRDY: u8 = 0x01;
pub const OUTCSR1_FIFOFULL: u8 = 0x02;
pub const OUTCSR1_OVERRUN: u8 = 0x04;
pub const OUTCSR1_DATAPACKETERROR: u8 = 0x08;
pub const OUTCSR1_FLUSHFIFO: u8 = 0x10;
pub const OUTCSR1_SENDSTALL: u8 = 0x20;
pub const OUTCSR1_SENTSTALL: u8 = 0x40;
pub const OUTCSR1_CLRDT: u8 = 0x80;

pub const OUTCSR2_DMAMODE: u8 = 0x10;
pub const OUTCSR2_DMAENAB: u8 = 0x20;
pub const OUTCSR2_ISO: u8 = 0x40;
pub const OUTCSR2_AUTOCLR: u8 = 0x80;

// ── INTR / INTRE Bit Definitions ───────────────────────────────────────────

pub const INTR_SUSPEND: u32 = 1 << 0;
pub const INTR_RESUME: u32 = 1 << 1;
pub const INTR_RESET: u32 = 1 << 2;
pub const INTR_SOF: u32 = 1 << 3;
pub const INTR_EP0: u32 = 1 << 4;
pub const INTR_INEP1: u32 = 1 << 5;
pub const INTR_INEP2: u32 = 1 << 6;
pub const INTR_INEP3: u32 = 1 << 7;
pub const INTR_INEP4: u32 = 1 << 8;
pub const INTR_OUTEP1: u32 = 1 << 9;
pub const INTR_OUTEP2: u32 = 1 << 10;
pub const INTR_OUTEP3: u32 = 1 << 11;
pub const INTR_OUTEP4: u32 = 1 << 12;

// ── EP0CSR Bit Definitions ─────────────────────────────────────────────────

pub const EP0CSR_OUTPKT_RDY: u32 = 1 << 0;
pub const EP0CSR_INPKT_RDY: u32 = 1 << 1;
pub const EP0CSR_SENT_STALL: u32 = 1 << 2;
pub const EP0CSR_DATA_END: u32 = 1 << 3;
pub const EP0CSR_SETUP_END: u32 = 1 << 4;
pub const EP0CSR_SEND_STALL: u32 = 1 << 5;
pub const EP0CSR_SV_OUTPKT_RDY: u32 = 1 << 6;
pub const EP0CSR_SV_SETUP_END: u32 = 1 << 7;

/// Access 8-bit endpoint FIFO data register for endpoint `ep` (0-7).
#[inline(always)]
pub fn fifo_ep_u8(ep: u8) -> *mut u8 {
    (USBD_BASE + 0x20 + ((ep as u32 & 0x07) * 4)) as *mut u8
}

/// Access endpoint FIFO data register for endpoint `ep` (0-15).
#[inline(always)]
pub fn ep_fifo_data(ep: u8) -> *mut u32 {
    (USBD_BASE + USBD_FIFODATA_OFFSET + ((ep as u32 & 0x0F) * 4)) as *mut u32
}

/// Select active endpoint index (0..7) in MUSB INDEX register.
#[inline(always)]
pub fn set_active_ep(ep: u8) {
    unsafe {
        USBD_INDEX_U8.write_volatile(ep & 0x07);
    }
}

/// Read active endpoint index from MUSB INDEX register.
#[inline(always)]
pub fn get_active_ep() -> u8 {
    unsafe { USBD_INDEX_U8.read_volatile() & 0x07 }
}

/// Write a byte buffer into the selected endpoint FIFO.
pub fn write_fifo(ep: u8, data: &[u8]) {
    let fifo = fifo_ep_u8(ep);
    for &b in data {
        unsafe {
            fifo.write_volatile(b);
        }
    }
}

/// Read up to `buf.len()` bytes from the selected endpoint FIFO.
pub fn read_fifo(ep: u8, buf: &mut [u8]) -> usize {
    let fifo = fifo_ep_u8(ep);
    for b in buf.iter_mut() {
        *b = unsafe { fifo.read_volatile() };
    }
    buf.len()
}
