use crate::services::power::PowerService;
use crate::services::radio::{
    ClientId, CrossBandMode, DualWatchMode, LockTier, RfClientParticipant,
};
// Main Dual VFO & Telemetry Application

use crate::SysContext;
use gfx::{Canvas, Color};
use kernel::app::{App, EventResult};
use kernel::event::{Event, KeyCode, KeyState};

pub struct VfoApp {
    repeater: gfx::gui::KeyRepeater,
}

impl VfoApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).repeater = gfx::gui::KeyRepeater::new(8, 2);
        }
    }
}

impl RfClientParticipant for VfoApp {
    #[inline(always)]
    fn client_id(&self) -> ClientId {
        ClientId::VFO_UI
    }

    #[inline(always)]
    fn default_tier(&self) -> LockTier {
        LockTier::FOREGROUND
    }
}

fn step_freq(ctx: &mut SysContext, up: bool) -> EventResult {
    let step = ctx.state.coord.active_channel().step.in_hz() as i32;
    let slot = ctx.active_vfo();
    let delta = if up { step } else { -step };
    ctx.step_vfo_frequency(slot, delta);
    EventResult::Consumed
}

#[inline(never)]
fn draw_tom_fill(canvas: &mut Canvas, x: i16, y: i16, text: &str) {
    gfx::text::draw_str(canvas, x, y, text, &gfx::fonts::tomthumb::FONT, Color::Fill);
}

#[inline(never)]
fn draw_vfo(
    canvas: &mut Canvas,
    y: i16,
    coord: &crate::services::radio::RadioCoordinator,
    slot: kernel::service::radio::VfoSlot,
    selected: bool,
) {
    let font = &gfx::fonts::tomthumb::FONT;
    let ch = coord.vfo_channel(slot);

    // 1. Inverted VFO / Channel Badge (x=0..19, y..y+7)
    let v_name = if slot == kernel::service::radio::VfoSlot::VfoA {
        "V1"
    } else {
        "V2"
    };
    let fg_color = gfx::widgets::draw_selection_capsule(
        canvas,
        [0, y as u8, 19, 7],
        if selected {
            gfx::widgets::CAPSULE_FILL
        } else {
            0
        },
    );
    let badge_text = if selected {
        match coord.vfo_memory_index[slot.index()] {
            Some(num) if num < 10 => "M0",
            Some(_) => "M",
            None => v_name,
        }
    } else {
        v_name
    };
    gfx::text::draw_str(canvas, 2, y + 6, badge_text, font, fg_color);

    // 2. RX / TX Active Status Indicator (x=0, y+8)
    let status_txt = if coord.is_slot_tx_active(slot) {
        "TX"
    } else if coord.is_slot_rx_active(slot) {
        "RX"
    } else {
        ""
    };
    if !status_txt.is_empty() {
        draw_tom_fill(canvas, 2, y + 14, status_txt);
    }

    // 3. Authentic Big Frequency (x=30)
    let mut fbuf_uninit = core::mem::MaybeUninit::<[u8; 10]>::uninit();
    // SAFETY: `format_freq_padded` writes all 10 bytes of `fbuf` before any read.
    let fbuf = unsafe { &mut *fbuf_uninit.as_mut_ptr() };
    let (freq_10hz, _) = utils::math::div10_u32(ch.effective_rx_freq().raw());
    let fbytes = gfx::text::format_u32(freq_10hz, fbuf).as_bytes();
    let flen = fbytes.len();
    let mut cur_x = 30i16;

    if flen >= 3 {
        let mhz_end = flen.saturating_sub(5);
        for (i, &c) in fbytes[..flen - 2].iter().enumerate() {
            if i == mhz_end {
                canvas.fill_rect(cur_x as u8, (y + 13) as u8, 3, 2, Color::Fill);
                cur_x += 3;
            }
            gfx::widgets::draw_big_digit(canvas, cur_x, y, c.wrapping_sub(b'0'), Color::Fill);
            cur_x += 13;
        }
        for &c in &fbytes[flen - 2..] {
            gfx::widgets::draw_small_digit(canvas, cur_x, y + 8, c.wrapping_sub(b'0'), Color::Fill);
            cur_x += 7;
        }
    }

    // 4 & 5. Modulation Type and Channel Attributes Line
    draw_tom_fill(canvas, 112, y + 6, ch.modulation.as_str());
    let attr_y = y + 22;
    draw_tom_fill(canvas, 32, attr_y, ch.power.as_str());
    draw_tom_fill(canvas, 44, attr_y, ch.bandwidth.as_str());
    draw_tom_fill(canvas, 56, attr_y, slot.default_name());
}

#[inline(never)]
fn handle_vfo_repeat_tick(
    repeater: &mut gfx::gui::KeyRepeater,
    ctx: &mut SysContext,
) -> EventResult {
    if let Some(repeat_code) = repeater.tick() {
        if matches!(repeat_code, KeyCode::Up | KeyCode::Down) {
            let res = step_freq(ctx, repeat_code == KeyCode::Up);
            if ctx.config.audio.key_beep != 0 {
                ctx.play_beep(crate::services::melody::BeepType::Scroll);
            }
            return res;
        }
    }
    EventResult::Ignored
}

#[inline(never)]
fn handle_vfo_key_press(ctx: &mut SysContext, code: KeyCode) -> EventResult {
    match code {
        KeyCode::Up => step_freq(ctx, true),
        KeyCode::Down => step_freq(ctx, false),
        KeyCode::Key1 => {
            ctx.toggle_vfo();
            EventResult::Consumed
        }
        KeyCode::Key2 => {
            ctx.state.coord.toggle_dual_watch();
            EventResult::Consumed
        }
        KeyCode::Key3 => {
            ctx.state.coord.toggle_cross_band();
            EventResult::Consumed
        }
        KeyCode::Key4 | KeyCode::Star => {
            let slot = ctx.active_vfo();
            ctx.toggle_reverse(slot);
            EventResult::Consumed
        }
        KeyCode::Menu | KeyCode::Exit => {
            ctx.post_event(
                crate::apps::Shortcut::LaunchApp(crate::apps::APP_ID_LAUNCHER as u8).into(),
            );
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl<'a> App<crate::services::SysContext<'a, '_>> for VfoApp {
    fn on_start(&mut self, ctx: &mut SysContext) {
        let slot = ctx.active_vfo();
        ctx.select_vfo(slot);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        match *event {
            Event::Tick => handle_vfo_repeat_tick(&mut self.repeater, ctx),
            Event::Key { code, state } => {
                self.repeater.handle_key(code, state);
                if code == KeyCode::Ptt {
                    ctx.handle_system_ptt(state);
                    EventResult::Consumed
                } else if state == KeyState::Pressed {
                    handle_vfo_key_press(ctx, code)
                } else {
                    EventResult::Ignored
                }
            }
            Event::Bus(bus_ev) if bus_ev.is_radio() => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, ctx: &SysContext) {
        use kernel::service::radio::VfoSlot;
        let coord = &ctx.state.coord;
        let active = coord.active_vfo;
        let slot_b = if active == VfoSlot::VfoC {
            VfoSlot::VfoC
        } else {
            VfoSlot::VfoB
        };
        draw_vfo(canvas, 8, coord, VfoSlot::VfoA, active == VfoSlot::VfoA);
        draw_vfo(canvas, 34, coord, slot_b, active == slot_b);

        // Dotted Divider Line (y: 32)
        gfx::draw::draw_dotted_line_h(canvas, 0, 127, 32, 2, Color::Fill);

        // 4. Live S-Meter Bar at Footer (y: 57 to 63)
        let rssi_dbm = ctx.state.rssi_dbm;
        let peak_hold = if rssi_dbm > -90 { 9 } else { 0 };

        gfx::widgets::draw_s_meter(
            canvas,
            2,
            57,
            rssi_dbm,
            peak_hold,
            &gfx::fonts::tomthumb::FONT,
        );

        // Active Mode Indicators (DW / TW / XB / R) in footer between S-Meter and Uptime
        match ctx.state.coord.dual_watch {
            DualWatchMode::Off => {}
            DualWatchMode::TripleWatch { .. } => draw_tom_fill(canvas, 68, 57, "TW"),
            _ => draw_tom_fill(canvas, 68, 57, "DW"),
        }
        if ctx.state.coord.cross_band != CrossBandMode::Off {
            draw_tom_fill(canvas, 79, 57, "XB");
        }
        if ctx.state.coord.active_channel().reverse {
            draw_tom_fill(canvas, 90, 57, "R");
        }

        // Display Uptime compactly next to S-Meter
        let mut num_buf_uninit = core::mem::MaybeUninit::<[u8; 5]>::uninit();
        // SAFETY: `format_u32` writes its output slice before returning `uptime_str`.
        let num_buf = unsafe { &mut *num_buf_uninit.as_mut_ptr() };
        let uptime_str = gfx::text::format_u32(ctx.power.uptime_secs(), num_buf);
        draw_tom_fill(canvas, 102, 57, "U:");
        draw_tom_fill(canvas, 114, 57, uptime_str);
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
