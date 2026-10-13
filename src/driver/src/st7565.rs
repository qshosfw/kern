use kern_platform_traits::{gpio::OutputPin, spi::SpiDevice};
use kernel::canvas::Canvas;

pub const LCD_WIDTH: usize = 128;
pub const LCD_HEIGHT: usize = 64;

pub struct St7565<'a> {
    spi: &'a mut dyn SpiDevice,
    cs: &'a mut dyn OutputPin,
    a0: &'a mut dyn OutputPin,
}

impl<'a> St7565<'a> {
    pub fn new(
        spi: &'a mut dyn SpiDevice,
        cs: &'a mut dyn OutputPin,
        a0: &'a mut dyn OutputPin,
    ) -> Self {
        Self { spi, cs, a0 }
    }

    #[inline(always)]
    fn write_cmd(&mut self, cmd: u8) {
        self.a0.set_low();
        self.spi.write_byte(cmd);
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn write_data(&mut self, data: u8) {
        self.a0.set_high();
        self.spi.write_byte(data);
    }

    pub fn init(&mut self) {
        self.cs.set_low();

        // 1. Software Reset (20ms is 20x ST7565 spec requirement of 1ms)
        self.write_cmd(0xE2);
        py32_hal::delay::delay_ms(20);

        // 2. LCD Operating Mode & Bias (1/9)
        self.write_cmd(0xA2); // Bias 1/9
        self.write_cmd(0xC0); // COM scan direction normal
        self.write_cmd(0xA1); // SEG scan direction reverse (0xA0 | 1)
        self.write_cmd(0xA6); // Inverse display off
        self.write_cmd(0xA4); // All pixel on off
        self.write_cmd(0x24); // Regulation ratio 5.0 (0x20 | 4)

        // 3. Contrast (Electronic Volume: 31 = optimal 50% default per DeltaFW/Fagci)
        self.write_cmd(0x81);
        self.write_cmd(31);

        // 4. Power Circuit Control Sequence
        self.write_cmd(0x28 | 0b011); // VB=0, VR=1, VF=1
        py32_hal::delay::delay_ms(1);
        self.write_cmd(0x28 | 0b110); // VB=1, VR=1, VF=0
        py32_hal::delay::delay_ms(1);
        for _ in 0..4 {
            self.write_cmd(0x28 | 0b111); // VB=1, VR=1, VF=1
        }
        py32_hal::delay::delay_ms(20); // Voltage booster stabilization

        // 5. Set Start Line 0 & Turn Display ON (0xAF)
        self.write_cmd(0x40);
        self.write_cmd(0xAF);

        self.cs.set_high();
    }

    /// Turn on the LCD panel output (0xAF) cleanly
    pub fn turn_on(&mut self) {
        self.cs.set_low();
        self.write_cmd(0x40); // Start line 0
        self.write_cmd(0xAF); // Display ON
        self.cs.set_high();
    }

    /// Turn off the LCD panel output (0xAE)
    pub fn turn_off(&mut self) {
        self.cs.set_low();
        self.write_cmd(0xAE); // Display OFF
        self.cs.set_high();
    }

    pub fn set_contrast(&mut self, contrast_percent: u8) {
        // Map 0..=100% user setting to optimal ST7565 Electronic Volume (EV) range 21..=37.
        // EV = 29..31 is the factory default optimum (50% user contrast).
        // Hardware EV values > 40 cause liquid crystal over-drive and pixel ghosting on ST7565.
        let ev = 21 + ((contrast_percent.min(100) as u16 * 16) / 100) as u8;
        self.cs.set_low();
        self.write_cmd(0x81);
        self.write_cmd(ev & 0x3F);
        self.cs.set_high();
    }

    pub fn set_inverted(&mut self, inverted: bool) {
        self.cs.set_low();
        if inverted {
            self.write_cmd(0xA7); // Inverse display ON (DeltaFW: 0xA6 | 1)
        } else {
            self.write_cmd(0xA6); // Normal display OFF (DeltaFW: 0xA6 | 0)
        }
        self.cs.set_high();
    }

    pub fn blit(&mut self, canvas: &mut Canvas) {
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
                for &byte in &canvas.buffer[page] {
                    self.spi.write_byte(byte);
                }
            }
        }

        canvas.dirty = 0;
        self.spi.wait_idle();
        self.cs.set_high();
    }
}
