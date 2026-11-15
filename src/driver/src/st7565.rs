use kern_platform_traits::{gpio::OutputPin, spi::SpiDevice};
use kernel::canvas::Canvas;

pub const LCD_WIDTH: usize = 128;
pub const LCD_HEIGHT: usize = 64;

pub struct St7565<'a> {
    spi: &'a mut dyn SpiDevice,
    cs: &'a mut dyn OutputPin,
    a0: &'a mut dyn OutputPin,
    pub subframe: u8,
    pub motion_bypass: bool,
    pub shading_mode: u8,
}

impl<'a> St7565<'a> {
    pub fn new(
        spi: &'a mut dyn SpiDevice,
        cs: &'a mut dyn OutputPin,
        a0: &'a mut dyn OutputPin,
    ) -> Self {
        Self {
            spi,
            cs,
            a0,
            subframe: 0,
            motion_bypass: false,
            shading_mode: 1,
        }
    }

    #[inline(always)]
    fn write_cmd(&mut self, cmd: u8) {
        self.a0.set_low();
        self.spi.write_byte(cmd);
    }

    #[inline(always)]
    fn end_transfer(&mut self) {
        self.spi.wait_idle();
        self.cs.set_high();
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn write_data(&mut self, data: u8) {
        self.a0.set_high();
        self.spi.write_byte(data);
    }

    #[inline(never)]
    fn send_cmds(&mut self, cmds: &[u8]) {
        self.cs.set_low();
        self.a0.set_low();
        for &cmd in cmds {
            self.spi.write_byte(cmd);
        }
        self.spi.wait_idle();
        self.cs.set_high();
    }

    pub fn init(&mut self) {
        // 1. Software Reset (20ms is 20x ST7565 spec requirement of 1ms)
        self.send_cmds(&[0xE2]);
        py32_hal::delay::delay_ms(20);

        // 2-4a. Bias, scan, normal display, regulation, contrast, power step 1
        self.send_cmds(&[0xA2, 0xC0, 0xA1, 0xA6, 0xA4, 0x24, 0x81, 31, 0x28 | 0b011]);
        py32_hal::delay::delay_ms(1);
        self.send_cmds(&[0x28 | 0b110]); // VB=1, VR=1, VF=0
        py32_hal::delay::delay_ms(1);
        self.send_cmds(&[0x2F, 0x2F, 0x2F, 0x2F]); // VB=1, VR=1, VF=1 x4
        py32_hal::delay::delay_ms(20); // Voltage booster stabilization

        // 5. Set Start Line 0 & Turn Display ON (0xAF)
        self.send_cmds(&[0x40, 0xAF]);
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
        py32_hal::delay::delay_ms(2);
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
        if bypass {
            py32_hal::spi::set_spi1_speed(py32_hal::spi::Spi1Speed::Turbo);
        } else {
            py32_hal::spi::set_spi1_speed(py32_hal::spi::Spi1Speed::Standard);
        }
    }

    /// Explicitly set the SPI1 interface speed profile.
    pub fn set_speed(&mut self, speed: py32_hal::spi::Spi1Speed) {
        py32_hal::spi::set_spi1_speed(speed);
    }

    /// Transmit canvas data to ST7565 LCD over hardware SPI.
    /// In grayscale mode with temporal FRC enabled and motion_bypass false,
    /// transmits the spatio-temporal phase slice corresponding to `self.subframe`.
    pub fn blit(&mut self, canvas: &mut Canvas) {
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
    pub fn blit_subframe(&mut self, canvas: &mut Canvas, #[allow(unused_variables)] subframe: u8) {
        if canvas.dirty == 0 {
            return;
        }

        self.cs.set_low();
        self.write_cmd(0x40); // Set start line 0

        for page in 0..8 {
            if (canvas.dirty & (1 << page)) != 0 {
                // Select Page and Column (Offset 4 for K1 ST7565 panel)
                self.a0.set_low();
                const COL_OFFSET: u8 = 4;
                self.spi.write_byte(0xB0 | (page as u8 & 0x0F));
                self.spi.write_byte(0x10 | (COL_OFFSET >> 4));
                self.spi.write_byte(COL_OFFSET & 0x0F);

                // Transmit 128 data bytes for this page
                self.a0.set_high();
                #[cfg(feature = "grayscale")]
                if self.shading_mode == 0 {
                    for col in 0..LCD_WIDTH {
                        let byte = canvas.get_subframe_byte(page, col, subframe);
                        self.spi.write_byte(byte);
                    }
                } else if self.shading_mode == 1 {
                    for col in 0..LCD_WIDTH {
                        let byte = canvas.get_dither_byte(page, col);
                        self.spi.write_byte(byte);
                    }
                } else {
                    for &byte in &canvas.buffer[page] {
                        self.spi.write_byte(byte);
                    }
                }
                #[cfg(not(feature = "grayscale"))]
                for &byte in &canvas.buffer[page] {
                    self.spi.write_byte(byte);
                }
            }
        }

        canvas.dirty = 0;
        self.end_transfer();
    }
}
