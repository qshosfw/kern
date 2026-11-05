#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SystemConfig {
    pub version: u8,            // 4
    pub model: u8,              // 0=UV-K1, 1=UV-K5 V3, 2=Custom
    pub uart_baud: u8,          // 0=115200, 1=38400
    pub cpu_clock: u8,          // 0=48 MHz, 1=72 MHz (PY32F071 high-perf mode)
    pub radio_id: u32,          // Standard Radio ID / DMR ID / Fleet ID
    pub owner: [u8; 12],        // Owner Call Sign / Name
    pub tag: [u8; 8],           // Tag / Device Identifier (formerly asset_tag)
    pub custom_model: [u8; 12], // Custom model string e.g. "UV-K1(8)", "UV-K1s"
    pub crc32: u32,
}

#[inline(always)]
fn ascii_or<'a>(bytes: &'a [u8], default: &'a str) -> &'a str {
    for &b in bytes {
        if b < 32 || b > 126 {
            return default;
        }
    }
    unsafe { core::str::from_utf8_unchecked(bytes) }
}

impl SystemConfig {
    pub const fn new() -> Self {
        Self {
            version: 4,
            model: 0,
            uart_baud: 0, // 115200 default
            cpu_clock: 1, // 72 MHz default (ensures stable USB and high performance)
            radio_id: 1,
            owner: *b"N0CALL      ",
            tag: *b"00000000",
            custom_model: *b"UV-K1       ",
            crc32: 0,
        }
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
        let len = self
            .custom_model
            .iter()
            .position(|&b| b == 0 || b == b' ')
            .unwrap_or(self.custom_model.len());
        if len == 0 {
            "Custom"
        } else {
            ascii_or(&self.custom_model[..len], "Custom")
        }
    }

    pub fn set_custom_model(&mut self, s: &str) {
        let trimmed = s.trim_ascii();
        self.custom_model = [b' '; 12];
        let bytes = trimmed.as_bytes();
        let l = bytes.len().min(12);
        self.custom_model[..l].copy_from_slice(&bytes[..l]);

        if trimmed.eq_ignore_ascii_case("UV-K1") || trimmed.eq_ignore_ascii_case("K1") {
            self.model = 0;
        } else if trimmed.eq_ignore_ascii_case("UV-K5 V3")
            || trimmed.eq_ignore_ascii_case("k5v3")
            || trimmed.eq_ignore_ascii_case("UV-K5")
            || trimmed.eq_ignore_ascii_case("K5")
        {
            self.model = 1;
        } else {
            self.model = 2; // Custom
        }
    }

    pub fn init_default_tag(&mut self) {
        let tag4 = bsp::uid::get_mac_tag();
        self.tag = [tag4[0], tag4[1], tag4[2], tag4[3], b' ', b' ', b' ', b' '];
    }

    pub fn owner_str(&self) -> &str {
        let len = self
            .owner
            .iter()
            .position(|&b| b == 0 || b == b' ')
            .unwrap_or(self.owner.len());
        ascii_or(&self.owner[..len], "N0CALL")
    }

    pub fn tag_str(&self) -> &str {
        let len = self
            .tag
            .iter()
            .position(|&b| b == 0 || b == b' ')
            .unwrap_or(self.tag.len());
        if len == 0 || &self.tag[..len] == b"00000000" {
            bsp::uid::get_mac_tag_str()
        } else {
            ascii_or(&self.tag[..len], bsp::uid::get_mac_tag_str())
        }
    }

    pub fn asset_tag_str(&self) -> &str {
        self.tag_str()
    }
}

crate::config_block!(SystemConfig, "/pref/system.cfg", 4);
