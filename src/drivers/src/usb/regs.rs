//! Portable MUSB / Full-Speed USB Device Controller register bit definitions.

pub const USB_INTR_SUSPEND: u8 = 0x01;
pub const USB_INTR_RESUME: u8 = 0x02;
pub const USB_INTR_RESET: u8 = 0x04;
pub const USB_INTR_SOF: u8 = 0x08;

pub const USB_POWER_SUSPENDENB: u8 = 0x01;
pub const USB_POWER_SUSPENDMODE: u8 = 0x02;
pub const USB_POWER_RESUME: u8 = 0x04;
pub const USB_POWER_RESET: u8 = 0x08;
pub const USB_POWER_HSMODE: u8 = 0x10;
pub const USB_POWER_HSENAB: u8 = 0x20;
pub const USB_POWER_SOFTCONN: u8 = 0x40;
pub const USB_POWER_ISOUPDATE: u8 = 0x80;

pub const EP0_RXPKTRDY: u8 = 0x01;
pub const EP0_TXPKTRDY: u8 = 0x02;
pub const EP0_SENTSTALL: u8 = 0x04;
pub const EP0_DATAEND: u8 = 0x08;
pub const EP0_SETUPEND: u8 = 0x10;
pub const EP0_SENDSTALL: u8 = 0x20;
pub const EP0_SVRXPKTRDY: u8 = 0x40;
pub const EP0_SVSETUPEND: u8 = 0x80;

pub const INCSR1_INPKTRDY: u8 = 0x01;
pub const INCSR1_FIFONOTEMPTY: u8 = 0x02;
pub const INCSR1_UNDERRUN: u8 = 0x04;
pub const INCSR1_FLUSHFIFO: u8 = 0x08;
pub const INCSR1_SENDSTALL: u8 = 0x10;
pub const INCSR1_SENTSTALL: u8 = 0x20;
pub const INCSR1_CLRDT: u8 = 0x40;

pub const INCSR2_DMAENAB: u8 = 0x10;
pub const INCSR2_MODE: u8 = 0x20;
pub const INCSR2_ISO: u8 = 0x40;
pub const INCSR2_AUTOSET: u8 = 0x80;

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
