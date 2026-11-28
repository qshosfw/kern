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
    pub refresh_rate: u8, // Animation tick: 0=50ms (Quiet), 1=18ms (Std), 2=15ms (Turbo), 3=12ms (Ultra)
    pub splash_path: [u8; 32], // e.g. "/user/splash/01_quansheng.rle"
    pub crc32: u32,
}

/// UI tick period in milliseconds while an animation is running, indexed by `refresh_rate`.
///
/// Animation controllers advance a fixed step per tick, so this table is the single place
/// that sets animation pace for the panel and for headless RigWire viewers alike.
#[cfg(feature = "animations")]
#[inline(always)]
pub fn anim_step_ms(refresh_rate: u8) -> u32 {
    const STEP_MS: [u8; 4] = [50, 18, 15, 12];
    STEP_MS[if refresh_rate < 4 {
        refresh_rate as usize
    } else {
        1
    }] as u32
}

impl DisplayConfig {
    pub const DEFAULT: Self = Self::new();

    pub const fn new() -> Self {
        Self {
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
            refresh_rate: 1,         // 18 ms animation tick by default
            splash_path: *b"/user/splash/01_quansheng.rle\0\0\0",
            crc32: 0,
        }
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
        super::read_cstr(&self.splash_path)
    }

    pub fn set_splash_path(&mut self, path: &str) {
        super::write_cstr(&mut self.splash_path, path);
    }

    fn backlight_timeout_code(&self) -> u16 {
        if self.backlight_mode >= 5 {
            u16::MAX
        } else {
            (::abi::prop::options::BACKLIGHT_TIMEOUT_S[self.backlight_mode as usize] as u16) * 1000
        }
    }

    pub fn apply_to_backlight<O: drivers::backlight::BacklightOutput>(
        &self,
        backlight: &mut drivers::backlight::BacklightController<O>,
    ) {
        let mode = match self.backlight_timeout_code() {
            0 => drivers::backlight::BacklightMode::Off,
            u16::MAX => drivers::backlight::BacklightMode::On,
            ms => drivers::backlight::BacklightMode::Auto(ms),
        };
        backlight.set_mode(mode);
        backlight.set_levels(self.backlight_max, self.backlight_min);
    }
}

crate::config_block!(DisplayConfig, "/pref/display.cfg", 3);
