use crate::services::display::DisplayService;
#[cfg(feature = "radio")]
use crate::services::radio::RfSquelch;
// Microkernel OS Orchestrator & Shell Compositor
// Routes hardware events to the active foreground application, manages background execution,
// and orchestrates global microkernel services like IPC shortcut dispatching.

pub mod compositor;
pub mod keypad_lock;

use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use compositor::OsCompositor;
use gfx::Canvas;
use kernel::app::EventResult;
use kernel::caps::CapabilitySet;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::ipc::{ipc_flags, ipc_msg_type, ipc_target};
use kernel::nav::NavStack;
use kernel::proc::{ExecType, ProcState, ProcessTable};
use keypad_lock::KeypadLockManager;

#[repr(C)]
pub struct Orchestrator {
    side1_held: bool,
    side2_held: bool,
    is_muted: bool,
    is_monitoring: bool,
    owned_key: Option<KeyCode>,
    active_app_id: usize,
    previous_app: Option<usize>,
    lock_mgr: KeypadLockManager,
    nav_stack: NavStack<6>,
}

pub const MAX_PROCESSES: usize = 8;

#[unsafe(link_section = ".uninit")]
pub static mut PROC_TABLE: core::mem::MaybeUninit<ProcessTable<MAX_PROCESSES>> =
    core::mem::MaybeUninit::uninit();

#[inline(always)]
pub fn proc_table() -> &'static ProcessTable<MAX_PROCESSES> {
    unsafe { (*core::ptr::addr_of!(PROC_TABLE)).assume_init_ref() }
}

#[inline(always)]
pub fn proc_table_mut() -> &'static mut ProcessTable<MAX_PROCESSES> {
    unsafe { (*core::ptr::addr_of_mut!(PROC_TABLE)).assume_init_mut() }
}

#[inline(always)]
pub fn init_proc_table() {
    unsafe {
        *core::ptr::addr_of_mut!(PROC_TABLE) = core::mem::MaybeUninit::new(ProcessTable::new());
    }
    #[cfg(feature = "krx_native")]
    crate::runtime::init_native_slot();
}

static mut LEASE_MGR: kernel::proc::ResourceLeaseManager<8> =
    kernel::proc::ResourceLeaseManager::new();

#[inline(always)]
pub fn lease_manager() -> &'static kernel::proc::ResourceLeaseManager<8> {
    unsafe { &*core::ptr::addr_of!(LEASE_MGR) }
}

#[inline(always)]
pub fn lease_manager_mut() -> &'static mut kernel::proc::ResourceLeaseManager<8> {
    unsafe { &mut *core::ptr::addr_of_mut!(LEASE_MGR) }
}

static mut STATUS_TRAY: kernel::proc::StatusTray<4> = kernel::proc::StatusTray::new();

#[inline(always)]
pub fn status_tray() -> &'static kernel::proc::StatusTray<4> {
    unsafe { &*core::ptr::addr_of!(STATUS_TRAY) }
}

#[inline(always)]
pub fn status_tray_mut() -> &'static mut kernel::proc::StatusTray<4> {
    unsafe { &mut *core::ptr::addr_of_mut!(STATUS_TRAY) }
}

pub static mut SUNK_KEYS: u32 = 0;

pub const BIT_MENU: u32 = 1 << 2;
pub const BIT_KEY5: u32 = 1 << 8;
pub const BIT_EXIT: u32 = 1 << 14;

pub const TAG_FAULT_ALERT: u8 = 0xFE;
pub const TAG_FAULT_CONFIRM: u8 = 0xFF;

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

fn filter_sunk_keys(event: &Event) -> bool {
    if let Event::Key { code, state } = *event {
        let bit = key_to_bit(code);
        if (unsafe { SUNK_KEYS } & bit) != 0 {
            if state == KeyState::Released {
                unsafe {
                    SUNK_KEYS &= !bit;
                }
                return true;
            } else if state == KeyState::Held {
                return true;
            } else {
                // KeyState::Pressed: A new press arrives, clear the stale sunk bit and allow it through!
                unsafe {
                    SUNK_KEYS &= !bit;
                }
                return false;
            }
        }
    }
    false
}

pub fn record_proc_activation(app_id: usize) {
    if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
        pcb.ledger.record_activation();
    }
}

fn record_proc_event_and_stack(app_id: usize) {
    if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
        pcb.ledger.record_event();
        let used_stk = kernel::sys::SystemDiagnostics::stack_used_now() as u16;
        pcb.ledger.record_stack_depth(used_stk);
    }
}

#[inline(never)]
fn launch_krx_shortcut(ctx: &mut crate::services::SysContext, path: &str) {
    if let Err(err) = crate::apps::runtime::launch_krx_package(ctx, path) {
        ctx.show_confirm(0, err.title(), err.message(), "OK", "EXIT");
        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
    }
}

impl Orchestrator {
    pub fn new(startup_app: usize) -> Self {
        init_proc_table();
        let table = proc_table_mut();

        fn spawn_proc(
            table: &mut ProcessTable<MAX_PROCESSES>,
            name: &str,
            app_id: usize,
            priority: kernel::sched::Priority,
            caps: CapabilitySet,
            state: ProcState,
        ) {
            let _ = table.spawn(name, app_id, priority, ExecType::StaticFirmware, caps);
            if let Some(pcb) = table.find_mut_by_app(app_id) {
                pcb.set_state(state);
                pcb.ledger.record_activation();
            }
        }

        // 1. Core Kernel Daemon (PID 1)
        spawn_proc(
            table,
            "Kernel",
            0xF0,
            kernel::sched::Priority::Safety,
            kernel::caps::CapabilitySet::ALL,
            ProcState::Running,
        );

        // 2. Radio Coordinator Service (PID 2)
        let radio_caps = CapabilitySet::empty()
            .with(kernel::caps::Cap::RadioRx)
            .with(kernel::caps::Cap::RadioTx);
        spawn_proc(
            table,
            "Radio",
            0xF1,
            kernel::sched::Priority::Radio,
            radio_caps,
            ProcState::ServiceIdle,
        );

        // 3. Power Management Service (PID 3)
        spawn_proc(
            table,
            "Power",
            0xF2,
            kernel::sched::Priority::Safety,
            CapabilitySet::empty(),
            ProcState::ServiceIdle,
        );

        // 4. Input & Keypad Daemon (PID 4)
        spawn_proc(
            table,
            "Input",
            0xF3,
            kernel::sched::Priority::Ui,
            CapabilitySet::empty(),
            ProcState::ServiceIdle,
        );

        // 5. Active Foreground Startup Application (PID 5)
        let name = crate::AppRegistry::descriptor(startup_app)
            .map(|m| m.name)
            .unwrap_or("Radio");
        let caps = crate::AppRegistry::required_caps(startup_app);
        spawn_proc(
            table,
            name,
            startup_app,
            kernel::sched::Priority::Ui,
            caps,
            ProcState::Running,
        );

        Self {
            active_app_id: startup_app,
            previous_app: None,
            nav_stack: NavStack::new(),
            owned_key: None,
            lock_mgr: KeypadLockManager::new(),
            side1_held: false,
            side2_held: false,
            is_muted: false,
            is_monitoring: false,
        }
    }

    #[inline(always)]
    pub fn proc_table(&self) -> &'static ProcessTable<MAX_PROCESSES> {
        proc_table()
    }

    #[inline(always)]
    pub fn proc_table_mut(&mut self) -> &'static mut ProcessTable<MAX_PROCESSES> {
        proc_table_mut()
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

    pub fn take_sleep_requested(&mut self) -> bool {
        let is_root = self.active_app_id == APP_ID_LAUNCHER;
        let req = self.lock_mgr.sleep_requested;
        self.lock_mgr.sleep_requested = false;
        req && is_root
    }

    pub fn reset_hold_tracking(&mut self) {
        self.lock_mgr.reset_hold_tracking();
    }

    pub fn active_app_id(&self) -> usize {
        self.active_app_id
    }

    pub fn set_startup_app(&mut self, app_id: usize) {
        if self.active_app_id != app_id {
            self.active_app_id = app_id;
            let table = proc_table_mut();
            if let Some(pcb) = table.find_mut_by_app(5) {
                let name = crate::AppRegistry::descriptor(app_id)
                    .map(|m| m.name)
                    .unwrap_or("App");
                pcb.name.clear();
                let _ = pcb.name.push_str(name);
                pcb.app_id = app_id;
            }
        }
    }

    /// Reset auto-lock inactivity countdown on external user or hardware activity.
    pub fn notify_activity(&mut self) {
        self.lock_mgr.reset_input_tracking();
    }

    fn tick_background_services<'a>(&mut self, _ctx: &mut crate::services::SysContext<'a, '_>) {
        // 1. Radio Coordinator / Dual Watch Daemon (PID 2)
        record_proc_activation(0xF1);

        // 2. Power Management & Battery Supervisor (PID 3)
        record_proc_activation(0xF2);

        // 3. System Kernel & Watch Uptime Service (PID 1)
        record_proc_activation(0xF0);
    }

    #[inline(never)]
    pub fn force_close_active_app<'a>(&mut self, ctx: &mut crate::services::SysContext<'a, '_>) {
        if self.active_app_id == APP_ID_LAUNCHER {
            return;
        }
        let closing_app = self.active_app_id;
        self.lock_mgr.reset_input_tracking();
        self.nav_stack.clear();
        self.switch_app_internal(ctx, APP_ID_LAUNCHER, false);
        if closing_app >= crate::AppRegistry::count() {
            crate::AppRegistry::unregister_dynamic();
        } else if let Some(pcb) = proc_table_mut().find_mut_by_app(closing_app) {
            pcb.set_state(ProcState::Stopped);
        }
        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Success);
    }

    fn handle_f_key_action<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        code: KeyCode,
    ) {
        self.lock_mgr.is_f_active = false;
        if code == KeyCode::Exit {
            if self.active_app_id != APP_ID_LAUNCHER {
                self.nav_stack.clear();
                self.switch_app_internal(ctx, APP_ID_LAUNCHER, false);
                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Success);
            }
            self.drain_ipc_commands(ctx);
            return;
        }

        let action = match code {
            KeyCode::Key0 => ctx.config.keys.f_0,
            KeyCode::Star => ctx.config.keys.f_star,
            KeyCode::Menu => ctx.config.keys.f_menu,
            KeyCode::Side1 => ctx.config.keys.f_side1,
            KeyCode::Side2 => ctx.config.keys.f_side2,
            _ => {
                if let Some(d) = code.to_digit().filter(|&d| d >= 1) {
                    ctx.config.keys.f_digits[(d - 1) as usize]
                } else {
                    0
                }
            }
        };
        if action != 0 {
            self.process_ipc_command(ctx, Shortcut::from_u8(action));
        } else {
            ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
        }
        self.drain_ipc_commands(ctx);
    }

    fn supervise_lock<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) -> (bool, bool) {
        let was_locked = self.lock_mgr.is_locked;
        let was_f_active = self.lock_mgr.is_f_active;

        let is_modal_active = unsafe {
            (*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL)).is_active()
        };
        let can_sleep =
            !self.lock_mgr.is_locked && self.active_app_id == APP_ID_LAUNCHER && !is_modal_active;

        let intercepted = self.lock_mgr.handle_event(
            event,
            ctx.config.keys.hold_delay_ms(),
            ctx.config.keys.f_key_timeout_ms(),
            ctx.config.keys.auto_lock_ms(),
            can_sleep,
        );

        if self.lock_mgr.force_close_requested {
            self.lock_mgr.force_close_requested = false;
            self.force_close_active_app(ctx);
            return (true, true);
        }

        let state_changed =
            was_locked != self.lock_mgr.is_locked || was_f_active != self.lock_mgr.is_f_active;

        if was_locked != self.lock_mgr.is_locked {
            if self.lock_mgr.is_locked {
                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Lock);
            } else {
                if let Some(ref mut display) = ctx.display {
                    display.wake();
                }
                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Unlock);
            }
        } else if !was_f_active && self.lock_mgr.is_f_active {
            ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Prompt);
        }

        (intercepted, state_changed)
    }

    fn update_key_ownership(&mut self, event: &Event, status: EventResult, fault_handled: bool) {
        if !fault_handled {
            if let Event::Key { code, state } = *event {
                if state == KeyState::Pressed {
                    self.owned_key = if status == EventResult::Consumed {
                        Some(code)
                    } else {
                        None
                    };
                } else if state == KeyState::Released && self.owned_key == Some(code) {
                    self.owned_key = None;
                }
            }
        }
    }

    fn handle_system_audio_events<'a>(
        &self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) {
        if let Event::Charging(true) = event {
            ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Charging);
        }
        #[cfg(feature = "usb")]
        if let Event::Usb(usb_ev) = event {
            match usb_ev {
                kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed => {
                    ctx.play_dialog_sound(crate::services::melody::DialogSoundType::UsbConnect);
                }
                kernel::event::UsbEvent::Disconnected | kernel::event::UsbEvent::Suspended => {
                    ctx.play_dialog_sound(crate::services::melody::DialogSoundType::UsbDisconnect);
                }
                _ => {}
            }
        }
        #[cfg(feature = "radio")]
        if let Event::Bus(bus_evt) = event {
            if bus_evt.topic == kernel::ipc::Topic::RADIO {
                if let Some(r_ev) = crate::services::radio::RadioEvent::from_bus_event(bus_evt) {
                    match r_ev {
                        crate::services::radio::RadioEvent::TotWarning { .. } => {
                            if ctx.config.audio.key_beep != 0 {
                                ctx.play_dialog_sound(
                                    crate::services::melody::DialogSoundType::Warning,
                                );
                            }
                        }
                        crate::services::radio::RadioEvent::TotExpired => {
                            if ctx.config.audio.key_beep != 0 {
                                ctx.play_dialog_sound(
                                    crate::services::melody::DialogSoundType::Error,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    #[inline(never)]
    pub fn dispatch_event<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: Event,
    ) -> bool {
        let prev_app = self.active_app_id;

        // Intercept and sink keys suppressed during screen/app/modal transitions
        if filter_sunk_keys(&event) {
            self.lock_mgr.reset_input_tracking();
            return false;
        }

        // 0. If an OS modal is active, modal compositor intercepts all input
        if self.handle_modal_event(ctx, &event) {
            self.lock_mgr.reset_input_tracking();
            self.drain_ipc_commands(ctx);
            return true;
        }

        // 1. Keypad Lock & Hold Supervisor (strictly precedes all apps)
        let (lock_intercepted, lock_state_changed) = self.supervise_lock(ctx, &event);
        if lock_intercepted {
            self.drain_ipc_commands(ctx);
            return true;
        }

        // When locked, any non-intercepted key event (except Ptt, Side1, Side2) must be sunk
        if self.lock_mgr.is_locked {
            if let Event::Key { code, .. } = event {
                if !matches!(code, KeyCode::Ptt | KeyCode::Side1 | KeyCode::Side2) {
                    self.drain_ipc_commands(ctx);
                    return lock_state_changed;
                }
            }
        }

        // 2. Global F-Key modifier interception
        if let Event::Key {
            code,
            state: KeyState::Pressed,
        } = event
        {
            if self.lock_mgr.is_f_active {
                self.handle_f_key_action(ctx, code);
                return true;
            }
            if code != KeyCode::Ptt && code != KeyCode::KeyF && ctx.config.audio.key_beep != 0 {
                ctx.play_beep(crate::services::melody::BeepType::from_u8(
                    ctx.config.audio.key_beep,
                ));
            }
        }

        self.handle_system_audio_events(ctx, &event);

        if matches!(event, Event::Tick) {
            self.tick_background_services(ctx);
            record_proc_activation(self.active_app_id);
        }

        // Intercept and dispatch Microkernel System & IPC Events
        if let Event::Sys(sys_evt) = event {
            if sys_evt.target == ipc_target::KERNEL {
                self.handle_kernel_sys_event(ctx, sys_evt);
                self.drain_ipc_commands(ctx);
                return true;
            } else if sys_evt.target == 0 {
                let cmd = Shortcut::from(sys_evt);
                if cmd != Shortcut::None {
                    self.process_ipc_command(ctx, cmd);
                    self.drain_ipc_commands(ctx);
                    return true;
                }
            }
        }

        // Key ownership tracking: if a key was consumed on Pressed, subsequent
        // Held/Released events for that key belong strictly to the active app.
        let is_key_owned = match event {
            Event::Key { code, .. } => self.owned_key == Some(code),
            _ => false,
        };

        // 3. Forward event to active foreground app
        let status = crate::AppRegistry::on_event(self.active_app_id, ctx, &event);
        record_proc_event_and_stack(self.active_app_id);

        // 4. Dynamic Fault & Error Interception
        let mut fault_handled = false;
        match status {
            EventResult::Error(code) | EventResult::RecoverableFault(code) => {
                fault_handled = true;
                self.handle_dynamic_fault(ctx, code);
            }
            _ => {}
        }

        // 5. Track key ownership or release
        self.update_key_ownership(&event, status, fault_handled);

        // 6. If foreground app consumed the event, owns this key, or fault was handled, it is completely sunk!
        if status == EventResult::Consumed || is_key_owned || fault_handled {
            if matches!(event, Event::Key { .. }) {
                self.lock_mgr.reset_input_tracking();
            }
        } else {
            // 7. Global navigation shortcuts & hotkeys (only if foreground app ignored the event and does not own the key)
            self.handle_global_shortcuts(ctx, event);
        }

        // Process pending Microkernel IPC System Commands
        self.drain_ipc_commands(ctx);

        status == EventResult::Consumed
            || fault_handled
            || self.active_app_id != prev_app
            || lock_state_changed
    }

    fn handle_dynamic_fault<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        code: u8,
    ) {
        if let Some(pcb) = proc_table_mut().find_mut_by_app(self.active_app_id) {
            pcb.restart_budget = pcb.restart_budget.saturating_sub(1);
            if pcb.restart_budget == 0 {
                pcb.set_state(ProcState::Stopped);
            }
        }

        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);

        let err = kernel::error::KernelError::from_code(code);
        let err_desc = err.as_str();

        unsafe {
            let modal_mut =
                &mut *core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL);
            modal_mut.show_confirm(TAG_FAULT_CONFIRM, "Fault", err_desc, "Stop", "Retry");
        }
        sink_enter_keys();
        sink_exit_key();
    }

    fn handle_modal_event<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) -> bool {
        unsafe {
            let modal_mut =
                &mut *core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL);
            if modal_mut.is_active() {
                let tag = modal_mut.tag;
                if let Some(res) = modal_mut.handle_event(event) {
                    if let Event::Key {
                        code,
                        state: KeyState::Pressed,
                    } = *event
                    {
                        sink_key(code);
                    }

                    if tag == TAG_FAULT_CONFIRM {
                        match res {
                            kernel::app::ModalResult::Confirmed(_) => {
                                // User selected Stop -> Safely stop app and return to VFO
                                if let Some(pcb) =
                                    proc_table_mut().find_mut_by_app(self.active_app_id)
                                {
                                    pcb.set_state(ProcState::Stopped);
                                }
                                self.switch_app_internal(ctx, APP_ID_VFO, false);
                            }
                            kernel::app::ModalResult::Canceled(_)
                            | kernel::app::ModalResult::Dismissed(_) => {
                                // User selected Retry -> Reset budget to 1 and restart the app cleanly
                                if let Some(pcb) =
                                    proc_table_mut().find_mut_by_app(self.active_app_id)
                                {
                                    if pcb.restart_budget == 0 {
                                        pcb.restart_budget = 1;
                                    }
                                    pcb.set_state(ProcState::Ready);
                                }
                                if self.active_app_id < crate::AppRegistry::count() {
                                    crate::AppRegistry::init_app(self.active_app_id);
                                }
                                crate::AppRegistry::on_start(self.active_app_id, ctx);
                            }
                            _ => {}
                        }
                    } else {
                        crate::AppRegistry::on_modal_result(self.active_app_id, ctx, &res);
                    }

                    if modal_mut.is_active() && modal_mut.tag == tag {
                        modal_mut.dismiss();
                    }
                }
                true
            } else {
                false
            }
        }
    }

    fn handle_global_shortcuts<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: Event,
    ) {
        match event {
            Event::Key {
                code: KeyCode::Exit,
                state: KeyState::Pressed,
            } => {
                // When already in Launcher (root launcher screen), Exit does not navigate back.
                // This ensures holding Exit to sleep does not trigger an accidental switch to VFO.
                if self.active_app_id == APP_ID_LAUNCHER {
                    return;
                }
                self.lock_mgr.reset_input_tracking();
                while let Some(prev) = self.nav_stack.pop() {
                    let is_valid = if prev < crate::AppRegistry::count() {
                        true
                    } else if prev < crate::AppRegistry::app_count() {
                        crate::AppRegistry::is_dynamic_loaded()
                    } else {
                        false
                    };
                    if prev != self.active_app_id && is_valid {
                        self.switch_app_internal(ctx, prev, false);
                        return;
                    }
                }
                if self.active_app_id != APP_ID_LAUNCHER {
                    self.switch_app_internal(ctx, APP_ID_LAUNCHER, false);
                    return;
                }
            }
            Event::Key {
                code: code @ (KeyCode::Side1 | KeyCode::Side2),
                state,
            } => {
                let (held, long, short) = if code == KeyCode::Side1 {
                    (
                        &mut self.side1_held,
                        ctx.config.keys.f1_long,
                        ctx.config.keys.f1_short,
                    )
                } else {
                    (
                        &mut self.side2_held,
                        ctx.config.keys.f2_long,
                        ctx.config.keys.f2_short,
                    )
                };
                if let Some(cfg) = Self::handle_side_key(held, state, long, short) {
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
    }

    fn handle_kernel_sys_event<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        sys_evt: kernel::ipc::SysEvent,
    ) {
        #[cfg(feature = "krx_runtime")]
        if let Some(msg) =
            crate::runtime::syscall::service::ipc_channel_mut().poll(ipc_target::KERNEL)
        {
            match msg.msg_type {
                ipc_msg_type::APP_SWITCH => {
                    if msg.payload_len > 0 {
                        let target_app = msg.payload[0] as usize;
                        self.switch_app(ctx, target_app);
                        if (msg.flags & ipc_flags::NEED_ACK) != 0 {
                            let rep = msg.reply_to(&[target_app as u8], false);
                            let _ = crate::runtime::syscall::service::ipc_channel_mut().send(rep);
                        }
                    }
                }
                ipc_msg_type::PING => {
                    if (msg.flags & ipc_flags::NEED_ACK) != 0 {
                        let rep = msg.reply_to(b"PONG", false);
                        let _ = crate::runtime::syscall::service::ipc_channel_mut().send(rep);
                    }
                }
                ipc_msg_type::DAEMON_STOP => {
                    if msg.payload_len > 0 {
                        let pid = kernel::proc::Pid(msg.payload[0]);
                        if let Some(pcb) = proc_table_mut().find_mut(pid) {
                            pcb.set_state(ProcState::Dormant);
                        }
                        if (msg.flags & ipc_flags::NEED_ACK) != 0 {
                            let rep = msg.reply_to(&[0], false);
                            let _ = crate::runtime::syscall::service::ipc_channel_mut().send(rep);
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        let cmd = Shortcut::from(sys_evt);
        if cmd != Shortcut::None {
            self.process_ipc_command(ctx, cmd);
        }
    }

    #[inline(never)]
    fn drain_ipc_commands<'a>(&mut self, ctx: &mut crate::services::SysContext<'a, '_>) {
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

    #[cfg(feature = "app_hibernation")]
    pub fn hibernate_app_state(app_id: usize) -> bool {
        if app_id >= crate::AppRegistry::app_count() {
            return false;
        }
        let size = crate::AppRegistry::app_size(app_id);
        if size == 0 {
            return false;
        }
        if let Some(ptr) = crate::AppRegistry::app_instance(app_id) {
            let slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, size) };
            if let Ok(comp_len) = crate::runtime::AppSwapManager::swap_out(app_id, slice) {
                if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
                    pcb.hibernate(comp_len);
                }
                return true;
            }
        }
        false
    }

    #[cfg(feature = "app_hibernation")]
    pub fn resume_app_state(app_id: usize) -> bool {
        if !crate::runtime::AppSwapManager::has_swap(app_id) {
            return false;
        }
        let size = crate::AppRegistry::app_size(app_id);
        if size == 0 {
            return false;
        }
        if let Some(ptr) = crate::AppRegistry::app_instance(app_id) {
            let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, size) };
            if crate::runtime::AppSwapManager::swap_in(app_id, slice).is_ok() {
                if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
                    pcb.resume();
                }
                return true;
            }
        }
        false
    }

    pub fn switch_app<'a>(&mut self, ctx: &mut crate::services::SysContext<'a, '_>, app_id: usize) {
        self.switch_app_internal(ctx, app_id, true);
    }

    fn switch_app_internal<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        app_id: usize,
        push_history: bool,
    ) {
        if app_id < crate::AppRegistry::app_count() {
            if self.active_app_id < crate::AppRegistry::app_count() {
                if let Some(pcb) = proc_table_mut().find_mut_by_app(self.active_app_id) {
                    pcb.set_state(ProcState::Suspended);
                }
                crate::AppRegistry::on_exit(self.active_app_id, ctx);
            }
            if self.active_app_id != app_id {
                if push_history && self.active_app_id != APP_ID_LAUNCHER {
                    self.nav_stack.push(self.active_app_id);

                    // Dynamic Max Foreground Apps enforcement based on live memory pressure:
                    let max_apps = kernel::sys::SystemDiagnostics::dynamic_max_foreground_apps();
                    if let Some(_evicted_app) =
                        self.nav_stack.trim_oldest(max_apps.saturating_sub(1))
                    {
                        #[cfg(feature = "app_hibernation")]
                        Self::hibernate_app_state(_evicted_app);
                    }
                }
                self.previous_app = Some(self.active_app_id);
            }
            if app_id == APP_ID_LAUNCHER {
                self.nav_stack.clear();
            }
            self.active_app_id = app_id;
            self.owned_key = None;
            self.lock_mgr.reset_input_tracking();
            sink_enter_keys();
            sink_exit_key();

            // Resume app if hibernated in swap slot
            #[cfg(feature = "app_hibernation")]
            Self::resume_app_state(app_id);

            // Track process activation in PCB table
            let table = proc_table_mut();
            let pcb = if let Some(pcb) = table.find_mut_by_app(app_id) {
                Some(pcb)
            } else {
                let name = crate::AppRegistry::descriptor(app_id)
                    .map(|m| m.name)
                    .unwrap_or("App");
                let caps = crate::AppRegistry::required_caps(app_id);
                let exec_type = if app_id >= crate::apps::AppRegistry::count() {
                    ExecType::RelocatableNative
                } else {
                    ExecType::StaticFirmware
                };
                let _ = table.spawn_or_recycle(
                    name,
                    app_id,
                    kernel::sched::Priority::Ui,
                    exec_type,
                    caps,
                );
                table.find_mut_by_app(app_id)
            };
            if let Some(pcb) = pcb {
                pcb.set_state(ProcState::Running);
                pcb.ledger.record_activation();
            }

            crate::AppRegistry::on_start(self.active_app_id, ctx);
        }
    }

    #[inline(never)]
    pub fn process_ipc_command<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
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
            Shortcut::Scanner | Shortcut::Explorer => {
                launch_krx_shortcut(ctx, "/apps/explorer.krx");
            }
            Shortcut::TaskManager => {
                launch_krx_shortcut(ctx, "/apps/sysinfo.krx");
            }
            Shortcut::Settings => self.switch_app(ctx, APP_ID_SETTINGS),
            Shortcut::LockKeypad => self.toggle_keypad_lock(),
            Shortcut::ToggleVfo => ctx.state.coord.toggle_vfo(ctx.radio),
            Shortcut::ToggleTxPower => {
                use kernel::service::radio::{RfChannel, RfPowerLevel};
                let ch = ctx.state.coord.active_channel_mut();
                ch.power = match ch.power {
                    RfPowerLevel::Low => RfPowerLevel::Medium,
                    RfPowerLevel::Medium => RfPowerLevel::High,
                    RfPowerLevel::High => RfPowerLevel::Low,
                };
                ctx.radio.set_power_level(ch.power);
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
                    ctx.state.coord.active_channel().squelch_level.clamp(1, 9)
                };
                ctx.radio.set_squelch_level(sql);
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
            Shortcut::Reboot => {
                crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
                kernel::sys::SystemDiagnostics::reboot()
            }
            Shortcut::Sleep => {
                self.lock_mgr.sleep_requested = true;
            }
            Shortcut::Fault => {
                panic!("Test panic");
            }
        }
    }

    #[inline]
    pub fn execute_shortcut<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        action: Shortcut,
    ) {
        self.process_ipc_command(ctx, action);
    }

    pub fn render<'a>(&self, canvas: &mut Canvas, ctx: &'a crate::services::SysContext<'a, '_>) {
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL)).tick(20);
        }
        OsCompositor::render(self.active_app_id, canvas, ctx, &self.lock_mgr);
    }

    pub fn is_animating(&self) -> bool {
        if crate::master_config_ref().display.ui_animations == 0 {
            return false;
        }
        if self.lock_mgr.is_animating() {
            return true;
        }
        unsafe {
            if (*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL)).is_animating()
            {
                return true;
            }
        }
        if self.active_app_id == APP_ID_LAUNCHER {
            if let Some(ptr) = crate::apps::AppRegistry::app_instance(APP_ID_LAUNCHER) {
                return unsafe { (*(ptr as *const crate::apps::LauncherApp)).is_animating() };
            }
        } else if self.active_app_id == APP_ID_SETTINGS {
            if let Some(ptr) = crate::apps::AppRegistry::app_instance(APP_ID_SETTINGS) {
                return unsafe { (*(ptr as *const crate::apps::SettingsApp)).is_animating() };
            }
        }
        false
    }
}
