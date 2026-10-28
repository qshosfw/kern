#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DisplayConfig {
    pub version: u8,              // 3
    pub backlight_mode: u8,       // 0=Off, 1=5s, 2=10s, 3=30s, 4=1m, 5=Always On
    pub backlight_max: u8,        // 1..10
    pub backlight_min: u8,        // 0..10
    pub contrast: u8,             // 0..100%
    pub inverted: u8,             // 0=Normal, 1=Inverted
    pub boot_splash_anim: u8,     // 0=None, 1=Dither Fill, 2=Invert Dissolve, 3=Curtain Wipe
    pub boot_splash_mode: u8,     // 0=None/Off, 1=Active
    pub boot_splash_duration: u8, // 0=Skip, 1=0.5s, 2=1.0s, 3=2.0s, 4=3.0s
    pub shading_mode: u8, // 0=Grayscale (2-bit FRC), 1=Dither (1-bit Bayer), 2=Monochrome (1-bit Binary)
    pub ui_animations: u8, // 0=Off, 1=On
    pub refresh_rate: u8, // 0=20Hz (Quiet), 1=60Hz (Std), 2=75Hz (Turbo), 3=90Hz (Ultra)
    pub splash_path: [u8; 32], // e.g. "/user/splash/01_quansheng.rle"
    pub crc32: u32,
}

impl DisplayConfig {
    pub const fn new() -> Self {
        let mut cfg = Self {
            version: 3,
            backlight_mode: 1,
            backlight_max: 10,
            backlight_min: 0,
            contrast: 50,
            inverted: 0,
            boot_splash_anim: 1,     // Dither Fill by default
            boot_splash_mode: 1,     // On by default
            boot_splash_duration: 2, // 1.0s by default
            shading_mode: 1,         // Dither (1-bit Bayer) by default: 100% flicker-free on ST7565
            ui_animations: 1,        // Kinetic animations On by default
            refresh_rate: 1,         // 60 Hz by default (matches 16ms LC response time & flashmap)
            splash_path: [0; 32],
            crc32: 0,
        };
        const DEF_PATH: &[u8] = b"/user/splash/01_quansheng.rle";
        let mut i = 0;
        while i < DEF_PATH.len() {
            cfg.splash_path[i] = DEF_PATH[i];
            i += 1;
        }
        cfg
    }

    #[inline(always)]
    pub const fn animations_enabled(&self) -> bool {
        #[cfg(feature = "animations")]
        {
            self.ui_animations != 0
        }
        #[cfg(not(feature = "animations"))]
        {
            false
        }
    }

    pub fn splash_path_str(&self) -> &str {
        let len = gfx::text::find_byte(&self.splash_path, 0).unwrap_or(self.splash_path.len());
        unsafe { core::str::from_utf8_unchecked(&self.splash_path[..len]) }
    }

    pub fn set_splash_path(&mut self, path: &str) {
        self.splash_path = [0; 32];
        let bytes = path.as_bytes();
        let copy_len = bytes.len().min(self.splash_path.len());
        self.splash_path[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    pub fn apply_to_backlight(&self, backlight: &mut drivers::backlight::BacklightController) {
        let mode = match self.backlight_mode {
            0 => drivers::backlight::BacklightMode::Off,
            1 => drivers::backlight::BacklightMode::Auto(5000),
            2 => drivers::backlight::BacklightMode::Auto(10000),
            3 => drivers::backlight::BacklightMode::Auto(30000),
            4 => drivers::backlight::BacklightMode::Auto(60000),
            _ => drivers::backlight::BacklightMode::On,
        };
        backlight.set_mode(mode);
        backlight.set_levels(self.backlight_max, self.backlight_min);
    }

    pub fn backlight_mode_service(&self) -> crate::services::BacklightMode {
        match self.backlight_mode {
            0 => crate::services::BacklightMode::Off,
            1 => crate::services::BacklightMode::Auto(5000),
            2 => crate::services::BacklightMode::Auto(10000),
            3 => crate::services::BacklightMode::Auto(30000),
            4 => crate::services::BacklightMode::Auto(60000),
            _ => crate::services::BacklightMode::On,
        }
    }
}

crate::config_block!(DisplayConfig, "/pref/display.cfg", 3);
