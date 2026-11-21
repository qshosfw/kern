#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SystemConfig {
    pub version: u8,            // 5
    pub model: u8,              // 0=UV-K1, 1=UV-K5 V3, 2=Custom
    pub uart_baud: u8,          // 0=115200, 1=38400
    pub cpu_clock: u8,          // 0=16 MHz, 1=24 MHz, 2=32 MHz, 3=48 MHz, 4=72 MHz
    pub radio_id: u32,          // Standard Radio ID / DMR ID / Fleet ID
    pub owner: [u8; 12],        // Owner Call Sign / Name
    pub tag: [u8; 8],           // Tag / Device Identifier (formerly asset_tag)
    pub custom_model: [u8; 12], // Custom model string e.g. "UV-K1(8)", "UV-K1s"
    pub lang_active: u8,        // 0=Built-in English, 1=External .lng pack at lang_path
    pub _lang_pad: [u8; 3],
    pub lang_path: [u8; 32], // e.g. "/sys/lang/ja.lng"
    pub crc32: u32,
}

#[inline(never)]
fn field_str<'a>(bytes: &'a [u8], default: &'a str, empty_is_default: bool) -> &'a str {
    let mut len = 0;
    while len < bytes.len() && bytes[len] != 0 && bytes[len] != b' ' {
        if bytes[len] < 32 || bytes[len] > 126 {
            return default;
        }
        len += 1;
    }
    if len == 0 && empty_is_default {
        default
    } else {
        unsafe { core::str::from_utf8_unchecked(bytes.get_unchecked(..len)) }
    }
}

impl SystemConfig {
    pub const DEFAULT: Self = Self::new();

    pub const fn new() -> Self {
        Self {
            version: 5,
            model: 0,
            uart_baud: 0, // 115200 default
            cpu_clock: 3, // 48 MHz default
            radio_id: 1,
            owner: *b"N0CALL      ",
            tag: *b"00000000",
            custom_model: *b"UV-K1       ",
            lang_active: 0,
            _lang_pad: [0; 3],
            lang_path: [0; 32],
            crc32: 0,
        }
    }

    pub fn lang_path_str(&self) -> &str {
        super::read_cstr(&self.lang_path)
    }

    pub const fn baud_rate(&self) -> u32 {
        if self.uart_baud == 1 { 38400 } else { 115200 }
    }

    pub const fn nav_layout(&self) -> u8 {
        if self.is_k1() { 0 } else { 1 }
    }

    pub const fn is_k1(&self) -> bool {
        if self.model == 0 {
            true
        } else if self.model == 1 {
            false
        } else {
            let bytes = &self.custom_model;
            (bytes[0] == b'U'
                && bytes[1] == b'V'
                && bytes[2] == b'-'
                && bytes[3] == b'K'
                && bytes[4] == b'1')
                || (bytes[0] == b'K' && bytes[1] == b'1')
                || (bytes[0] == b'k' && bytes[1] == b'1')
        }
    }

    pub fn model_str(&self) -> &str {
        match self.model {
            0 => "UV-K1",
            1 => "UV-K5 V3",
            _ => self.custom_model_str(),
        }
    }

    pub fn custom_model_str(&self) -> &str {
        field_str(&self.custom_model, "Custom", true)
    }

    #[inline(never)]
    pub fn set_custom_model(&mut self, s: &str) {
        self.custom_model = [b' '; 12];
        let bytes = utils::format::trim_ascii_bytes(s.as_bytes());
        let l = bytes.len().min(12);
        self.custom_model[..l].copy_from_slice(&bytes[..l]);
        let trimmed = unsafe { core::str::from_utf8_unchecked(bytes) };

        const KNOWN: [(&str, u8); 6] = [
            ("UV-K1", 0),
            ("K1", 0),
            ("UV-K5 V3", 1),
            ("k5v3", 1),
            ("UV-K5", 1),
            ("K5", 1),
        ];
        self.model = 2;
        for &(pat, id) in &KNOWN {
            if utils::format::ascii_eq_ignore_case(trimmed, pat) {
                self.model = id;
                break;
            }
        }
    }

    pub fn init_default_tag(&mut self) {
        let tag4 = bsp::uid::get_mac_tag();
        self.tag = [tag4[0], tag4[1], tag4[2], tag4[3], b' ', b' ', b' ', b' '];
    }

    pub fn owner_str(&self) -> &str {
        field_str(&self.owner, "N0CALL", false)
    }

    pub fn tag_str(&self) -> &str {
        let s = field_str(&self.tag, "", false);
        if s.is_empty() || s == "00000000" {
            bsp::uid::get_mac_tag_str()
        } else {
            s
        }
    }

    pub fn asset_tag_str(&self) -> &str {
        self.tag_str()
    }
}

crate::config_block!(SystemConfig, "/pref/system.cfg", 5);
