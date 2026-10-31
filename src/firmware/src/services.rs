pub mod audio;
pub mod calibration;
pub mod display;
#[cfg(feature = "uart")]
pub mod io;
pub mod melody;
pub mod power;
pub mod primitives;
pub mod radio;
pub mod storage;
pub mod telemetry;
pub mod usb;
pub mod wire;

pub use audio::*;
pub use calibration::*;
pub use display::*;
#[cfg(feature = "uart")]
pub use io::*;
pub use melody::*;
pub use power::*;
pub use radio::*;
pub use storage::*;
pub use telemetry::*;
pub use usb::*;
pub use wire::*;

pub type SystemRadio<'a> = drivers::bk4829::Bk4829<'a, drivers::bk4829::Disabled>;
pub use drivers::audio::AudioAmplifier as SystemAudio;
pub use drivers::battery::SystemPower;
pub type SystemDisplay<'a, 'b> = display::DisplayHardware<'a, 'b>;
#[cfg(feature = "uart")]
pub use drivers::uart::HardwareUart as SystemUart;
pub use drivers::usb::UsbDriver as SystemUsb;

/// Dependency-injected System Context provided to applications during event dispatch and rendering.
pub struct FirmwareContext<'a, 'hw> {
    pub state: &'a mut RadioState,
    pub radio: &'a mut SystemRadio<'hw>,
    pub power: &'a mut SystemPower,
    pub audio: Option<&'a mut SystemAudio>,
    pub display: Option<&'a mut SystemDisplay<'a, 'hw>>,
    pub ipc: Option<&'a mut kernel::ipc::MessageQueue<kernel::ipc::SysEvent, 8>>,
    pub config: &'a mut crate::config::MasterConfig,
    pub is_keypad_locked: bool,
    pub is_f_active: bool,
    pub usb: Option<&'a mut SystemUsb>,

    pub storage: &'a mut SystemStorage<'hw>,
    pub calibration: &'a SystemCalibration,
    pub passcode: &'a mut crate::security::PasscodeState,

    #[cfg(feature = "uart")]
    pub uart: &'a mut SystemUart,
}

fn play_tone_file(storage: &mut SystemStorage, radio: &mut SystemRadio, path: &str) {
    let buf = crate::services::melody::load_roger_melody(storage, path);
    if !buf.is_empty() {
        radio.play_tone_sequence(&buf);
    }
}

impl<'a, 'hw> FirmwareContext<'a, 'hw> {
    /// Check if current session is authorized for given trust scope
    pub fn is_authorized(&mut self, scope: crate::security::TrustScope) -> bool {
        crate::security::check_authorization(self.passcode, self.storage, scope).is_ok()
    }

    /// Obtain an AuthToken capability if session is authorized
    pub fn require_auth(
        &mut self,
        scope: crate::security::TrustScope,
    ) -> Result<crate::security::AuthToken, crate::security::passcode::PasscodeError> {
        crate::security::check_authorization(self.passcode, self.storage, scope)
    }

    /// Execute a closure requiring privileged authorization by verifying passcode or using existing session authorization
    pub fn with_auth<R, F: FnOnce(&crate::security::AuthToken) -> R>(
        &mut self,
        scope: crate::security::TrustScope,
        passcode: &[u8],
        f: F,
    ) -> Result<R, crate::security::passcode::PasscodeError> {
        let token = if self.is_authorized(scope) {
            crate::security::AuthToken::new(scope)
        } else {
            crate::security::authenticate(self.passcode, self.storage, passcode, scope)?
        };
        Ok(f(&token))
    }

    /// Send high-level SysEvent IPC request to the microkernel/OS dispatcher
    pub fn post_event(&mut self, event: kernel::ipc::SysEvent) -> bool {
        if let Some(ref mut queue) = self.ipc {
            queue.try_push(event).is_ok()
        } else {
            false
        }
    }

    /// Check if system supports a specific hardware or subsystem capability token
    #[inline(always)]
    pub fn has_cap(&self, cap: kernel::caps::Cap) -> bool {
        kernel::caps::CapsCheck::has(self, cap)
    }

    /// Get composite hardware capability token set
    #[inline(always)]
    pub fn caps(&self) -> kernel::caps::CapabilitySet {
        kernel::caps::CapsCheck::caps(self)
    }

    /// Read-only access to global process table
    #[inline(always)]
    pub fn proc_table(&self) -> &'static kernel::proc::ProcessTable<8> {
        crate::orchestrator::proc_table()
    }

    /// Mutable access to global process table
    #[inline(always)]
    pub fn proc_table_mut(&mut self) -> &'static mut kernel::proc::ProcessTable<8> {
        crate::orchestrator::proc_table_mut()
    }

    /// Sets master system volume on audio amplifier, radio AF DAC, and config

    /// Suppress and sink a specific key until its physical release is complete.
    #[inline(always)]
    pub fn sink_key(&mut self, key: kernel::event::KeyCode) {
        crate::orchestrator::sink_key(key);
    }

    /// Suppress and sink all enter/selection keys (Menu and Key5) until released.
    #[inline(always)]
    pub fn sink_enter_keys(&mut self) {
        crate::orchestrator::sink_enter_keys();
    }

    /// Suppress and sink exit/back key until released.
    #[inline(always)]
    pub fn sink_exit_key(&mut self) {
        crate::orchestrator::sink_exit_key();
    }

    /// Flush any dirty configuration blocks to storage.
    #[inline(never)]
    pub fn flush_config(&mut self) -> Result<(), crate::config::ConfigError> {
        self.config.flush_dirty(self.storage)
    }

    /// Show an OS modal popup dialog on top of the active application.
    pub fn show_popup(&mut self, tag: u8, title: &'static str, message: &'static str) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_popup(tag, title, message);
        }
    }

    /// Show an OS modal popup dialog with a dynamic message slice.
    pub fn show_popup_str(&mut self, tag: u8, title: &'static str, message: &str) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_popup_str(tag, title, message);
        }
    }

    /// Show an OS modal confirmation dialog with Confirm/Cancel badges.
    pub fn show_confirm(
        &mut self,
        tag: u8,
        title: &'static str,
        message: &'static str,
        confirm: &'static str,
        cancel: &'static str,
    ) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_confirm(tag, title, message, confirm, cancel);
        }
    }

    /// Show an OS modal popup dialog with dynamic title and message slices.
    pub fn show_popup_dynamic(&mut self, tag: u8, title: &str, message: &str) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_popup_dynamic(tag, title, message);
        }
    }

    /// Show an OS modal confirmation dialog with dynamic title and message slices.
    pub fn show_confirm_dynamic(
        &mut self,
        tag: u8,
        title: &str,
        message: &str,
        confirm: &'static str,
        cancel: &'static str,
    ) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_confirm_dynamic(tag, title, message, confirm, cancel);
        }
    }

    /// Open an OS modal text input prompt.
    #[cfg(feature = "passcode")]
    pub fn prompt_text(
        &mut self,
        tag: u8,
        title: &'static str,
        initial: &str,
        max_len: usize,
        digits_only: bool,
    ) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL)).prompt_text(
                tag,
                title,
                initial,
                max_len,
                digits_only,
            );
        }
    }

    /// Open an OS modal passcode input prompt with dots.
    #[cfg(feature = "passcode")]
    pub fn prompt_passcode(
        &mut self,
        tag: u8,
        title: &'static str,
        max_len: usize,
        digits_only: bool,
        expose_len: bool,
    ) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .prompt_passcode(tag, title, max_len, digits_only, expose_len);
        }
    }

    /// Open an OS modal bottom option sheet.
    pub fn show_sheet(&mut self, tag: u8, sheet: gfx::gui::modules::menu::OptionSheet) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_sheet(tag, sheet);
        }
    }

    /// Close any active OS modal dialog immediately.
    pub fn close_modal(&mut self) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL)).close();
        }
    }

    /// Returns true if an OS modal is currently covering the screen.
    pub fn has_active_modal(&self) -> bool {
        unsafe { (*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL)).is_active() }
    }

    pub fn set_volume(&mut self, level: u8) {
        if let Some(ref mut audio) = self.audio {
            audio.set_volume(level);
        }
        self.radio.set_volume_level(level);
        self.config.audio.volume = level;
    }

    pub fn apply_audio_config(&mut self) {
        let cfg = self.config.audio;
        if let Some(ref mut audio) = self.audio {
            audio.set_volume(cfg.volume);
        }
        self.radio.set_volume_level(cfg.volume);
        self.radio
            .set_tone_gain((cfg.beep_gain as u16 * 12).min(127) as u8);
        self.radio.set_mic_gain(cfg.mic_gain);
        self.radio.set_audio_profile(cfg.audio_profile);
    }

    /// Play single keypress or system feedback beep
    pub fn play_beep(&mut self, beep: BeepType) {
        self.radio.play_beep(beep);
    }

    /// Play a system dialog / notification sound.
    /// Respects volume setting — silent when volume is 0.
    pub fn play_dialog_sound(&mut self, sound: melody::DialogSoundType) {
        if self.config.audio.volume > 0 {
            self.radio.play_dialog_sound(sound);
        }
    }

    /// Play roger beep melody from file path with automatic fallback
    pub fn play_roger_path(&mut self, path: &str) {
        play_tone_file(self.storage, self.radio, path);
    }

    /// Play currently configured active roger beep
    pub fn play_active_roger(&mut self) {
        if self.config.audio.roger_beep != 0 {
            let path = self.config.audio.roger_path_str();
            let mel = crate::services::melody::load_roger_melody(self.storage, path);
            if !mel.is_empty() {
                self.radio.play_tone_sequence(mel.as_slice());
            }
        }
    }

    /// Play roger beep melody (backward compatibility)
    pub fn play_roger(&mut self, roger: RogerType) {
        if roger == RogerType::Off {
            return;
        }
        self.play_active_roger();
    }

    /// Play customized frequency tone note
    pub fn play_tone(&mut self, freq_hz: u16, duration_ms: u16) {
        self.radio.play_tone(freq_hz, duration_ms);
    }

    /// Backwards-compatible status bar drawing with explicit title and center_text
    #[inline(always)]
    pub fn draw_status_bar(
        &self,
        canvas: &mut gfx::Canvas,
        title: &str,
        center_text: Option<&str>,
    ) {
        let mut cfg = kernel::app::StatusBarConfig::new(title);
        cfg.center_text = center_text;
        self.draw_status_bar_cfg(canvas, &cfg);
    }

    #[inline(never)]
    pub fn draw_status_bar_right_mode(&self, canvas: &mut gfx::Canvas, inverted: bool) -> i16 {
        let is_usb = self.usb.as_ref().map(|u| u.is_connected()).unwrap_or(false);
        draw_status_right_unified(
            canvas,
            is_usb,
            self.is_f_active,
            self.is_keypad_locked,
            self.config.battery.battery_format,
            inverted,
        )
    }

    #[inline(always)]
    fn draw_status_bar_right(&self, canvas: &mut gfx::Canvas) -> i16 {
        self.draw_status_bar_right_mode(canvas, false)
    }

    /// Renders a fully declarative, modular, auto-justified status bar
    #[inline(never)]
    pub fn draw_status_bar_cfg(
        &self,
        canvas: &mut gfx::Canvas,
        config: &kernel::app::StatusBarConfig<'_>,
    ) {
        use gfx::Color;
        use gfx::fonts::tomthumb::FONT;
        use gfx::widgets::status_bar::{SIGNAL_WIDTH, draw_badge, draw_signal};

        // Clear status bar background & draw dimmed divider line if enabled
        if config.show_divider {
            canvas.fill_rect(0, 0, 128, 7, Color::Clear);
            canvas.draw_horizontal_line(0, 6, 128, Color::DarkGray);
        }

        // 1. Right items (flowing right-to-left)
        let right_limit = self.draw_status_bar_right(canvas);

        // 2. Left items (flowing left-to-right)
        let mut left_x = 1i16;
        if let Some(lvl) = config.signal_level {
            draw_signal(canvas, left_x, 0, lvl, Color::Fill);
            left_x += (SIGNAL_WIDTH as i16) + 2;
        }
        if let Some(title) = config.title {
            if !title.is_empty() {
                let w = gfx::text::str_width(title, &FONT) as i16;
                gfx::text::draw_str_max_x(
                    canvas,
                    left_x,
                    4,
                    title,
                    &FONT,
                    Color::Fill,
                    right_limit,
                );
                left_x += w + 2;
            }
        }
        if let Some(badge) = config.left_badge {
            if !badge.is_empty() {
                let bw = draw_badge(canvas, left_x, 0, badge, &FONT, false) as i16;
                left_x += bw + 2;
            }
        }

        // 3. Center items
        if let Some(center) = config.center_text {
            if !center.is_empty() {
                let w = gfx::text::str_width(center, &FONT) as i16;
                let ideal_x = 64i16 - (w >> 1);
                let x = if ideal_x >= left_x && ideal_x + w <= right_limit {
                    ideal_x
                } else {
                    left_x + ((right_limit - left_x - w).max(0) >> 1)
                };
                if x + w <= right_limit {
                    gfx::text::draw_str(canvas, x, 4, center, &FONT, Color::Fill);
                }
            }
        }
    }
}

#[inline(never)]
pub fn draw_status_right_unified(
    canvas: &mut gfx::Canvas,
    is_usb: bool,
    is_f_active: bool,
    is_keypad_locked: bool,
    bfmt: u8,
    inverted: bool,
) -> i16 {
    use gfx::Color;
    use gfx::fonts::tomthumb::FONT;
    use gfx::widgets::status_bar::{draw_battery_colored, draw_f_key};

    let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
    let pct = pwr.battery_percentage();
    let voltage_mv = pwr.battery_voltage_mv();
    let temp_deci_c = pwr.mcu_temperature_deci_c();
    let is_charging = pwr.is_charging();
    let is_low = pwr.is_low() || pct == 0;
    let serial_active = py32_hal::uart::is_active(800);

    let (fg, bg) = if inverted {
        (Color::Clear, Color::Fill)
    } else {
        (Color::Fill, Color::Clear)
    };

    let y_offset: u8 = if inverted { 2 } else { 0 };
    let y_text: i16 = if inverted { 6 } else { 4 };

    let mut right_limit = 126i16; // 127 - 1px padding
    let now_ms = py32_hal::delay::millis();
    let has_bat_icon = matches!(bfmt, 3..=5);

    let shading_mode = crate::master_config_ref().display.shading_mode;

    if has_bat_icon {
        let anim_step = ((now_ms >> 8) & 0xFF) as u8;
        right_limit -= 12;
        let blink_off = is_low && !is_charging && (((now_ms >> 9) & 1) != 0);
        if !blink_off {
            draw_battery_colored(
                canvas,
                right_limit as u8,
                y_offset,
                pct,
                is_charging,
                anim_step,
                fg,
                bg,
                shading_mode,
            );
        }
    }

    if is_charging {
        let bolt_x = if has_bat_icon {
            right_limit - 7
        } else {
            right_limit - 5
        };
        gfx::icons::draw_icon_5x5(canvas, bolt_x, y_offset as i16, gfx::icons::Icon::Bolt, fg);
        right_limit = bolt_x;
    } else if is_low {
        right_limit -= if has_bat_icon { 5 } else { 4 };
        gfx::text::draw_char(canvas, right_limit, y_text, '!', &FONT, fg);
    }

    if is_usb {
        let plug_icon = if serial_active {
            gfx::icons::Icon::SerialActive
        } else {
            gfx::icons::Icon::Plug
        };
        let plug_x = if is_charging {
            right_limit - 9
        } else if has_bat_icon {
            right_limit - 10
        } else {
            right_limit - 9
        };
        gfx::icons::draw_icon_5x5(canvas, plug_x, y_offset as i16, plug_icon, fg);
        right_limit = plug_x;
    }

    let mut nbuf = [0u8; 12];
    let (val, dec, suf_b) = match bfmt {
        1 | 4 => (voltage_mv as i32 / 10, 2, b'v'),
        6 => (voltage_mv as i32 / 100, 1, b'v'),
        2 | 5 | 7 => (pct as i32, 0, b'%'),
        8 => (temp_deci_c as i32 / 10, 0, b'C'),
        _ => (0, 0, 0),
    };
    let bat_str = if suf_b != 0 {
        let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_ref(&suf_b)) };
        gfx::text::format_int_fixed(val, dec, "", s, &mut nbuf)
    } else {
        ""
    };
    if !bat_str.is_empty() {
        let w = gfx::text::str_width(bat_str, &FONT) as i16;
        right_limit -= w + 2;
        gfx::text::draw_str(canvas, right_limit, y_text, bat_str, &FONT, fg);
    }

    if !inverted {
        if is_f_active {
            right_limit -= 7;
            draw_f_key(canvas, right_limit, y_offset as i16);
        }
        if is_keypad_locked {
            right_limit -= 7;
            gfx::icons::draw_icon_5x5(
                canvas,
                right_limit,
                y_offset as i16,
                gfx::icons::Icon::Lock,
                Color::Fill,
            );
        }
    }

    right_limit
}

pub fn draw_boot_status_right(canvas: &mut gfx::Canvas, battery_format: u8, is_usb: bool) -> i16 {
    draw_status_right_unified(canvas, is_usb, false, false, battery_format, true)
}

impl<'a, 'hw> kernel::caps::CapsCheck for FirmwareContext<'a, 'hw> {
    #[inline(always)]
    fn caps(&self) -> kernel::caps::CapabilitySet {
        const BASE_CAPS: kernel::caps::CapabilitySet = kernel::caps::CapabilitySet::NONE
            .with(kernel::caps::Cap::RadioRx)
            .with(kernel::caps::Cap::RadioTx)
            .with(kernel::caps::Cap::StorageRead)
            .with(kernel::caps::Cap::StorageWrite)
            .with(kernel::caps::Cap::AudioOutput)
            .with(kernel::caps::Cap::DisplayDirect)
            .with(kernel::caps::Cap::Keypad)
            .with(kernel::caps::Cap::AudioCapture)
            .with(kernel::caps::Cap::TaskControl)
            .with(kernel::caps::Cap::ServiceRegistry)
            .with(kernel::caps::Cap::SerialComm)
            .with(kernel::caps::Cap::UsbComm)
            .with(kernel::caps::Cap::HardwareCrc)
            .with(kernel::caps::Cap::RealTimeClock)
            .with(kernel::caps::Cap::PowerManage);

        #[allow(unused_mut)]
        let mut c = BASE_CAPS;
        #[cfg(feature = "fm_radio")]
        {
            c = c.with(kernel::caps::Cap::BroadcastFm);
        }
        #[cfg(feature = "secure_fs")]
        {
            c = c.with(kernel::caps::Cap::CryptoAead);
        }
        c
    }
}
