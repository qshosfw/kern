use kern_platform_traits::display::{CommandDisplayBus, PagedFramebuffer};
use kern_platform_traits::spi::SpiSpeed;

pub const LCD_WIDTH: usize = 128;
pub const LCD_HEIGHT: usize = 64;

pub struct St7565<B> {
    bus: B,
    pub subframe: u8,
    pub motion_bypass: bool,
    pub shading_mode: u8,
}

impl<B: CommandDisplayBus> St7565<B> {
    pub const fn new(bus: B) -> Self {
        Self {
            bus,
            subframe: 0,
            motion_bypass: false,
            shading_mode: 1,
        }
    }

    #[inline(always)]
    fn write_cmd(&mut self, cmd: u8) {
        self.bus.dc_low();
        self.bus.write_byte(cmd);
    }

    #[inline(always)]
    fn end_transfer(&mut self) {
        self.bus.wait_idle();
        self.bus.cs_high();
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn write_data(&mut self, data: u8) {
        self.bus.dc_high();
        self.bus.write_byte(data);
    }

    #[inline(never)]
    fn send_cmds(&mut self, cmds: &[u8]) {
        self.bus.cs_low();
        self.bus.dc_low();
        for &cmd in cmds {
            self.bus.write_byte(cmd);
        }
        self.end_transfer();
    }

    pub fn init(&mut self) {
        // Packed init steps: [cmd_len, cmd_bytes..., delay_ms]
        static INIT_SEQ: [u8; 27] = [
            1, 0xE2, 20, // Software Reset
            9, 0xA2, 0xC0, 0xA1, 0xA6, 0xA4, 0x24, 0x81, 31, 0x2B, 1, // Bias..power step 1
            1, 0x2E, 1, // VB=1, VR=1, VF=0
            4, 0x2F, 0x2F, 0x2F, 0x2F, 20, // VB=1, VR=1, VF=1 x4
            2, 0x40, 0xAF, 0, // Start line 0 & ON
        ];
        let mut i = 0usize;
        while i < INIT_SEQ.len() {
            let n = INIT_SEQ[i] as usize;
            let start = i + 1;
            let end = start + n;
            self.send_cmds(&INIT_SEQ[start..end]);
            let ms = INIT_SEQ[end] as u32;
            if ms != 0 {
                self.bus.delay_ms(ms);
            }
            i = end + 1;
        }
    }

    /// Turn on the LCD panel output (0xAF) cleanly
    pub fn turn_on(&mut self) {
        self.send_cmds(&[0x40, 0xAF]);
    }

    /// Turn off the LCD panel output (0xAE)
    pub fn turn_off(&mut self) {
        self.send_cmds(&[0xAE]);
    }

    /// Enter ST7565 sub-microamp Sleep mode (`0xAE` Display OFF + `0xA5` All Points ON).
    /// Retains display RAM and bias configuration so `exit_sleep()` avoids a 42 ms cold reset.
    pub fn enter_sleep(&mut self) {
        self.send_cmds(&[0xAE, 0xA5]);
    }

    /// Exit ST7565 Sleep mode cleanly (`0x2F` booster + `0xA4` + `0x40` + `0xAF`).
    pub fn exit_sleep(&mut self) {
        self.send_cmds(&[0x2F]); // Re-enable VB/VR/VF power circuits
        #[cfg(target_arch = "arm")]
        self.bus.delay_ms(2);
        self.send_cmds(&[0xA4, 0x40, 0xAF]);
    }

    pub fn set_contrast(&mut self, contrast_percent: u8) {
        // Map 0..=100% user setting to optimal ST7565 Electronic Volume (EV) range 21..=41.
        // EV = 31 is the factory default optimum (50% user contrast, 21 + 10 = 31 per DeltaFW).
        // Hardware EV values > 45 cause liquid crystal over-drive and pixel ghosting on ST7565.
        let pct = contrast_percent.min(100) as u16;
        let ev = 21 + utils::math::div10_u16(pct * 2).0 as u8;
        self.send_cmds(&[0x81, ev & 0x3F]);
    }

    pub fn set_inverted(&mut self, inverted: bool) {
        self.send_cmds(&[if inverted { 0xA7 } else { 0xA6 }]);
    }

    /// Dynamically enable or disable motion bypass.
    /// When true (e.g. during kinetic list scrolling or lateral transitions),
    /// the driver switches to Turbo SPI (12 MHz) and transmits crisp 1-bit high-contrast
    /// frames to prevent STN liquid crystal ghosting and smearing.
    pub fn set_motion_bypass(&mut self, bypass: bool) {
        self.motion_bypass = bypass;
        self.bus.set_speed(if bypass {
            SpiSpeed::Turbo
        } else {
            SpiSpeed::Standard
        });
    }

    /// Explicitly set the SPI interface speed profile.
    pub fn set_speed(&mut self, speed: SpiSpeed) {
        self.bus.set_speed(speed);
    }

    /// Transmit canvas data to ST7565 LCD over hardware SPI.
    /// In grayscale mode with temporal FRC enabled and motion_bypass false,
    /// transmits the spatio-temporal phase slice corresponding to `self.subframe`.
    pub fn blit<F: PagedFramebuffer>(&mut self, canvas: &mut F) {
        let subframe = self.subframe;
        self.blit_subframe(canvas, subframe);
        #[cfg(feature = "grayscale")]
        if self.shading_mode == 0 {
            self.subframe = if self.subframe >= 2 {
                0
            } else {
                self.subframe + 1
            };
        }
    }

    /// Blit an explicit subframe slice (0..3) to the ST7565 LCD.
    pub fn blit_subframe<F: PagedFramebuffer>(
        &mut self,
        canvas: &mut F,
        #[allow(unused_variables)] subframe: u8,
    ) {
        let dirty = canvas.dirty_mask();
        if dirty == 0 {
            return;
        }

        self.bus.cs_low();
        self.write_cmd(0x40); // Set start line 0

        for page in 0..8 {
            if (dirty & (1 << page)) != 0 {
                // Select Page and Column (Offset 4 for K1 ST7565 panel)
                self.bus.dc_low();
                const COL_OFFSET: u8 = 4;
                self.bus.write_byte(0xB0 | (page as u8 & 0x0F));
                self.bus.write_byte(0x10 | (COL_OFFSET >> 4));
                self.bus.write_byte(COL_OFFSET & 0x0F);

                // Transmit 128 data bytes for this page
                self.bus.dc_high();
                #[cfg(feature = "grayscale")]
                if self.shading_mode == 0 {
                    for col in 0..LCD_WIDTH {
                        let byte = canvas.subframe_byte(page, col, subframe);
                        self.bus.write_byte(byte);
                    }
                } else if self.shading_mode == 1 {
                    for col in 0..LCD_WIDTH {
                        let byte = canvas.dither_byte(page, col);
                        self.bus.write_byte(byte);
                    }
                } else {
                    for &byte in canvas.page_bytes(page) {
                        self.bus.write_byte(byte);
                    }
                }
                #[cfg(not(feature = "grayscale"))]
                for &byte in canvas.page_bytes(page) {
                    self.bus.write_byte(byte);
                }
            }
        }

        canvas.clear_dirty();
        self.end_transfer();
    }
}
