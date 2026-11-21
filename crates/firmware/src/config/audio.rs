#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AudioConfig {
    pub version: u8,           // 3
    pub volume: u8,            // 0..10: AF Preamp / DAC Gain (Reg 0x48)
    pub key_beep: u8,          // 0=Off, 1=Tone, 2=Click, 3=Double, 4=Warning, 5=Scroll, 6=Custom
    pub mic_gain: u8,          // 0..31 dB (Reg 0x7D)
    pub roger_beep: u8,        // 0=Off, 1=Active
    pub voice_prompt: u8,      // 0=Off, 1=Beep
    pub beep_gain: u8,         // 0..10: Tone Generator Gain (Reg 0x70)
    pub boot_jingle: u8,       // 0=Off, 1=Chime, 2=Nokia, 3=Chirp, 4=GameBoy
    pub audio_profile: u8,     // 0=Flat, 1=Clean, 2=Mid, 3=Boost, 4=Max (Reg 0x54/0x55)
    pub roger_path: [u8; 32],  // e.g. "/user/rogers/01_classic.ton" (up to 32 chars)
    pub jingle_path: [u8; 32], // e.g. "/user/jingles/01_chime.ton" (up to 32 chars)
    pub _pad: [u8; 3],         // Explicit 4-byte alignment padding before crc32
    pub crc32: u32,
}

impl AudioConfig {
    pub const DEFAULT: Self = Self::new();

    pub const fn new() -> Self {
        let mut cfg = Self {
            version: 3,
            volume: 5,
            key_beep: 1,
            mic_gain: 20,     // 20 dB standard mic gain
            roger_beep: 0,    // Off
            voice_prompt: 1,  // Beep
            beep_gain: 5,     // Default tone gain (~48 in Reg 0x70)
            boot_jingle: 1,   // Default Chime
            audio_profile: 0, // Default Flat
            roger_path: [0; 32],
            jingle_path: [0; 32],
            _pad: [0; 3],
            crc32: 0,
        };
        const DEF_ROGER: &[u8] = b"/user/rogers/01_classic.ton";
        let mut i = 0;
        while i < DEF_ROGER.len() {
            cfg.roger_path[i] = DEF_ROGER[i];
            i += 1;
        }
        const DEF_JINGLE: &[u8] = b"/user/jingles/01_chime.ton";
        let mut j = 0;
        while j < DEF_JINGLE.len() {
            cfg.jingle_path[j] = DEF_JINGLE[j];
            j += 1;
        }
        cfg
    }

    pub fn roger_path_str(&self) -> &str {
        super::read_cstr(&self.roger_path)
    }

    pub fn set_roger_path(&mut self, path: &str) {
        super::write_cstr(&mut self.roger_path, path);
    }

    pub fn jingle_path_str(&self) -> &str {
        super::read_cstr(&self.jingle_path)
    }

    pub fn set_jingle_path(&mut self, path: &str) {
        super::write_cstr(&mut self.jingle_path, path);
    }
}

crate::config_block!(AudioConfig, "/pref/audio.cfg", 3);
