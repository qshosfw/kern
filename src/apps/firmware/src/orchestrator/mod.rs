use crate::services::display::DisplayService;
use crate::services::radio::RadioService;
// Microkernel OS Orchestrator & Shell Compositor
// Routes hardware events to the active foreground application, manages background execution,
// and orchestrates global microkernel services like IPC shortcut dispatching.

pub mod compositor;
pub mod keypad_lock;

use crate::apps::{APP_ID_EXPLORER, APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use compositor::OsCompositor;
use gfx::Canvas;
use kernel::app::EventResult;
use kernel::event::{Event, KeyCode, KeyState};
use keypad_lock::KeypadLockManager;

pub struct Orchestrator {
    active_app_id: usize,
    previous_app: Option<usize>,
    lock_mgr: KeypadLockManager,
    side1_held: bool,
    side2_held: bool,
    is_muted: bool,
    is_monitoring: bool,
}

pub static mut SUNK_KEYS: u32 = 0;

pub const BIT_MENU: u32 = 1 << 2;
pub const BIT_KEY5: u32 = 1 << 8;
pub const BIT_EXIT: u32 = 1 << 14;

pub use drivers::keyboard::key_to_bit;

pub fn sink_key(key: KeyCode) {
    let bit = key_to_bit(key);
    unsafe {
        SUNK_KEYS |= bit;
    }
}

pub fn sink_enter_keys() {
    unsafe {
        SUNK_KEYS |= BIT_MENU | BIT_KEY5;
    }
}

pub fn sink_exit_key() {
    unsafe {
        SUNK_KEYS |= BIT_EXIT;
    }
}

pub fn clear_sunk_keys() {
    unsafe {
        SUNK_KEYS = 0;
    }
}

impl Orchestrator {
    pub const fn new(startup_app: usize) -> Self {
        Self {
            active_app_id: startup_app,
            previous_app: None,
            lock_mgr: KeypadLockManager::new(),
            side1_held: false,
            side2_held: false,
            is_muted: false,
            is_monitoring: false,
        }
    }

    pub fn is_keypad_locked(&self) -> bool {
        self.lock_mgr.is_locked
    }

    pub fn is_f_active(&self) -> bool {
        self.lock_mgr.is_f_active
    }

    pub fn toggle_keypad_lock(&mut self) {
        self.lock_mgr.toggle_lock();
    }

    pub fn active_app_id(&self) -> usize {
        self.active_app_id
    }

    /// Reset auto-lock inactivity countdown on external user or hardware activity.
    pub fn notify_activity(&mut self) {
        self.lock_mgr.reset_input_tracking();
    }

    pub fn dispatch_event<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        event: Event,
    ) {
        // Intercept and sink keys suppressed during screen/app/modal transitions
        if let Event::Key { code, state } = event {
            let bit = key_to_bit(code);
            if (unsafe { SUNK_KEYS } & bit) != 0 {
                if state == KeyState::Released {
                    unsafe {
                        SUNK_KEYS &= !bit;
                    }
                    return;
                } else if state == KeyState::Held {
                    return;
                } else {
                    unsafe {
                        SUNK_KEYS &= !bit;
                    }
                }
            }
        }

        // 0. If an OS modal is active, modal compositor intercepts all input
        if !self.handle_modal_event(ctx, &event) {
            let hold_threshold = (ctx.config.keys.hold_ticks() * 2).max(16);
            let f_timeout = ctx.config.keys.f_key_timeout_ticks();
            let auto_lock_ticks = ctx.config.keys.auto_lock_ticks();

            // 1. If keypad is locked, lock manager intercepts all input
            if !self.lock_mgr.is_locked
                || !self.handle_locked_event(
                    ctx,
                    &event,
                    hold_threshold,
                    f_timeout,
                    auto_lock_ticks,
                )
            {
                // 2. Forward event to active foreground app
                let status = crate::AppRegistry::on_event(self.active_app_id, ctx, &event);

                // 3. If foreground app consumed the event, it is completely sunk!
                if status == EventResult::Consumed {
                    self.lock_mgr.reset_input_tracking();
                } else {
                    // 4. Global navigation shortcuts & hotkeys (only if foreground app ignored the event)
                    self.handle_global_shortcuts(
                        ctx,
                        event,
                        hold_threshold,
                        f_timeout,
                        auto_lock_ticks,
                    );
                }
            }
        }

        // Process pending Microkernel IPC System Commands
        self.drain_ipc_commands(ctx);
    }

    fn handle_modal_event<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        event: &Event,
    ) -> bool {
        unsafe {
            let modal_mut =
                &mut *core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL);
            if modal_mut.is_active() {
                if let Some(res) = modal_mut.handle_event(event) {
                    if let Event::Key {
                        code,
                        state: KeyState::Pressed,
                    } = *event
                    {
                        sink_key(code);
                    }
                    crate::AppRegistry::on_modal_result(self.active_app_id, ctx, &res);
                }
                true
            } else {
                false
            }
        }
    }

    fn handle_locked_event<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        event: &Event,
        hold_threshold: u32,
        f_timeout: u32,
        auto_lock_ticks: u32,
    ) -> bool {
        if self
            .lock_mgr
            .handle_event(event, hold_threshold, f_timeout, auto_lock_ticks)
        {
            if !self.lock_mgr.is_locked {
                if let Some(ref mut display) = ctx.display {
                    display.wake();
                }
                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Unlock);
            }
            true
        } else {
            false
        }
    }

    #[inline(never)]
    fn handle_global_shortcuts<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        event: Event,
        hold_threshold: u32,
        f_timeout: u32,
        auto_lock_ticks: u32,
    ) {
        let was_locked = self.lock_mgr.is_locked;
        if self
            .lock_mgr
            .handle_event(&event, hold_threshold, f_timeout, auto_lock_ticks)
        {
            if was_locked != self.lock_mgr.is_locked && self.lock_mgr.is_locked {
                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Lock);
            }
            return;
        }

        match event {
            Event::Key {
                code: KeyCode::Exit,
                state: KeyState::Pressed,
            } => {
                if self.active_app_id != APP_ID_LAUNCHER && self.active_app_id != APP_ID_VFO {
                    self.switch_app(ctx, APP_ID_LAUNCHER);
                    return;
                }
            }
            Event::Key {
                code: KeyCode::Side1,
                state,
            } => {
                if let Some(cfg) = Self::handle_side_key(
                    &mut self.side1_held,
                    state,
                    ctx.config.keys.f1_long,
                    ctx.config.keys.f1_short,
                ) {
                    self.process_ipc_command(ctx, Shortcut::from_u8(cfg));
                }
            }
            Event::Key {
                code: KeyCode::Side2,
                state,
            } => {
                if let Some(cfg) = Self::handle_side_key(
                    &mut self.side2_held,
                    state,
                    ctx.config.keys.f2_long,
                    ctx.config.keys.f2_short,
                ) {
                    self.process_ipc_command(ctx, Shortcut::from_u8(cfg));
                }
            }
            Event::Key {
                code: KeyCode::Menu,
                state: KeyState::Held,
            } => {
                self.process_ipc_command(ctx, Shortcut::from_u8(ctx.config.keys.m_long));
            }
            _ => {}
        }

        if self.lock_mgr.is_f_active {
            if let Event::Key {
                code,
                state: KeyState::Pressed,
            } = event
            {
                if code != KeyCode::KeyF {
                    self.lock_mgr.is_f_active = false;
                    let digit_idx = code
                        .to_digit()
                        .filter(|&d| d >= 1)
                        .map(|d| (d - 1) as usize);
                    if let Some(idx) = digit_idx {
                        let action = ctx.config.keys.f_digits[idx];
                        if action != 0 {
                            self.process_ipc_command(ctx, Shortcut::from_u8(action));
                            return;
                        }
                    }
                }
            }
        }
    }

    #[inline(never)]
    fn drain_ipc_commands<'a>(&mut self, ctx: &mut crate::services::FirmwareContext<'a, '_>) {
        loop {
            let next_cmd = match ctx.ipc {
                Some(ref mut queue) => queue.pop().map(Shortcut::from),
                None => None,
            };
            if let Some(cmd) = next_cmd {
                self.process_ipc_command(ctx, cmd);
            } else {
                break;
            }
        }
    }

    fn handle_side_key(
        held_flag: &mut bool,
        state: KeyState,
        long_cfg: u8,
        short_cfg: u8,
    ) -> Option<u8> {
        match state {
            KeyState::Held => {
                *held_flag = true;
                Some(long_cfg)
            }
            KeyState::Released => {
                let fired = !*held_flag;
                *held_flag = false;
                if fired { Some(short_cfg) } else { None }
            }
            _ => None,
        }
    }

    pub fn switch_app<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        app_id: usize,
    ) {
        if app_id < crate::AppRegistry::app_count() {
            if self.active_app_id < crate::AppRegistry::app_count() {
                crate::AppRegistry::on_exit(self.active_app_id, ctx);
            }
            if self.active_app_id != app_id {
                self.previous_app = Some(self.active_app_id);
            }
            self.active_app_id = app_id;
            sink_enter_keys();
            sink_exit_key();
            crate::AppRegistry::on_start(self.active_app_id, ctx);
        }
    }

    #[inline(never)]
    pub fn process_ipc_command<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        cmd: Shortcut,
    ) {
        match cmd {
            Shortcut::None => {}
            Shortcut::LaunchApp(id) => self.switch_app(ctx, id as usize),
            Shortcut::Flashlight => {
                if bsp::pins::FLASHLIGHT.is_high() {
                    bsp::pins::FLASHLIGHT.set_low();
                } else {
                    bsp::pins::FLASHLIGHT.set_high();
                }
            }
            Shortcut::Scanner | Shortcut::Explorer => self.switch_app(ctx, APP_ID_EXPLORER),
            Shortcut::Settings => self.switch_app(ctx, APP_ID_SETTINGS),
            Shortcut::LockKeypad => self.toggle_keypad_lock(),
            Shortcut::ToggleVfo => ctx.state.toggle_vfo(),
            Shortcut::ToggleTxPower => {
                let vfo = ctx.state.selected_vfo_mut();
                vfo.power = match vfo.power {
                    "L" => "M",
                    "M" => "H",
                    _ => "L",
                };
            }
            Shortcut::PlayBeep {
                freq_hz,
                duration_ms,
            } => {
                ctx.play_tone(freq_hz, duration_ms);
            }
            Shortcut::Monitor => {
                self.is_monitoring = !self.is_monitoring;
                let sql = if self.is_monitoring {
                    0
                } else {
                    ctx.state.squelch_level.clamp(1, 9)
                };
                ctx.radio.set_squelch(sql);
            }
            Shortcut::MuteAudio => {
                self.is_muted = !self.is_muted;
                if self.is_muted {
                    ctx.set_volume(0);
                } else {
                    ctx.set_volume(ctx.config.audio.volume);
                }
            }
            Shortcut::FmRadio => {
                self.switch_app(ctx, APP_ID_VFO);
            }
            Shortcut::Reboot => kernel::sys::SystemDiagnostics::reboot(),
        }
    }

    #[inline]
    pub fn execute_shortcut<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        action: Shortcut,
    ) {
        self.process_ipc_command(ctx, action);
    }

    pub fn render<'a>(
        &self,
        canvas: &mut Canvas,
        ctx: &'a crate::services::FirmwareContext<'a, '_>,
    ) {
        OsCompositor::render(self.active_app_id, canvas, ctx, &self.lock_mgr);
    }
}
