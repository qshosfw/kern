//! Core concrete Canvas framebuffer for high-efficiency monochrome rendering.

pub const LCD_WIDTH: usize = 128;
pub const LCD_HEIGHT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Clear = 0,
    Fill = 1,
    Invert = 2,
}

#[derive(Clone, Copy)]
pub struct Canvas {
    pub buffer: [[u8; LCD_WIDTH]; 8],
    pub dirty: u8,
}

impl Canvas {
    pub const fn new() -> Self {
        Self {
            buffer: [[0; LCD_WIDTH]; 8],
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
        self.dirty = 0xFF;
    }

    #[inline(always)]
    pub fn mark_all_dirty(&mut self) {
        self.dirty = 0xFF;
    }

    #[inline(always)]
    pub fn draw_pixel(&mut self, x: u8, y: u8, color: Color) {
        if (x as usize) < LCD_WIDTH && (y as usize) < LCD_HEIGHT {
            let page = (y >> 3) as usize;
            let bit = y & 7;
            let col = x as usize;
            match color {
                Color::Clear => self.buffer[page][col] &= !(1 << bit),
                Color::Fill => self.buffer[page][col] |= 1 << bit,
                Color::Invert => self.buffer[page][col] ^= 1 << bit,
            }
            self.dirty |= 1 << page;
        }
    }

    pub fn draw_horizontal_line(&mut self, x: u8, y: u8, length: u8, color: Color) {
        if (y as usize) >= LCD_HEIGHT || (x as usize) >= LCD_WIDTH || length == 0 {
            return;
        }
        let end_x = (x as usize + length as usize).min(LCD_WIDTH);
        let page = (y >> 3) as usize;
        let mask = 1u8 << (y & 7);
        let not_mask = !mask;
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
        }
        self.dirty |= 1 << page;
    }

    pub fn draw_vertical_line(&mut self, x: u8, y: u8, length: u8, color: Color) {
        if (x as usize) >= LCD_WIDTH || (y as usize) >= LCD_HEIGHT || length == 0 {
            return;
        }
        let end_y = (y as usize + length as usize).min(LCD_HEIGHT);
        let start_page = (y >> 3) as usize;
        let end_page = (end_y.saturating_sub(1) >> 3) as usize;
        let col = x as usize;

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
            match color {
                Color::Fill => self.buffer[page][col] |= mask,
                Color::Clear => self.buffer[page][col] &= !mask,
                Color::Invert => self.buffer[page][col] ^= mask,
            }
            self.dirty |= 1 << page;
        }
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
    }

    #[inline(always)]
    pub fn buffer_flat_mut(&mut self) -> &mut [u8; 1024] {
        self.dirty = 0xFF;
        unsafe { &mut *(self.buffer.as_mut_ptr() as *mut [u8; 1024]) }
    }
}
