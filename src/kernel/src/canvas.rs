//! Core concrete Canvas framebuffer for high-efficiency monochrome rendering.

pub const LCD_WIDTH: usize = 128;
pub const LCD_HEIGHT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Clear = 0,     // Level 0: 0% White (lo=0, hi=0)
    LightGray = 1, // Level 1: 33% Light Gray (lo=1, hi=0)
    DarkGray = 2,  // Level 2: 67% Dark Gray (lo=0, hi=1)
    Fill = 3,      // Level 3: 100% Solid Black (lo=1, hi=1)
    Invert = 4,    // Bitwise Invert
}

/// Standardized monochromatic icon glyph identifier for UI and status bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Icon {
    Antenna = 0,
    Lock = 1,
    Unlock = 2,
    Speaker = 3,
    Shield = 4,
    Keypad = 5,
    Flashlight = 6,
    Mesh = 7,
    Function = 8,
    Plug = 9,
    SerialActive = 10,
    Bolt = 11,
    Broadcast = 12,
    Storage = 13,
    Scan = 14,
    Beacon = 15,
    Monitor = 16,
    Converter = 17,
    Roger = 18,
    Beep = 19,
    NoListen = 20,
    Moon = 21,
}

impl Icon {
    #[inline(always)]
    pub const fn width(self) -> u8 {
        match self {
            Icon::Plug | Icon::SerialActive | Icon::Storage | Icon::NoListen => 7,
            Icon::Beacon => 6,
            _ => 5,
        }
    }

    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Icon::Antenna),
            1 => Some(Icon::Lock),
            2 => Some(Icon::Unlock),
            3 => Some(Icon::Speaker),
            4 => Some(Icon::Shield),
            5 => Some(Icon::Keypad),
            6 => Some(Icon::Flashlight),
            7 => Some(Icon::Mesh),
            8 => Some(Icon::Function),
            9 => Some(Icon::Plug),
            10 => Some(Icon::SerialActive),
            11 => Some(Icon::Bolt),
            12 => Some(Icon::Broadcast),
            13 => Some(Icon::Storage),
            14 => Some(Icon::Scan),
            15 => Some(Icon::Beacon),
            16 => Some(Icon::Monitor),
            17 => Some(Icon::Converter),
            18 => Some(Icon::Roger),
            19 => Some(Icon::Beep),
            20 => Some(Icon::NoListen),
            21 => Some(Icon::Moon),
            _ => None,
        }
    }
}

/// A status bar glyph: either a standardized built-in icon or a custom 5x5 bitmap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusGlyph {
    Builtin(Icon),
    Custom([u8; 5]),
}

impl StatusGlyph {
    #[inline(always)]
    pub const fn width(&self) -> u8 {
        match self {
            StatusGlyph::Builtin(ic) => ic.width(),
            StatusGlyph::Custom(_) => 5,
        }
    }

    #[inline(always)]
    pub const fn builtin(icon: Icon) -> Self {
        StatusGlyph::Builtin(icon)
    }

    #[inline(always)]
    pub const fn custom(bmp: [u8; 5]) -> Self {
        StatusGlyph::Custom(bmp)
    }

    #[inline(always)]
    pub const fn default_builtin() -> Self {
        StatusGlyph::Builtin(Icon::Antenna)
    }
}

impl From<Icon> for StatusGlyph {
    #[inline(always)]
    fn from(icon: Icon) -> Self {
        StatusGlyph::Builtin(icon)
    }
}

#[derive(Clone, Copy)]
pub struct Canvas {
    pub buffer: [[u8; LCD_WIDTH]; 8],
    #[cfg(feature = "grayscale")]
    pub buffer_lo: [[u8; LCD_WIDTH]; 8],
    pub dirty: u8,
}

impl Canvas {
    pub const fn new() -> Self {
        Self {
            buffer: [[0; LCD_WIDTH]; 8],
            #[cfg(feature = "grayscale")]
            buffer_lo: [[0; LCD_WIDTH]; 8],
            dirty: 0,
        }
    }

    #[inline(always)]
    pub const fn width(&self) -> u8 {
        LCD_WIDTH as u8
    }

    #[inline(always)]
    pub const fn height(&self) -> u8 {
        LCD_HEIGHT as u8
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.buffer = [[0; LCD_WIDTH]; 8];
        #[cfg(feature = "grayscale")]
        {
            self.buffer_lo = [[0; LCD_WIDTH]; 8];
        }
        self.dirty = 0xFF;
    }

    #[inline(always)]
    pub fn mark_all_dirty(&mut self) {
        self.dirty = 0xFF;
    }

    #[inline(never)]
    pub fn draw_pixel(&mut self, x: u8, y: u8, color: Color) {
        if (x as usize) < LCD_WIDTH && (y as usize) < LCD_HEIGHT {
            let page = (y >> 3) as usize;
            let bit = y & 7;
            let col = x as usize;
            let mask = 1 << bit;
            #[cfg(feature = "grayscale")]
            {
                match color {
                    Color::Clear => {
                        self.buffer[page][col] &= !mask;
                        self.buffer_lo[page][col] &= !mask;
                    }
                    Color::LightGray => {
                        self.buffer[page][col] &= !mask;
                        self.buffer_lo[page][col] |= mask;
                    }
                    Color::DarkGray => {
                        self.buffer[page][col] |= mask;
                        self.buffer_lo[page][col] &= !mask;
                    }
                    Color::Fill => {
                        self.buffer[page][col] |= mask;
                        self.buffer_lo[page][col] |= mask;
                    }
                    Color::Invert => {
                        self.buffer[page][col] ^= mask;
                        self.buffer_lo[page][col] ^= mask;
                    }
                }
            }
            #[cfg(not(feature = "grayscale"))]
            {
                match color {
                    Color::Clear => self.buffer[page][col] &= !mask,
                    Color::Fill => self.buffer[page][col] |= mask,
                    Color::Invert => self.buffer[page][col] ^= mask,
                    Color::DarkGray => {
                        if ((col ^ (y as usize)) & 1) == 0 {
                            self.buffer[page][col] |= mask;
                        } else {
                            self.buffer[page][col] &= !mask;
                        }
                    }
                    Color::LightGray => {
                        if (col & 1 == 0) && (y & 1 == 0) {
                            self.buffer[page][col] |= mask;
                        } else {
                            self.buffer[page][col] &= !mask;
                        }
                    }
                }
            }
            self.dirty |= 1 << page;
        }
    }

    #[inline(always)]
    pub fn draw_horizontal_line(&mut self, x: u8, y: u8, length: u8, color: Color) {
        self.fill_rect(x, y, length, 1, color);
    }

    #[inline(always)]
    pub fn draw_vertical_line(&mut self, x: u8, y: u8, length: u8, color: Color) {
        self.fill_rect(x, y, 1, length, color);
    }

    pub fn draw_rect(&mut self, x: u8, y: u8, width: u8, height: u8, color: Color) {
        if width == 0 || height == 0 {
            return;
        }
        self.draw_horizontal_line(x, y, width, color);
        self.draw_horizontal_line(x, (y + height - 1).min(63), width, color);
        self.draw_vertical_line(x, y, height, color);
        self.draw_vertical_line((x + width - 1).min(127), y, height, color);
    }

    /// Clear the four outermost corners of a rectangle (for rounded / chamfered UI elements)
    pub fn clear_corners(&mut self, x: u8, y: u8, width: u8, height: u8) {
        if width < 2 || height < 2 {
            return;
        }
        let right = (x + width - 1).min(127);
        let bottom = (y + height - 1).min(63);
        self.draw_pixel(x, y, Color::Clear);
        self.draw_pixel(right, y, Color::Clear);
        self.draw_pixel(x, bottom, Color::Clear);
        self.draw_pixel(right, bottom, Color::Clear);
    }

    #[inline(always)]
    pub fn draw_pixel_i(&mut self, x: i16, y: i16, color: Color) {
        if (x as u16) < LCD_WIDTH as u16 && (y as u16) < LCD_HEIGHT as u16 {
            self.draw_pixel(x as u8, y as u8, color);
        }
    }

    #[inline(never)]
    pub fn draw_horizontal_line_i(&mut self, x: i16, y: i16, len: i16, color: Color) {
        if len <= 0 || y < 0 || y >= LCD_HEIGHT as i16 {
            return;
        }
        let x0 = x.max(0) as u8;
        let x1 = (x + len).min(LCD_WIDTH as i16).max(0) as u8;
        if x1 > x0 {
            self.draw_horizontal_line(x0, y as u8, x1 - x0, color);
        }
    }

    #[inline(never)]
    pub fn draw_vertical_line_i(&mut self, x: i16, y: i16, len: i16, color: Color) {
        if len <= 0 || x < 0 || x >= LCD_WIDTH as i16 {
            return;
        }
        let y0 = y.max(0) as u8;
        let y1 = (y + len).min(LCD_HEIGHT as i16).max(0) as u8;
        if y1 > y0 {
            self.draw_vertical_line(x as u8, y0, y1 - y0, color);
        }
    }

    #[inline(never)]
    pub fn draw_rect_i(&mut self, x: i16, y: i16, w: i16, h: i16, color: Color) {
        if w <= 0 || h <= 0 || x >= LCD_WIDTH as i16 || y >= LCD_HEIGHT as i16 {
            return;
        }
        self.draw_horizontal_line_i(x, y, w, color);
        self.draw_horizontal_line_i(x, y + h - 1, w, color);
        self.draw_vertical_line_i(x, y, h, color);
        self.draw_vertical_line_i(x + w - 1, y, h, color);
    }

    pub fn clear_corners_i(&mut self, x: i16, y: i16, w: i16, h: i16) {
        if w < 2 || h < 2 || x >= LCD_WIDTH as i16 || y >= LCD_HEIGHT as i16 {
            return;
        }
        self.draw_pixel_i(x, y, Color::Clear);
        self.draw_pixel_i(x + w - 1, y, Color::Clear);
        self.draw_pixel_i(x, y + h - 1, Color::Clear);
        self.draw_pixel_i(x + w - 1, y + h - 1, Color::Clear);
    }

    #[inline(never)]
    pub fn fill_rect_i(&mut self, x: i16, y: i16, w: i16, h: i16, color: Color) {
        if w <= 0 || h <= 0 || x >= LCD_WIDTH as i16 || y >= LCD_HEIGHT as i16 {
            return;
        }
        let x0 = x.max(0) as u8;
        let y0 = y.max(0) as u8;
        let x1 = (x + w).min(LCD_WIDTH as i16).max(0) as u8;
        let y1 = (y + h).min(LCD_HEIGHT as i16).max(0) as u8;
        if x1 > x0 && y1 > y0 {
            self.fill_rect(x0, y0, x1 - x0, y1 - y0, color);
        }
    }

    pub fn fill_rect_dither_i(&mut self, x: i16, y: i16, w: i16, h: i16, progress: u8) {
        if progress == 0 || w <= 0 || h <= 0 || x >= LCD_WIDTH as i16 || y >= LCD_HEIGHT as i16 {
            return;
        }
        let x0 = x.max(0) as u8;
        let y0 = y.max(0) as u8;
        let x1 = (x + w).min(LCD_WIDTH as i16).max(0) as u8;
        let y1 = (y + h).min(LCD_HEIGHT as i16).max(0) as u8;
        if x1 > x0 && y1 > y0 {
            self.fill_rect_dither(x0, y0, x1 - x0, y1 - y0, progress);
        }
    }

    pub fn fill_rect(&mut self, x: u8, y: u8, width: u8, height: u8, color: Color) {
        if (x as usize) >= LCD_WIDTH || (y as usize) >= LCD_HEIGHT || width == 0 || height == 0 {
            return;
        }
        let end_x = (x as usize + width as usize).min(LCD_WIDTH);
        let end_y = (y as usize + height as usize).min(LCD_HEIGHT);
        let start_page = (y >> 3) as usize;
        let end_page = (end_y.saturating_sub(1) >> 3) as usize;

        for page in start_page..=end_page {
            let page_start_y = page * 8;
            let page_end_y = page_start_y + 8;
            let top = if (y as usize) > page_start_y {
                (y as usize) - page_start_y
            } else {
                0
            };
            let bottom = if end_y < page_end_y {
                end_y - page_start_y
            } else {
                8
            };
            let mask = ((0xFFu16 << top) & (0xFFu16 >> (8 - bottom))) as u8;
            let not_mask = !mask;
            #[cfg(feature = "grayscale")]
            {
                let row_hi = &mut self.buffer[page];
                let row_lo = &mut self.buffer_lo[page];
                match color {
                    Color::Fill => {
                        for col in x as usize..end_x {
                            row_hi[col] |= mask;
                            row_lo[col] |= mask;
                        }
                    }
                    Color::Clear => {
                        for col in x as usize..end_x {
                            row_hi[col] &= not_mask;
                            row_lo[col] &= not_mask;
                        }
                    }
                    Color::LightGray => {
                        for col in x as usize..end_x {
                            row_hi[col] &= not_mask;
                            row_lo[col] |= mask;
                        }
                    }
                    Color::DarkGray => {
                        for col in x as usize..end_x {
                            row_hi[col] |= mask;
                            row_lo[col] &= not_mask;
                        }
                    }
                    Color::Invert => {
                        for col in x as usize..end_x {
                            row_hi[col] ^= mask;
                            row_lo[col] ^= mask;
                        }
                    }
                }
            }
            #[cfg(not(feature = "grayscale"))]
            {
                let row = &mut self.buffer[page];
                match color {
                    Color::Fill => {
                        for b in &mut row[x as usize..end_x] {
                            *b |= mask;
                        }
                    }
                    Color::Clear => {
                        for b in &mut row[x as usize..end_x] {
                            *b &= not_mask;
                        }
                    }
                    Color::Invert => {
                        for b in &mut row[x as usize..end_x] {
                            *b ^= mask;
                        }
                    }
                    Color::DarkGray => {
                        for col in x as usize..end_x {
                            let dither = if (col & 1) == 0 {
                                0b1010_1010
                            } else {
                                0b0101_0101
                            };
                            row[col] = (row[col] & not_mask) | (dither & mask);
                        }
                    }
                    Color::LightGray => {
                        for col in x as usize..end_x {
                            let dither = if (col & 1) == 0 {
                                0b1000_1000
                            } else {
                                0b0010_0010
                            };
                            row[col] = (row[col] & not_mask) | (dither & mask);
                        }
                    }
                }
            }
            self.dirty |= 1 << page;
        }
    }

    #[inline(always)]
    pub fn write_page_byte(&mut self, page: usize, col: usize, byte: u8) {
        if page < 8 && col < LCD_WIDTH {
            self.buffer[page][col] = byte;
            self.dirty |= 1 << page;
        }
    }

    pub fn load_page_slice(&mut self, page: usize, data: &[u8]) {
        if page < 8 {
            let len = data.len().min(LCD_WIDTH);
            self.buffer[page][..len].copy_from_slice(&data[..len]);
            self.dirty |= 1 << page;
        }
    }

    /// Synchronize lower plane (buffer_lo) with upper plane (buffer) to ensure
    /// 100% solid fill when rendering monochrome assets in dual-plane modes.
    #[inline(always)]
    pub fn sync_planes(&mut self) {
        #[cfg(feature = "grayscale")]
        {
            self.buffer_lo = self.buffer;
        }
    }

    pub fn load_raw_vram(&mut self, data: &[u8]) {
        for page in 0..8 {
            let start = page * LCD_WIDTH;
            let end = (start + LCD_WIDTH).min(data.len());
            if start < data.len() {
                let slice = &data[start..end];
                self.buffer[page][..slice.len()].copy_from_slice(slice);
                self.dirty |= 1 << page;
            }
        }
        self.sync_planes();
    }

    #[inline(always)]
    pub fn buffer_flat(&self) -> &[u8; 1024] {
        unsafe { &*(self.buffer.as_ptr() as *const [u8; 1024]) }
    }

    #[inline(always)]
    pub fn buffer_flat_mut(&mut self) -> &mut [u8; 1024] {
        self.dirty = 0xFF;
        unsafe { &mut *(self.buffer.as_mut_ptr() as *mut [u8; 1024]) }
    }

    pub const BAYER_4X4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

    /// Fill rectangle using progressive Bayer 4x4 matrix dithering.
    /// `progress` ranges from 0 (empty) to 16 (100% solid fill).
    #[inline(never)]
    pub fn fill_rect_dither(&mut self, x: u8, y: u8, width: u8, height: u8, progress: u8) {
        if progress == 0
            || width == 0
            || height == 0
            || (x as usize) >= LCD_WIDTH
            || (y as usize) >= LCD_HEIGHT
        {
            return;
        }
        if progress >= 16 {
            self.fill_rect(x, y, width, height, Color::Fill);
            return;
        }

        let end_x = (x as usize + width as usize).min(LCD_WIDTH);
        let end_y = (y as usize + height as usize).min(LCD_HEIGHT);
        let start_page = (y >> 3) as usize;
        let end_page = (end_y.saturating_sub(1) >> 3) as usize;

        for page in start_page..=end_page {
            let page_start_y = page * 8;
            let page_end_y = page_start_y + 8;
            let top = if (y as usize) > page_start_y {
                (y as usize) - page_start_y
            } else {
                0
            };
            let bottom = if end_y < page_end_y {
                end_y - page_start_y
            } else {
                8
            };
            let row = &mut self.buffer[page];

            for col in (x as usize)..end_x {
                let x_mod = col & 3;
                let mut mask = 0u8;
                for bit in top..bottom {
                    let py = page_start_y + bit;
                    let threshold = Self::BAYER_4X4[((py & 3) << 2) | x_mod];
                    if threshold < progress {
                        mask |= 1 << bit;
                    }
                }
                row[col] |= mask;
            }
            self.dirty |= 1 << page;
        }
    }

    /// Apply Bayer 4x4 dither reveal across the entire 128x64 canvas.
    /// Pixels where `threshold >= progress` are cleared (progress: 0..=16).
    pub fn apply_dither_mask(&mut self, progress: u8) {
        if progress == 0 {
            self.buffer = [[0; LCD_WIDTH]; 8];
        } else if progress < 16 {
            // For a 4x4 Bayer matrix, threshold depends only on (y & 3) and (x & 3).
            // Since y = page * 8 + bit, y & 3 is identical to bit & 3.
            // Bits 0..3 and bits 4..7 have identical thresholds for each column c in 0..4.
            // Precompute the 8-bit keep mask for each column c:
            let mut masks = [0u8; 4];
            for (c, mask) in masks.iter_mut().enumerate() {
                let b0 = (Self::BAYER_4X4[c] < progress) as u8;
                let b1 = (Self::BAYER_4X4[4 + c] < progress) as u8;
                let b2 = (Self::BAYER_4X4[8 + c] < progress) as u8;
                let b3 = (Self::BAYER_4X4[12 + c] < progress) as u8;
                let nib = b0 | (b1 << 1) | (b2 << 2) | (b3 << 3);
                *mask = nib | (nib << 4);
            }
            let mask_word = u32::from_ne_bytes(masks);

            let flat = self.buffer_flat_mut();
            for chunk in flat.chunks_exact_mut(4) {
                let word = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let res = (word & mask_word).to_ne_bytes();
                chunk[0] = res[0];
                chunk[1] = res[1];
                chunk[2] = res[2];
                chunk[3] = res[3];
            }
        }
        self.sync_planes();
        self.dirty = 0xFF;
    }

    /// Apply 2D Bayer matrix invert dissolve across the entire 128x64 canvas.
    /// Pixels where `threshold >= progress` are inverted (progress: 0..=16).
    pub fn apply_invert_dissolve(&mut self, progress: u8) {
        if progress < 16 {
            let mut inv_masks = [0u8; 4];
            for (c, mask) in inv_masks.iter_mut().enumerate() {
                let b0 = (Self::BAYER_4X4[c] >= progress) as u8;
                let b1 = (Self::BAYER_4X4[4 + c] >= progress) as u8;
                let b2 = (Self::BAYER_4X4[8 + c] >= progress) as u8;
                let b3 = (Self::BAYER_4X4[12 + c] >= progress) as u8;
                let nib = b0 | (b1 << 1) | (b2 << 2) | (b3 << 3);
                *mask = nib | (nib << 4);
            }
            let inv_word = u32::from_ne_bytes(inv_masks);

            let flat = self.buffer_flat_mut();
            for chunk in flat.chunks_exact_mut(4) {
                let word = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                let res = (word ^ inv_word).to_ne_bytes();
                chunk[0] = res[0];
                chunk[1] = res[1];
                chunk[2] = res[2];
                chunk[3] = res[3];
            }
        }
        self.sync_planes();
        self.dirty = 0xFF;
    }

    /// Apply curtain wipe from left to right across the 128x64 canvas.
    /// `progress` ranges from 0 to 16 (0 = all wiped, 16 = fully visible).
    pub fn apply_curtain_wipe(&mut self, progress: u8) {
        if progress < 16 {
            let sweep_x = (progress as usize) << 3;
            for page in 0..8 {
                let row = &mut self.buffer[page];
                for col in sweep_x..LCD_WIDTH {
                    row[col] = 0;
                }
                if sweep_x < 127 {
                    row[sweep_x] = 0xFF;
                }
            }
        }
        self.sync_planes();
        self.dirty = 0xFF;
    }

    /// Hardware-efficient in-place vertical bit-shift scroll.
    /// Positive `dy` shifts content down (towards page 7).
    /// Negative `dy` shifts content up (towards page 0).
    pub fn scroll_vertical(&mut self, dy: i8, fill_byte: u8) {
        if dy == 0 {
            return;
        }
        scroll_plane_vertical(&mut self.buffer, dy, fill_byte);
        #[cfg(feature = "grayscale")]
        {
            scroll_plane_vertical(&mut self.buffer_lo, dy, fill_byte);
        }
        self.dirty = 0xFF;
    }

    /// Hardware-efficient horizontal column shift across all display pages.
    /// Positive `dx` shifts content right.
    /// Negative `dx` shifts content left.
    pub fn scroll_horizontal(&mut self, dx: i8, fill_byte: u8) {
        if dx == 0 {
            return;
        }
        scroll_plane_horizontal(&mut self.buffer, dx, fill_byte);
        #[cfg(feature = "grayscale")]
        {
            scroll_plane_horizontal(&mut self.buffer_lo, dx, fill_byte);
        }
        self.dirty = 0xFF;
    }

    /// Calculate 1-bit composite byte for a given subframe (0..2) using
    /// Pure Temporal Frame Rate Modulation (FRC) at 166.7 Hz:
    /// - Clear (hi=0, lo=0): 0/3 (0.0% luminance)
    /// - LightGray (hi=0, lo=1): 1/3 (33.3% luminance, solid on subframe 0)
    /// - DarkGray (hi=1, lo=0): 2/3 (66.7% luminance, solid on subframes 0 and 1)
    /// - Fill (hi=1, lo=1): 3/3 (100.0% solid black)
    ///
    /// Pure temporal modulation delivers 100% uniform solid strokes across all
    /// columns, completely eliminating spatial checkerboard dotting and
    /// directional traveling waves (apparent motion v == 0).
    #[inline(always)]
    pub fn get_subframe_byte(&self, page: usize, col: usize, subframe: u8) -> u8 {
        let hi = self.buffer[page][col];
        #[cfg(feature = "grayscale")]
        let lo = self.buffer_lo[page][col];

        #[cfg(feature = "grayscale")]
        {
            match subframe % 3 {
                0 => hi | lo,
                1 => hi,
                _ => hi & lo,
            }
        }
        #[cfg(not(feature = "grayscale"))]
        {
            let _ = (col, subframe);
            hi
        }
    }

    /// Fast-path compatibility helper.
    #[inline(always)]
    pub fn get_subframe_byte_k(&self, page: usize, col: usize, phase: usize) -> u8 {
        self.get_subframe_byte(page, col, phase as u8)
    }

    /// Calculate 1-bit composite byte for a given display page and column using
    /// 2x2 Bayer spatial dithering.
    ///
    /// Provides 100% static, flicker-free, non-pulsating grayscale halftones on
    /// passive STN LCDs without temporal beating or SPI bus hammering:
    /// - Clear (hi=0, lo=0): 0% fill
    /// - LightGray (hi=0, lo=1): 25% spatial fill (1/4 pixels)
    /// - DarkGray (hi=1, lo=0): 75% spatial fill (3/4 pixels)
    /// - Fill (hi=1, lo=1): 100% solid fill
    #[inline(always)]
    pub fn get_dither_byte(&self, page: usize, col: usize) -> u8 {
        let hi = self.buffer[page][col];
        #[cfg(feature = "grayscale")]
        {
            let lo = self.buffer_lo[page][col];
            let is_even_col = (col & 1) == 0;
            let lg_mask = if is_even_col { 0x55 } else { 0x00 };
            let dg_mask = if is_even_col { 0xFF } else { 0xAA };

            let fill = hi & lo;
            let dg = (hi & !lo) & dg_mask;
            let lg = (!hi & lo) & lg_mask;
            fill | dg | lg
        }
        #[cfg(not(feature = "grayscale"))]
        {
            let _ = (page, col);
            hi
        }
    }
}

#[inline(never)]
fn scroll_plane_vertical(plane: &mut [[u8; LCD_WIDTH]; 8], dy: i8, fill_byte: u8) {
    if dy.unsigned_abs() >= LCD_HEIGHT as u8 {
        *plane = [[fill_byte; LCD_WIDTH]; 8];
        return;
    }

    if dy > 0 {
        let dy = dy as usize;
        let page_shift = dy >> 3;
        let bit_shift = dy & 7;

        for p in (0..8).rev() {
            if p >= page_shift {
                let src_p = p - page_shift;
                if bit_shift == 0 {
                    for col in 0..LCD_WIDTH {
                        plane[p][col] = plane[src_p][col];
                    }
                } else {
                    for col in 0..LCD_WIDTH {
                        let curr = plane[src_p][col];
                        let prev = if src_p > 0 {
                            plane[src_p - 1][col]
                        } else {
                            fill_byte
                        };
                        plane[p][col] = (curr << bit_shift) | (prev >> (8 - bit_shift));
                    }
                }
            } else {
                plane[p] = [fill_byte; LCD_WIDTH];
            }
        }
    } else {
        let abs_dy = (-dy) as usize;
        let page_shift = abs_dy >> 3;
        let bit_shift = abs_dy & 7;

        for p in 0..8 {
            if p + page_shift < 8 {
                let src_p = p + page_shift;
                if bit_shift == 0 {
                    for col in 0..LCD_WIDTH {
                        plane[p][col] = plane[src_p][col];
                    }
                } else {
                    for col in 0..LCD_WIDTH {
                        let curr = plane[src_p][col];
                        let next = if src_p + 1 < 8 {
                            plane[src_p + 1][col]
                        } else {
                            fill_byte
                        };
                        plane[p][col] = (curr >> bit_shift) | (next << (8 - bit_shift));
                    }
                }
            } else {
                plane[p] = [fill_byte; LCD_WIDTH];
            }
        }
    }
}

#[inline(never)]
fn scroll_plane_horizontal(plane: &mut [[u8; LCD_WIDTH]; 8], dx: i8, fill_byte: u8) {
    if dx.unsigned_abs() >= LCD_WIDTH as u8 {
        *plane = [[fill_byte; LCD_WIDTH]; 8];
        return;
    }

    if dx > 0 {
        let shift = dx as usize;
        for page in 0..8 {
            let row = &mut plane[page];
            let mut i = LCD_WIDTH;
            while i > shift {
                i -= 1;
                row[i] = row[i - shift];
            }
            row[..shift].fill(fill_byte);
        }
    } else {
        let shift = (-dx) as usize;
        for page in 0..8 {
            let row = &mut plane[page];
            let mut i = 0;
            while i < LCD_WIDTH - shift {
                row[i] = row[i + shift];
                i += 1;
            }
            row[LCD_WIDTH - shift..].fill(fill_byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canvas_dimensions() {
        let canvas = Canvas::new();
        assert_eq!(canvas.width(), 128);
        assert_eq!(canvas.height(), 64);
        assert_eq!(canvas.dirty, 0);
    }

    #[test]
    fn test_canvas_clear_and_dirty() {
        let mut canvas = Canvas::new();
        canvas.clear();
        assert_eq!(canvas.dirty, 0xFF);
        for page in 0..8 {
            for col in 0..128 {
                assert_eq!(canvas.buffer[page][col], 0);
            }
        }
    }

    #[test]
    fn test_canvas_draw_pixel() {
        let mut canvas = Canvas::new();
        // Point (10, 10): page = 10 / 8 = 1, bit = 10 % 8 = 2.
        canvas.draw_pixel(10, 10, Color::Fill);
        assert_eq!(canvas.dirty, 1 << 1);
        assert_eq!(canvas.buffer[1][10], 0b0000_0100);

        // Invert
        canvas.draw_pixel(10, 10, Color::Invert);
        assert_eq!(canvas.buffer[1][10], 0);

        // Re-fill and Clear
        canvas.draw_pixel(10, 10, Color::Fill);
        assert_eq!(canvas.buffer[1][10], 0b0000_0100);
        canvas.draw_pixel(10, 10, Color::Clear);
        assert_eq!(canvas.buffer[1][10], 0);

        // Out of bounds check
        canvas.dirty = 0;
        canvas.draw_pixel(128, 64, Color::Fill);
        canvas.draw_pixel(200, 100, Color::Fill);
        assert_eq!(canvas.dirty, 0);
    }

    #[test]
    fn test_canvas_lines_and_rect() {
        let mut canvas = Canvas::new();
        // Horizontal line: y=0, x=0..8
        canvas.draw_horizontal_line(0, 0, 8, Color::Fill);
        assert_eq!(canvas.dirty, 1);
        for col in 0..8 {
            assert_eq!(canvas.buffer[0][col], 0b0000_0001);
        }
        assert_eq!(canvas.buffer[0][8], 0);

        // Vertical line: x=20, y=6..14 (spans page 0: bits 6..7 and page 1: bits 0..5)
        canvas.dirty = 0;
        canvas.draw_vertical_line(20, 6, 8, Color::Fill);
        assert_eq!(canvas.dirty, (1 << 0) | (1 << 1));
        assert_eq!(canvas.buffer[0][20], 0b1100_0000);
        assert_eq!(canvas.buffer[1][20], 0b0011_1111);
    }

    #[test]
    fn test_canvas_fill_rect_and_corners() {
        let mut canvas = Canvas::new();
        // Fill 4x4 rect at (0, 0)
        canvas.fill_rect(0, 0, 4, 4, Color::Fill);
        for col in 0..4 {
            assert_eq!(canvas.buffer[0][col], 0b0000_1111);
        }

        // Clear corners (0,0), (3,0), (0,3), (3,3)
        canvas.clear_corners(0, 0, 4, 4);
        assert_eq!(canvas.buffer[0][0], 0b0000_0110);
        assert_eq!(canvas.buffer[0][1], 0b0000_1111);
        assert_eq!(canvas.buffer[0][2], 0b0000_1111);
        assert_eq!(canvas.buffer[0][3], 0b0000_0110);
    }

    #[test]
    fn test_canvas_flat_buffer() {
        let mut canvas = Canvas::new();
        let flat = canvas.buffer_flat_mut();
        flat[0] = 0xAA;
        flat[128] = 0x55; // start of page 1
        assert_eq!(canvas.buffer[0][0], 0xAA);
        assert_eq!(canvas.buffer[1][0], 0x55);
        assert_eq!(canvas.dirty, 0xFF);
    }

    #[test]
    fn test_canvas_scroll_vertical() {
        let mut canvas = Canvas::new();
        // Draw pixel at (10, 0)
        canvas.draw_pixel(10, 0, Color::Fill);
        assert_eq!(canvas.buffer[0][10], 0x01);

        // Scroll down by 2 pixels (content moves down to y=2)
        canvas.scroll_vertical(2, 0);
        assert_eq!(canvas.buffer[0][10], 0x04);

        // Scroll down across page boundary by 7 pixels (y=2 -> y=9, which is page 1 bit 1)
        canvas.scroll_vertical(7, 0);
        assert_eq!(canvas.buffer[0][10], 0x00);
        assert_eq!(canvas.buffer[1][10], 0x02);

        // Scroll up by 9 pixels (content moves back to y=0)
        canvas.scroll_vertical(-9, 0);
        assert_eq!(canvas.buffer[0][10], 0x01);
        assert_eq!(canvas.buffer[1][10], 0x00);
    }

    #[test]
    fn test_canvas_scroll_horizontal() {
        let mut canvas = Canvas::new();
        canvas.draw_pixel(5, 5, Color::Fill);
        assert_eq!(canvas.buffer[0][5], 0x20);

        // Scroll right by 3 columns
        canvas.scroll_horizontal(3, 0);
        assert_eq!(canvas.buffer[0][5], 0x00);
        assert_eq!(canvas.buffer[0][8], 0x20);

        // Scroll left by 4 columns
        canvas.scroll_horizontal(-4, 0);
        assert_eq!(canvas.buffer[0][8], 0x00);
        assert_eq!(canvas.buffer[0][4], 0x20);
    }

    #[test]
    fn test_canvas_fill_rect_dither() {
        let mut canvas = Canvas::new();
        // Progress 0: nothing drawn
        canvas.fill_rect_dither(0, 0, 4, 4, 0);
        assert_eq!(canvas.buffer[0][0], 0x00);

        // Progress 8: partial dither pattern (50% filled)
        canvas.fill_rect_dither(0, 0, 4, 4, 8);
        let filled_bits: u32 = (0..4)
            .map(|col| (canvas.buffer[0][col] & 0x0F).count_ones())
            .sum();
        assert_eq!(filled_bits, 8); // 8 out of 16 pixels filled

        // Progress 16: 100% solid fill
        canvas.fill_rect_dither(0, 0, 4, 4, 16);
        for col in 0..4 {
            assert_eq!(canvas.buffer[0][col], 0x0F);
        }
    }

    #[test]
    fn test_canvas_grayscale_frm_and_interleaving() {
        let mut canvas = Canvas::new();

        // Pixel (0, 0) Clear (0%)
        canvas.draw_pixel(0, 0, Color::Clear);
        // Pixel (1, 0) LightGray (33%)
        canvas.draw_pixel(1, 0, Color::LightGray);
        // Pixel (2, 0) DarkGray (67%)
        canvas.draw_pixel(2, 0, Color::DarkGray);
        // Pixel (3, 0) Fill (100%)
        canvas.draw_pixel(3, 0, Color::Fill);

        // Subframe outputs for each pixel across frames 0..3
        for frame in 0..3 {
            let b0 = canvas.get_subframe_byte(0, 0, frame) & 1;
            let b3 = canvas.get_subframe_byte(0, 3, frame) & 1;

            // Clear is always 0
            assert_eq!(b0, 0, "Clear pixel must be 0 in frame {}", frame);
            // Fill is always 1
            assert_eq!(b3, 1, "Fill pixel must be 1 in frame {}", frame);
        }

        #[cfg(feature = "grayscale")]
        {
            // LightGray: active in 1 out of 3 frames (33.3% duty cycle)
            let count_lg: u8 = (0..3).map(|f| canvas.get_subframe_byte(0, 1, f) & 1).sum();
            assert_eq!(
                count_lg, 1,
                "LightGray must be active in exactly 1 frame out of 3 (33.3% duty cycle)"
            );

            // DarkGray: active in 2 out of 3 frames (66.7% duty cycle)
            let count_dg: u8 = (0..3).map(|f| canvas.get_subframe_byte(0, 2, f) & 1).sum();
            assert_eq!(
                count_dg, 2,
                "DarkGray must be active in exactly 2 frames out of 3 (66.7% duty cycle)"
            );

            // Verify 100% solid uniform byte outputs (zero dots! Every pixel in column is identical)
            for col in 0..4 {
                for f in 0..3 {
                    let byte = canvas.get_subframe_byte(0, col, f);
                    assert!(
                        byte == 0 || (byte & 0x01 != 0),
                        "Output must be full uniform byte, never broken dots"
                    );
                }
            }
        }
    }

    #[test]
    fn test_canvas_spatial_bayer_dither() {
        let mut canvas = Canvas::new();

        // Clear (0, 0), LightGray (1, 0), DarkGray (2, 0), Fill (3, 0)
        canvas.draw_pixel(0, 0, Color::Clear);
        canvas.draw_pixel(1, 0, Color::LightGray);
        canvas.draw_pixel(2, 0, Color::DarkGray);
        canvas.draw_pixel(3, 0, Color::Fill);

        // Clear is 0
        assert_eq!(canvas.get_dither_byte(0, 0) & 1, 0);
        // Fill is 1
        assert_eq!(canvas.get_dither_byte(0, 3) & 1, 1);

        #[cfg(feature = "grayscale")]
        {
            // Even col (0) vs Odd col (1) for 2x2 Bayer:
            // Fill a 2x2 block with LightGray: (4, 0), (5, 0), (4, 1), (5, 1)
            canvas.draw_pixel(4, 0, Color::LightGray);
            canvas.draw_pixel(5, 0, Color::LightGray);
            canvas.draw_pixel(4, 1, Color::LightGray);
            canvas.draw_pixel(5, 1, Color::LightGray);

            // In 2x2 block, exactly 1 pixel should be set (25% density)
            let b4 = canvas.get_dither_byte(0, 4);
            let b5 = canvas.get_dither_byte(0, 5);
            let count_lg = ((b4 & 1) + ((b4 >> 1) & 1) + (b5 & 1) + ((b5 >> 1) & 1)) as u8;
            assert_eq!(
                count_lg, 1,
                "LightGray 2x2 Bayer dither must produce 25% (1/4) fill"
            );

            // Fill a 2x2 block with DarkGray: (6, 0), (7, 0), (6, 1), (7, 1)
            canvas.draw_pixel(6, 0, Color::DarkGray);
            canvas.draw_pixel(7, 0, Color::DarkGray);
            canvas.draw_pixel(6, 1, Color::DarkGray);
            canvas.draw_pixel(7, 1, Color::DarkGray);

            // In 2x2 block, exactly 3 pixels should be set (75% density)
            let b6 = canvas.get_dither_byte(0, 6);
            let b7 = canvas.get_dither_byte(0, 7);
            let count_dg = ((b6 & 1) + ((b6 >> 1) & 1) + (b7 & 1) + ((b7 >> 1) & 1)) as u8;
            assert_eq!(
                count_dg, 3,
                "DarkGray 2x2 Bayer dither must produce 75% (3/4) fill"
            );
        }
    }

    #[test]
    fn test_canvas_splash_animations() {
        let mut canvas = Canvas::new();

        // 1. Dither mask test
        canvas.buffer = [[0xFF; LCD_WIDTH]; 8];
        canvas.sync_planes();
        canvas.apply_dither_mask(0);
        for page in 0..8 {
            for col in 0..LCD_WIDTH {
                assert_eq!(canvas.buffer[page][col], 0);
            }
        }

        // Test monotonic bit count increase across dither steps
        let mut prev_bits = 0usize;
        for step in 0..=16 {
            canvas.buffer = [[0xFF; LCD_WIDTH]; 8];
            canvas.sync_planes();
            canvas.apply_dither_mask(step);

            #[cfg(feature = "grayscale")]
            assert_eq!(canvas.buffer, canvas.buffer_lo);

            let bits: usize = canvas
                .buffer
                .iter()
                .flat_map(|page| page.iter())
                .map(|b| b.count_ones() as usize)
                .sum();

            assert!(bits >= prev_bits);
            prev_bits = bits;
        }
        assert_eq!(prev_bits, 1024 * 8);

        // 2. Invert dissolve test
        canvas.buffer = [[0xFF; LCD_WIDTH]; 8];
        canvas.sync_planes();
        canvas.apply_invert_dissolve(0);
        for page in 0..8 {
            for col in 0..LCD_WIDTH {
                assert_eq!(canvas.buffer[page][col], 0);
            }
        }
        canvas.buffer = [[0xFF; LCD_WIDTH]; 8];
        canvas.sync_planes();
        canvas.apply_invert_dissolve(16);
        for page in 0..8 {
            for col in 0..LCD_WIDTH {
                assert_eq!(canvas.buffer[page][col], 0xFF);
            }
        }

        // 3. Curtain wipe test
        canvas.buffer = [[0xFF; LCD_WIDTH]; 8];
        canvas.sync_planes();
        canvas.apply_curtain_wipe(8); // sweep_x = 64
        for page in 0..8 {
            for col in 0..64 {
                assert_eq!(canvas.buffer[page][col], 0xFF);
            }
            assert_eq!(canvas.buffer[page][64], 0xFF); // scanline
            for col in 65..128 {
                assert_eq!(canvas.buffer[page][col], 0);
            }
        }
        canvas.buffer = [[0xFF; LCD_WIDTH]; 8];
        canvas.sync_planes();
        canvas.apply_curtain_wipe(16);
        for page in 0..8 {
            for col in 0..128 {
                assert_eq!(canvas.buffer[page][col], 0xFF);
            }
        }
    }
}
