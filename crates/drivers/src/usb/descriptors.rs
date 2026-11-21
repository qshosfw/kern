//! USB 2.0 Standard and Class Descriptors for PY32F071.
//!
//! Provides compile-time generated, feature-gated descriptor sets for:
//! - Standalone USB CDC ACM (Virtual COM Port)
//! - Standalone USB MSC (Mass Storage Class / Disk)
//! - Composite USB Device (CDC ACM + Mass Storage Class)

// ── Standard Descriptor Types ───────────────────────────────────────────────
pub const DESC_TYPE_DEVICE: u8 = 1;
pub const DESC_TYPE_CONFIGURATION: u8 = 2;
pub const DESC_TYPE_STRING: u8 = 3;
pub const DESC_TYPE_INTERFACE: u8 = 4;
pub const DESC_TYPE_ENDPOINT: u8 = 5;
pub const DESC_TYPE_DEVICE_QUALIFIER: u8 = 6;
pub const DESC_TYPE_INTERFACE_ASSOCIATION: u8 = 11;
pub const DESC_TYPE_CS_INTERFACE: u8 = 0x24;

// ── Endpoint Definitions ───────────────────────────────────────────────────
pub const EP0_SIZE: u8 = 64;

// MSC Endpoints
pub const MSC_EP_IN: u8 = 0x81; // EP1 IN
pub const MSC_EP_OUT: u8 = 0x02; // EP2 OUT
pub const MSC_EP_SIZE: u8 = 64;

// CDC Endpoints
pub const CDC_EP_NOTIF: u8 = 0x83; // EP3 IN Interrupt
pub const CDC_NOTIF_SIZE: u8 = 16;

#[cfg(all(feature = "usb_cdc", not(feature = "usb_msc")))]
pub const CDC_EP_IN: u8 = 0x81; // EP1 IN Bulk (DeltaFW / F4HWN standard)
#[cfg(all(feature = "usb_cdc", not(feature = "usb_msc")))]
pub const CDC_EP_OUT: u8 = 0x02; // EP2 OUT Bulk

#[cfg(all(feature = "usb_cdc", feature = "usb_msc"))]
pub const CDC_EP_IN: u8 = 0x84; // EP4 IN Bulk
#[cfg(all(feature = "usb_cdc", feature = "usb_msc"))]
pub const CDC_EP_OUT: u8 = 0x04; // EP4 OUT Bulk

#[cfg(not(feature = "usb_cdc"))]
pub const CDC_EP_IN: u8 = 0x84;
#[cfg(not(feature = "usb_cdc"))]
pub const CDC_EP_OUT: u8 = 0x04;

pub const CDC_DATA_SIZE: u8 = 64;

// ── Standard Device Descriptor ──────────────────────────────────────────────
pub static DEVICE_DESCRIPTOR: [u8; 18] = [
    18,               // bLength
    DESC_TYPE_DEVICE, // bDescriptorType
    0x00,
    0x02, // bcdUSB: USB 2.0
    #[cfg(feature = "usb_cdc")]
    0xEF, // bDeviceClass: Miscellaneous (IAD) - standard for modern CDC & Composite
    #[cfg(not(feature = "usb_cdc"))]
    0x00, // bDeviceClass: Defined at interface level
    0x02, // bDeviceSubClass: Common Class
    0x01, // bDeviceProtocol: Interface Association
    EP0_SIZE, // bMaxPacketSize0 (64 bytes)
    0xB7,
    0x36, // idVendor (0x36B7 - Puya / Kern)
    0xFF,
    0xFF, // idProduct (0xFFFF - matches DeltaFW & standard Puya CDC tools)
    0x00,
    0x01, // bcdDevice (1.00)
    1,    // iManufacturer (String index 1)
    2,    // iProduct (String index 2)
    3,    // iSerialNumber (String index 3)
    1,    // bNumConfigurations
];

// ── Configuration Descriptor ────────────────────────────────────────────────
#[cfg(all(feature = "usb_cdc", feature = "usb_msc"))]
pub static CONFIG_DESCRIPTOR: [u8; 9 + 8 + 9 + 5 + 5 + 4 + 5 + 7 + 9 + 7 + 7 + 9 + 7 + 7] = [
    // 1. Configuration Header (9 bytes)
    9,
    DESC_TYPE_CONFIGURATION,
    (9 + 8 + 9 + 5 + 5 + 4 + 5 + 7 + 9 + 7 + 7 + 9 + 7 + 7) as u8,
    0,    // wTotalLength (98 bytes)
    3,    // bNumInterfaces: CDC Comm (0), CDC Data (1), MSC (2)
    1,    // bConfigurationValue
    5,    // iConfiguration: String 5 (Live Power Telemetry)
    0xC0, // bmAttributes: Self-Powered (Internal battery)
    50,   // bMaxPower: 100mA (50 * 2mA)
    // 2. Interface Association Descriptor (IAD) for CDC (8 bytes)
    8,
    DESC_TYPE_INTERFACE_ASSOCIATION,
    0,    // bFirstInterface (Interface 0)
    2,    // bInterfaceCount (Interfaces 0 and 1)
    0x02, // bFunctionClass (CDC)
    0x02, // bFunctionSubClass (Abstract Control Model)
    0x01, // bFunctionProtocol (AT Commands / V.25ter)
    4,    // iFunction: String 4 ("Serial")
    // 3. CDC Communication Interface (Interface 0, 9 bytes)
    9,
    DESC_TYPE_INTERFACE,
    0,    // bInterfaceNumber: 0
    0,    // bAlternateSetting: 0
    1,    // bNumEndpoints: 1 (Notification EP)
    0x02, // bInterfaceClass: CDC
    0x02, // bInterfaceSubClass: ACM
    0x01, // bInterfaceProtocol: AT Commands
    4,    // iInterface: String 4 ("Serial")
    // 4. CDC Header Functional Descriptor (5 bytes)
    5,
    DESC_TYPE_CS_INTERFACE,
    0x00,
    0x10,
    0x01, // CDC 1.10
    // 5. CDC Call Management Functional Descriptor (5 bytes)
    5,
    DESC_TYPE_CS_INTERFACE,
    0x01,
    0x00,
    1, // Data interface is 1
    // 6. CDC ACM Functional Descriptor (4 bytes)
    4,
    DESC_TYPE_CS_INTERFACE,
    0x02,
    0x02, // Support Line Coding & Serial State
    // 7. CDC Union Functional Descriptor (5 bytes)
    5,
    DESC_TYPE_CS_INTERFACE,
    0x06,
    0,
    1, // Master: 0, Slave: 1
    // 8. CDC Notification Endpoint (EP3 IN Interrupt, 7 bytes)
    7,
    DESC_TYPE_ENDPOINT,
    CDC_EP_NOTIF, // bEndpointAddress (0x83)
    0x03,         // bmAttributes: Interrupt
    CDC_NOTIF_SIZE,
    0,  // wMaxPacketSize (16 bytes)
    10, // bInterval (10 ms)
    // 9. CDC Data Interface (Interface 1, 9 bytes)
    9,
    DESC_TYPE_INTERFACE,
    1,    // bInterfaceNumber: 1
    0,    // bAlternateSetting: 0
    2,    // bNumEndpoints: 2 (Bulk IN, Bulk OUT)
    0x0A, // bInterfaceClass: CDC Data
    0x00, // bInterfaceSubClass
    0x00, // bInterfaceProtocol
    6,    // iInterface: String 6 (Hardware MAC e.g. "MAC: 02:4A:...")
    // 10. CDC Data OUT Endpoint (EP4 OUT Bulk, 7 bytes)
    7,
    DESC_TYPE_ENDPOINT,
    CDC_EP_OUT, // bEndpointAddress (0x04)
    0x02,       // bmAttributes: Bulk
    CDC_DATA_SIZE,
    0, // wMaxPacketSize (64 bytes)
    0, // bInterval
    // 11. CDC Data IN Endpoint (EP4 IN Bulk, 7 bytes)
    7,
    DESC_TYPE_ENDPOINT,
    CDC_EP_IN, // bEndpointAddress (0x84)
    0x02,      // bmAttributes: Bulk
    CDC_DATA_SIZE,
    0, // wMaxPacketSize (64 bytes)
    0, // bInterval
    // 12. Mass Storage Class Interface (Interface 2, 9 bytes)
    9,
    DESC_TYPE_INTERFACE,
    2,    // bInterfaceNumber: 2
    0,    // bAlternateSetting: 0
    2,    // bNumEndpoints: 2 (Bulk IN, Bulk OUT)
    0x08, // bInterfaceClass: Mass Storage (MSC)
    0x06, // bInterfaceSubClass: SCSI transparent command set
    0x50, // bInterfaceProtocol: Bulk-Only Transport (BOT)
    7,    // iInterface: String 7 ("Storage")
    // 13. MSC Bulk OUT Endpoint (EP2 OUT, 7 bytes)
    7,
    DESC_TYPE_ENDPOINT,
    MSC_EP_OUT, // bEndpointAddress (0x02)
    0x02,       // bmAttributes: Bulk
    MSC_EP_SIZE,
    0, // wMaxPacketSize (64 bytes)
    0, // bInterval
    // 14. MSC Bulk IN Endpoint (EP1 IN, 7 bytes)
    7,
    DESC_TYPE_ENDPOINT,
    MSC_EP_IN, // bEndpointAddress (0x81)
    0x02,      // bmAttributes: Bulk
    MSC_EP_SIZE,
    0, // wMaxPacketSize (64 bytes)
    0, // bInterval
];

#[cfg(all(feature = "usb_msc", not(feature = "usb_cdc")))]
pub static CONFIG_DESCRIPTOR: [u8; 9 + 9 + 7 + 7] = [
    // 1. Configuration Header (9 bytes)
    9,
    DESC_TYPE_CONFIGURATION,
    (9 + 9 + 7 + 7) as u8,
    0,    // wTotalLength (32 bytes)
    1,    // bNumInterfaces: 1 (MSC)
    1,    // bConfigurationValue
    5,    // iConfiguration: String 5 (Live Power Telemetry)
    0xC0, // bmAttributes: Self-Powered
    50,   // bMaxPower: 100mA
    // 2. Mass Storage Class Interface (Interface 0, 9 bytes)
    9,
    DESC_TYPE_INTERFACE,
    0,    // bInterfaceNumber: 0
    0,    // bAlternateSetting: 0
    2,    // bNumEndpoints: 2
    0x08, // bInterfaceClass: Mass Storage (MSC)
    0x06, // bInterfaceSubClass: SCSI transparent command set
    0x50, // bInterfaceProtocol: Bulk-Only Transport (BOT)
    7,    // iInterface: String 7 ("Storage")
    // 3. MSC Bulk OUT Endpoint (EP2 OUT, 7 bytes)
    7,
    DESC_TYPE_ENDPOINT,
    MSC_EP_OUT, // bEndpointAddress (0x02)
    0x02,       // bmAttributes: Bulk
    MSC_EP_SIZE,
    0, // wMaxPacketSize (64 bytes)
    0, // bInterval
    // 4. MSC Bulk IN Endpoint (EP1 IN, 7 bytes)
    7,
    DESC_TYPE_ENDPOINT,
    MSC_EP_IN, // bEndpointAddress (0x81)
    0x02,      // bmAttributes: Bulk
    MSC_EP_SIZE,
    0, // wMaxPacketSize (64 bytes)
    0, // bInterval
];

#[cfg(all(feature = "usb_cdc", not(feature = "usb_msc")))]
pub static CONFIG_DESCRIPTOR: [u8; 9 + 8 + 9 + 5 + 5 + 4 + 5 + 7 + 9 + 7 + 7] = [
    // 1. Configuration Header (9 bytes)
    9,
    DESC_TYPE_CONFIGURATION,
    (9 + 8 + 9 + 5 + 5 + 4 + 5 + 7 + 9 + 7 + 7) as u8,
    0,
    2,    // bNumInterfaces: 2
    1,    // bConfigurationValue
    5,    // iConfiguration: String 5 (Live Power Telemetry e.g. "Supply: 8.12v")
    0xC0, // bmAttributes: Self-Powered
    50,   // bMaxPower: 100mA
    // 2. IAD
    8,
    DESC_TYPE_INTERFACE_ASSOCIATION,
    0,
    2,
    0x02,
    0x02,
    0x01,
    4, // iFunction: String 4 ("Serial")
    // 3. CDC Comm Interface 0
    9,
    DESC_TYPE_INTERFACE,
    0,
    0,
    1,
    0x02,
    0x02,
    0x01,
    4, // iInterface: String 4 ("Serial")
    5,
    DESC_TYPE_CS_INTERFACE,
    0x00,
    0x10,
    0x01,
    5,
    DESC_TYPE_CS_INTERFACE,
    0x01,
    0x00,
    1,
    4,
    DESC_TYPE_CS_INTERFACE,
    0x02,
    0x02,
    5,
    DESC_TYPE_CS_INTERFACE,
    0x06,
    0,
    1,
    7,
    DESC_TYPE_ENDPOINT,
    CDC_EP_NOTIF,
    0x03,
    CDC_NOTIF_SIZE,
    0,
    10,
    // 4. CDC Data Interface 1
    9,
    DESC_TYPE_INTERFACE,
    1,
    0,
    2,
    0x0A,
    0x00,
    0x00,
    6, // iInterface: String 6 (Hardware MAC e.g. "MAC: 02:4A:...")
    7,
    DESC_TYPE_ENDPOINT,
    CDC_EP_OUT,
    0x02,
    CDC_DATA_SIZE,
    0,
    0,
    7,
    DESC_TYPE_ENDPOINT,
    CDC_EP_IN,
    0x02,
    CDC_DATA_SIZE,
    0,
    0,
];

// Fallback if neither is active (base USB)
#[cfg(all(not(feature = "usb_cdc"), not(feature = "usb_msc")))]
pub static CONFIG_DESCRIPTOR: [u8; 9] = [9, DESC_TYPE_CONFIGURATION, 9, 0, 0, 1, 0, 0xC0, 50];

// ── Standard String Descriptors ─────────────────────────────────────────────
pub static STRING_LANGID: [u8; 4] = [
    4,
    DESC_TYPE_STRING,
    0x09,
    0x04, // 0x0409 English (US)
];

#[cfg(feature = "usb_msc")]
pub static STRING_INTERFACE_STORAGE: [u8; 16] = [
    16,
    DESC_TYPE_STRING,
    b'S',
    0,
    b't',
    0,
    b'o',
    0,
    b'r',
    0,
    b'a',
    0,
    b'g',
    0,
    b'e',
    0,
];
