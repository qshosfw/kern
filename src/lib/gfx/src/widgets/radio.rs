use super::spectrum::{draw_big_digit, draw_small_digit};
use crate::text;
use crate::{Canvas, Color};

/// VFO Channel State Information for Radio Screens
#[derive(Debug, Clone, Copy)]
pub struct VfoInfo<'a> {
    pub vfo_num: u8,              // 1 for VFO A, 2 for VFO B
    pub channel_num: Option<u16>, // Some(channel_idx) if in MR mode
    pub channel_name: &'a str,
    pub freq_hz: u32,
    pub power: &'a str,    // "H", "M", "L"
    pub bw: &'a str,       // "W", "N"
    pub mod_type: &'a str, // "FM", "AM", "USB"
    pub rx_active: bool,
    pub tx_active: bool,
    pub selected: bool,
}

/// Render an authentic k1-fw / deltafw Dual VFO row (Badge, Frequency, Tags, Channel Name)
pub fn draw_vfo_row(
    canvas: &mut Canvas,
    y: i16, // y is the top of the VFO block, e.g., 8 for VFO A
    vfo: &VfoInfo,
) {
    let font = &crate::fonts::tomthumb::FONT;

    // 1. Inverted VFO / Channel Badge (x=0..19, y..y+7)
    let v_name = if vfo.vfo_num == 1 { "V1" } else { "V2" };
    let (badge_text, fg_color) = if vfo.selected {
        canvas.fill_rect(0, y as u8, 19, 7, Color::Fill);
        let t = match vfo.channel_num {
            Some(ch) if ch < 10 => "M0",
            Some(_) => "M",
            None => v_name,
        };
        (t, Color::Clear)
    } else {
        canvas.draw_rect(0, y as u8, 19, 7, Color::Fill);
        (v_name, Color::Fill)
    };
    text::draw_str(canvas, 2, y + 6, badge_text, font, fg_color);

    // 2. RX / TX Active Status Indicator (x=0, y+8)
    let status_txt = if vfo.tx_active {
        "TX"
    } else if vfo.rx_active {
        "RX"
    } else {
        ""
    };
    if !status_txt.is_empty() {
        text::draw_str(canvas, 2, y + 14, status_txt, font, Color::Fill);
    }

    // 3. Authentic Big Frequency (x=30)
    let mut fbuf = [0u8; 10];
    let fbytes = text::format_u32(utils::math::div10_u32(vfo.freq_hz).0, &mut fbuf).as_bytes();
    let flen = fbytes.len();
    let mut cur_x = 30i16;

    if flen >= 3 {
        let mhz_end = flen.saturating_sub(5);
        for (i, &ch) in fbytes[..flen - 2].iter().enumerate() {
            if i == mhz_end {
                canvas.fill_rect(cur_x as u8, (y + 13) as u8, 3, 2, Color::Fill);
                cur_x += 3;
            }
            draw_big_digit(canvas, cur_x, y, ch.wrapping_sub(b'0'), Color::Fill);
            cur_x += 13;
        }
        for &ch in &fbytes[flen - 2..] {
            draw_small_digit(canvas, cur_x, y + 8, ch.wrapping_sub(b'0'), Color::Fill);
            cur_x += 7;
        }
    }

    // 4. Modulation Type (top right)
    text::draw_str(canvas, 112, y + 6, vfo.mod_type, font, Color::Fill);

    // 5. Attributes Line (beneath frequency)
    let attr_y = y + 22;
    text::draw_str(canvas, 32, attr_y, vfo.power, font, Color::Fill);
    text::draw_str(canvas, 44, attr_y, vfo.bw, font, Color::Fill);
    if !vfo.channel_name.is_empty() {
        text::draw_str(canvas, 56, attr_y, vfo.channel_name, font, Color::Fill);
    }
}
