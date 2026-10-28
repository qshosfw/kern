pub use kernel::service::{BacklightMode, DisplayService, LedService};

use drivers::backlight::BacklightController;
use drivers::st7565::St7565;

pub struct DisplayHardware<'a, 'b> {
    pub lcd: &'a mut St7565<'b>,
    pub canvas: &'a mut gfx::Canvas,
    pub backlight: &'a mut BacklightController,
}

impl<'a, 'b> DisplayService for DisplayHardware<'a, 'b> {
    fn set_contrast(&mut self, contrast: u8) {
        self.lcd.set_contrast(contrast);
    }

    fn set_inverted(&mut self, inverted: bool) {
        self.lcd.set_inverted(inverted);
    }

    fn set_shading_mode(&mut self, mode: u8) {
        self.lcd.shading_mode = mode;
    }

    fn set_backlight_mode(&mut self, mode: BacklightMode) {
        let bmode = match mode {
            BacklightMode::Off => drivers::backlight::BacklightMode::Off,
            BacklightMode::Auto(ms) => drivers::backlight::BacklightMode::Auto(ms),
            BacklightMode::On => drivers::backlight::BacklightMode::On,
        };
        self.backlight.set_mode(bmode);
    }

    fn set_backlight_brightness(&mut self, max: u8, min: u8) {
        self.backlight.set_levels(max, min);
    }

    fn set_keypad_light_mode(&mut self, mode: u8) {
        self.backlight.set_keypad_mode(mode);
    }

    fn wake(&mut self) {
        self.backlight.wake();
    }

    fn show_progress(&mut self, title: &str, subtitle: &str, progress: u8) {
        gfx::widgets::draw_modal_progress(self.canvas, title, subtitle, progress);
        self.lcd.blit(self.canvas);
    }
}
