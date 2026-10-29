use crate::services::power::PowerService;
use crate::services::radio::{
    ClientId, ClientToken, CrossBandMode, DualWatchMode, LockTier, RfClientParticipant, RfSquelch,
    TxPermission,
};
// Main Dual VFO & Telemetry Application

pub mod assets;

use crate::SysContext;
use gfx::{Canvas, Color};
use kernel::app::{App, AppMetadata, EventResult};
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttMode {
    Momentary = 0,
    Toggle = 1,
    DoublePressLatch = 2,
}

impl PttMode {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => PttMode::Toggle,
            2 => PttMode::DoublePressLatch,
            _ => PttMode::Momentary,
        }
    }
}

pub struct VfoApp {
    repeater: gfx::gui::KeyRepeater,
    latched: bool,
    last_ptt_press_ms: u32,
}

impl VfoApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).repeater = gfx::gui::KeyRepeater::new(8, 2);
            (*ptr).latched = false;
            (*ptr).last_ptt_press_ms = 0;
        }
    }

    #[inline(never)]
    fn start_transmitting(&mut self, ctx: &mut SysContext) {
        let perm = ctx.state.coord.request_ptt_start(
            ctx.radio,
            ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND),
        );
        if perm.is_granted() {
            ctx.state.sync_from_coord();
        } else {
            self.latched = false;
            match perm {
                TxPermission::BlockedBusyChannel | TxPermission::BlockedTotPenalty => {
                    if ctx.config.audio.key_beep != 0 {
                        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
                    }
                }
                _ => {}
            }
        }
    }

    #[inline(never)]
    fn stop_transmitting(&mut self, ctx: &mut SysContext) {
        self.latched = false;
        if ctx.state.coord.is_transmitting {
            ctx.state.coord.request_ptt_stop(ctx.radio);
            ctx.state.sync_from_coord();
            if ctx.config.audio.roger_beep != 0 {
                ctx.play_active_roger();
            }
        }
    }

    #[inline(never)]
    fn handle_ptt_event(&mut self, ctx: &mut SysContext, state: KeyState) -> EventResult {
        let mode = PttMode::from_u8(ctx.config.keys.ptt_mode);

        match mode {
            PttMode::Momentary => {
                if state == KeyState::Pressed {
                    self.start_transmitting(ctx);
                } else if state == KeyState::Released {
                    self.stop_transmitting(ctx);
                }
            }
            PttMode::Toggle => {
                if state == KeyState::Pressed {
                    if ctx.state.coord.is_transmitting {
                        self.stop_transmitting(ctx);
                    } else {
                        self.latched = true;
                        self.start_transmitting(ctx);
                    }
                }
            }
            PttMode::DoublePressLatch => {
                if state == KeyState::Pressed {
                    let now_ms = py32_hal::delay::millis();
                    let diff_ms = now_ms.wrapping_sub(self.last_ptt_press_ms);
                    self.last_ptt_press_ms = now_ms;

                    if self.latched {
                        // Any press while latched turns TX off
                        self.stop_transmitting(ctx);
                    } else if diff_ms <= 400 {
                        // Double-press within 400ms: latch TX!
                        self.latched = true;
                        self.start_transmitting(ctx);
                    } else {
                        // First press: momentary transmit start
                        self.start_transmitting(ctx);
                    }
                } else if state == KeyState::Released {
                    // In double-press latch mode: if not latched, release stops momentary TX
                    if !self.latched {
                        self.stop_transmitting(ctx);
                    }
                }
            }
        }
        EventResult::Consumed
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
    let step = (ctx.state.step_khz as i32) * 1_000;
    let slot = ctx.state.coord.active_vfo;
    let delta = if up { step } else { -step };
    ctx.state.coord.step_vfo_freq(ctx.radio, slot, delta);
    ctx.state.sync_from_coord();
    EventResult::Consumed
}

#[inline(never)]
fn draw_vfo(
    canvas: &mut Canvas,
    y: i16,
    vfo: &crate::services::radio::VfoChannelState,
    selected: bool,
) {
    let font = &gfx::fonts::tomthumb::FONT;

    // 1. Inverted VFO / Channel Badge (x=0..19, y..y+7)
    let v_name = if vfo.vfo_num == 1 { "V1" } else { "V2" };
    let (badge_text, fg_color) = if selected {
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
    gfx::text::draw_str(canvas, 2, y + 6, badge_text, font, fg_color);

    // 2. RX / TX Active Status Indicator (x=0, y+8)
    let status_txt = if vfo.tx_active {
        "TX"
    } else if vfo.rx_active {
        "RX"
    } else {
        ""
    };
    if !status_txt.is_empty() {
        gfx::text::draw_str(canvas, 2, y + 14, status_txt, font, Color::Fill);
    }

    // 3. Authentic Big Frequency (x=30)
    let mut fbuf = [0u8; 10];
    let fbytes = gfx::text::format_u32(vfo.frequency.raw() / 10, &mut fbuf).as_bytes();
    let flen = fbytes.len();
    let mut cur_x = 30i16;

    if flen >= 3 {
        let mhz_end = flen.saturating_sub(5);
        for (i, &ch) in fbytes[..flen - 2].iter().enumerate() {
            if i == mhz_end {
                canvas.fill_rect(cur_x as u8, (y + 13) as u8, 3, 2, Color::Fill);
                cur_x += 3;
            }
            gfx::widgets::draw_big_digit(canvas, cur_x, y, ch.wrapping_sub(b'0'), Color::Fill);
            cur_x += 13;
        }
        for &ch in &fbytes[flen - 2..] {
            gfx::widgets::draw_small_digit(
                canvas,
                cur_x,
                y + 8,
                ch.wrapping_sub(b'0'),
                Color::Fill,
            );
            cur_x += 7;
        }
    }

    // 4. Modulation Type (top right)
    gfx::text::draw_str(canvas, 112, y + 6, vfo.mod_type, font, Color::Fill);

    // 5. Attributes Line (beneath frequency)
    let attr_y = y + 22;
    gfx::text::draw_str(canvas, 32, attr_y, vfo.power, font, Color::Fill);
    gfx::text::draw_str(canvas, 44, attr_y, vfo.bw, font, Color::Fill);
    if !vfo.channel_name.is_empty() {
        gfx::text::draw_str(canvas, 56, attr_y, vfo.channel_name, font, Color::Fill);
    }
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
            ctx.state.coord.toggle_vfo(ctx.radio);
            ctx.state.sync_from_coord();
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
            let slot = ctx.state.coord.active_vfo;
            ctx.state.coord.toggle_reverse(ctx.radio, slot);
            ctx.state.sync_from_coord();
            EventResult::Consumed
        }
        KeyCode::Menu => {
            ctx.post_event(
                crate::apps::Shortcut::LaunchApp(crate::apps::APP_ID_LAUNCHER as u8).into(),
            );
            EventResult::Consumed
        }
        KeyCode::Exit => {
            ctx.post_event(
                crate::apps::Shortcut::LaunchApp(crate::apps::APP_ID_LAUNCHER as u8).into(),
            );
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl<'a> App<crate::services::FirmwareContext<'a, '_>> for VfoApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Radio",
            icon: &assets::ICON_16,
            icon_8: &assets::ICON_8,
        }
    }

    fn on_start(&mut self, ctx: &mut SysContext) {
        ctx.state
            .coord
            .select_vfo(ctx.radio, ctx.state.coord.active_vfo);
        ctx.state.sync_from_coord();
        ctx.state.rssi_dbm = ctx.radio.get_rssi_dbm();
        self.latched = false;
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        ctx.state.rssi_dbm = ctx.radio.get_rssi_dbm();

        match *event {
            Event::Tick => handle_vfo_repeat_tick(&mut self.repeater, ctx),
            Event::Key { code, state } => {
                self.repeater.handle_key(code, state);
                if code == KeyCode::Ptt {
                    self.handle_ptt_event(ctx, state)
                } else if state == KeyState::Pressed {
                    handle_vfo_key_press(ctx, code)
                } else {
                    EventResult::Ignored
                }
            }
            Event::Bus(bus_ev) if bus_ev.is_radio() => {
                ctx.state.sync_from_coord();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    fn status_bar_config(&self) -> kernel::app::StatusBarConfig<'_> {
        kernel::app::StatusBarConfig {
            visible: true,
            title: Some("Radio"),
            ..Default::default()
        }
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, ctx: &SysContext) {
        let (vfo_b, sel_b) = if ctx.state.active_vfo == 2 {
            (&ctx.state.vfo_c, true)
        } else {
            (&ctx.state.vfo_b, ctx.state.active_vfo == 1)
        };
        draw_vfo(canvas, 8, &ctx.state.vfo_a, ctx.state.active_vfo == 0);
        draw_vfo(canvas, 34, vfo_b, sel_b);

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
        let font = &gfx::fonts::tomthumb::FONT;
        let mut draw_footer = |x, text| gfx::text::draw_str(canvas, x, 57, text, font, Color::Fill);

        match ctx.state.coord.dual_watch {
            DualWatchMode::Off => {}
            DualWatchMode::TripleWatch { .. } => draw_footer(68, "TW"),
            _ => draw_footer(68, "DW"),
        }
        if ctx.state.coord.cross_band != CrossBandMode::Off {
            draw_footer(79, "XB");
        }
        if ctx.state.coord.active_channel().reverse {
            draw_footer(90, "R");
        }

        // Display Uptime compactly next to S-Meter
        let mut num_buf = [b' '; 5];
        let uptime_str = gfx::text::format_u32(ctx.power.uptime_secs(), &mut num_buf);
        draw_footer(102, "U:");
        draw_footer(114, uptime_str);
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
