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
#[cfg(feature = "wire")]
pub mod wire;

pub use crate::data::arena::staging_buf;
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
#[cfg(feature = "wire")]
pub use wire::*;

pub type SystemRadio<'a> = drivers::bk4829::Bk4829<'a, drivers::bk4829::Disabled>;
pub use drivers::audio::AudioAmplifier as SystemAudio;
pub use drivers::battery::SystemPower;
pub type SystemDisplay<'a, 'b> = display::DisplayHardware<'a, 'b>;
#[cfg(feature = "uart")]
pub use drivers::uart::HardwareUart as SystemUart;
pub use drivers::usb::UsbDriver as SystemUsb;

/// Dependency-injected System Context provided to applications during event dispatch and rendering.
pub struct SysContext<'a, 'hw> {
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

#[inline(never)]
fn play_tone_file(storage: &mut SystemStorage, radio: &mut SystemRadio, path: &str) {
    let mut notes = [melody::ToneNote::new(0, 0); melody::MAX_MELODY_NOTES];
    let n = crate::services::melody::load_roger_notes(storage, path, &mut notes);
    if n > 0 {
        radio.play_tone_sequence(&notes[..n]);
    }
}

impl<'a, 'hw> SysContext<'a, 'hw> {
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
    #[inline(always)]
    pub fn show_popup(&mut self, tag: u8, title: &str, message: &str) {
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_popup(tag, title, message);
        }
    }

    /// Show an OS modal confirmation dialog with Confirm/Cancel badges.
    #[inline(always)]
    pub fn show_confirm(
        &mut self,
        tag: u8,
        title: &str,
        message: &str,
        confirm: &'static str,
        cancel: &'static str,
    ) {
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_confirm(tag, title, message, confirm, cancel);
        }
    }

    /// Show a destructive confirmation dialog requiring `hold_ms` hold on `Menu`.
    #[inline(always)]
    pub fn show_hold_confirm(
        &mut self,
        tag: u8,
        title: &str,
        message: &str,
        confirm: &'static str,
        cancel: &'static str,
        hold_ms: u16,
    ) {
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_hold_confirm(tag, title, message, confirm, cancel, hold_ms);
        }
    }

    /// Open an OS modal text input prompt.
    #[cfg(feature = "text_input")]
    #[inline(always)]
    pub fn prompt_text(
        &mut self,
        tag: u8,
        title: &str,
        initial: &str,
        max_len: usize,
        digits_only: bool,
    ) {
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

    #[cfg(not(feature = "text_input"))]
    #[inline(always)]
    pub fn prompt_text(
        &mut self,
        _tag: u8,
        _title: &str,
        _initial: &str,
        _max_len: usize,
        _digits_only: bool,
    ) {
    }

    /// Open an OS modal passcode input prompt with dots.
    #[cfg(feature = "passcode")]
    #[inline(always)]
    pub fn prompt_passcode(
        &mut self,
        tag: u8,
        title: &str,
        max_len: usize,
        digits_only: bool,
        expose_len: bool,
    ) {
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .prompt_passcode(tag, title, max_len, digits_only, expose_len);
        }
    }

    /// Open an OS modal bottom option sheet.
    #[inline(always)]
    pub fn show_sheet(&mut self, tag: u8, sheet: gfx::gui::modules::menu::OptionSheet) {
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_sheet(tag, sheet);
        }
    }

    /// Close any active OS modal dialog immediately.
    #[inline(always)]
    pub fn close_modal(&mut self) {
        crate::orchestrator::sink_enter_keys();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL)).close();
        }
    }

    /// Returns true if an OS modal is currently covering the screen.
    #[inline(always)]
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
    #[inline(never)]
    pub fn play_active_roger(&mut self) {
        if self.config.audio.roger_beep != 0 {
            let path = self.config.audio.roger_path_str();
            play_tone_file(self.storage, self.radio, path);
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

    #[inline(never)]
    pub fn draw_status_bar_right_mode(
        &self,
        canvas: &mut gfx::Canvas,
        inverted: bool,
        partial: bool,
    ) -> i16 {
        let is_usb = self
            .usb
            .as_ref()
            .map(|u| u.is_connected())
            .unwrap_or_else(|| {
                #[cfg(feature = "usb")]
                {
                    drivers::usb::is_active_usb_connected()
                }
                #[cfg(not(feature = "usb"))]
                {
                    false
                }
            });

        let pct = self.power.battery_percentage();
        let is_charging = self.power.is_charging();
        let is_low = self.power.is_low() || pct == 0;
        let serial_active = py32_hal::uart::is_active(800);
        let now_ms = py32_hal::delay::millis();
        let anim_step = ((now_ms >> 8) & 0xFF) as u8;
        let shading_mode = self.config.display.shading_mode;
        let tray_glyphs = crate::orchestrator::status_tray().active_glyphs();

        let flags = (is_charging as u8)
            | ((is_low as u8) << 1)
            | ((is_usb as u8) << 2)
            | ((serial_active as u8) << 3)
            | ((self.is_f_active as u8) << 4)
            | ((self.is_keypad_locked as u8) << 5)
            | ((inverted as u8) << 6)
            | ((partial as u8) << 7);

        let cfg = gfx::widgets::status_bar::StatusRightConfig {
            percentage: pct,
            battery_format: self.config.battery.battery_format,
            shading_mode,
            anim_step,
            flags,
            voltage_mv: self.power.battery_voltage_mv(),
            temp_deci_c: self.power.mcu_temperature_deci_c(),
            dynamic_glyphs: tray_glyphs,
        };

        cfg.render(canvas)
    }

    /// Renders a fully declarative, modular, auto-justified status bar
    #[inline(never)]
    pub fn draw_status_bar_cfg(
        &self,
        canvas: &mut gfx::Canvas,
        config: &kernel::app::StatusBarConfig<'_>,
    ) {
        let right_limit = self.draw_status_bar_right_mode(canvas, false, config.partial);
        if !config.partial {
            gfx::widgets::status_bar::draw_status_bar_layout(canvas, config, right_limit);
        }
    }
}

#[cfg(feature = "passcode")]
pub fn draw_boot_status_right(canvas: &mut gfx::Canvas, battery_format: u8, is_usb: bool) -> i16 {
    let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
    let pct = pwr.battery_percentage();
    let is_charging = pwr.is_charging();
    let is_low = pwr.is_low() || pct == 0;
    let mut flags = gfx::widgets::status_bar::STATUS_FLAG_INVERTED;
    if is_charging {
        flags |= gfx::widgets::status_bar::STATUS_FLAG_CHARGING;
    }
    if is_low {
        flags |= gfx::widgets::status_bar::STATUS_FLAG_LOW;
    }
    if is_usb {
        flags |= gfx::widgets::status_bar::STATUS_FLAG_USB;
    }
    let cfg = gfx::widgets::status_bar::StatusRightConfig {
        percentage: pct,
        battery_format,
        shading_mode: crate::master_config_ref().display.shading_mode,
        anim_step: 0,
        flags,
        voltage_mv: pwr.battery_voltage_mv(),
        temp_deci_c: pwr.mcu_temperature_deci_c(),
        dynamic_glyphs: &[],
    };
    cfg.render(canvas)
}

impl<'a, 'hw> kernel::caps::CapsCheck for SysContext<'a, 'hw> {
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
