// Microkernel OS Orchestrator & Shell Compositor
// Routes hardware events to the active foreground application, manages background execution,
// and orchestrates global microkernel services like IPC shortcut dispatching.

pub mod compositor;
mod key_sink;
pub mod keypad_lock;
mod procs;

pub use key_sink::*;
pub use procs::*;

/// Shared monotonic tick delta helper for compositor and lock/sleep hold timers.
pub(super) fn tick_dt_ms(last_ms: &mut u32, max_dt: u16) -> u16 {
    #[cfg(target_arch = "arm")]
    let (now, dt) = {
        let now = py32_hal::delay::millis();
        let dt = if *last_ms == 0 {
            20
        } else {
            now.wrapping_sub(*last_ms).min(max_dt as u32) as u16
        };
        (now, dt)
    };
    #[cfg(not(target_arch = "arm"))]
    let (now, dt) = (last_ms.wrapping_add(50), 50u16.min(max_dt));
    *last_ms = now;
    dt
}

#[cfg(not(test))]
use crate::apps::APP_ID_SETTINGS;
#[cfg(not(test))]
use crate::apps::APP_ID_VFO;
use crate::apps::{APP_ID_LAUNCHER, Shortcut};
#[cfg(test)]
use ::abi::CapabilitySet;
use compositor::OsCompositor;
use gfx::Canvas;
use kernel::app::EventResult;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::ipc::ipc_target;
#[cfg(feature = "krx_native")]
use kernel::ipc::{ipc_flags, ipc_msg_type};
use kernel::nav::NavStack;
use kernel::proc::{ExecType, ProcState, ProcessTable};
use keypad_lock::KeypadLockManager;

#[repr(C)]
pub struct Orchestrator {
    side1_held: bool,
    side2_held: bool,
    owned_key: Option<KeyCode>,
    active_app_id: usize,
    previous_app: Option<usize>,
    lock_mgr: KeypadLockManager,
    nav_stack: NavStack<6>,
}

pub(crate) fn launch_krx_shortcut_arg(path: &str, action_id: u32) -> bool {
    let mut path_copy = [0u8; 32];
    let pb = path.as_bytes();
    let plen = pb.len().min(path_copy.len());
    path_copy[..plen].copy_from_slice(&pb[..plen]);
    let safe_path = unsafe { core::str::from_utf8_unchecked(&path_copy[..plen]) };
    if let Err(err) = crate::runtime::launch_krx_package(safe_path, action_id) {
        crate::runtime::show_launch_error_dialog(safe_path, err);
        crate::services::play_dialog_sound(crate::services::melody::DialogSoundType::Error);
        return false;
    }
    true
}

#[cfg(feature = "filesystem")]
#[inline(always)]
pub(crate) fn launch_krx_shortcut(path: &str) -> bool {
    launch_krx_shortcut_arg(path, 0)
}

impl Orchestrator {
    /// Initialize an `Orchestrator` directly in caller storage without a stack temporary.
    pub unsafe fn init_in_place(dst: *mut Self, startup_app: usize) {
        init_proc_table();
        let table = proc_table_mut();

        // Spawn only real resident/foreground application actors (zero fake static kernel PCBs).
        let name = crate::AppRegistry::descriptor(startup_app)
            .map(|m| m.name)
            .unwrap_or("Home");
        let caps = crate::AppRegistry::required_caps(startup_app);
        if table
            .spawn(
                name,
                startup_app,
                kernel::sched::Priority::Ui,
                ExecType::StaticFirmware,
                caps,
            )
            .is_ok()
            && let Some(pcb) = table.find_mut_by_app(startup_app)
        {
            pcb.set_state(ProcState::Running);
        }

        unsafe {
            core::ptr::write_bytes(dst as *mut u8, 0, core::mem::size_of::<Self>());
            core::ptr::write(core::ptr::addr_of_mut!((*dst).active_app_id), startup_app);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).previous_app), None);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).nav_stack), NavStack::new());
            core::ptr::write(core::ptr::addr_of_mut!((*dst).owned_key), None);
            core::ptr::write(
                core::ptr::addr_of_mut!((*dst).lock_mgr),
                KeypadLockManager::new(),
            );
        }
    }

    pub fn new(startup_app: usize) -> Self {
        let mut uninit = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_in_place(uninit.as_mut_ptr(), startup_app);
            uninit.assume_init()
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

    #[inline(always)]
    pub fn is_lock_overlay_active(&self) -> bool {
        self.lock_mgr.is_overlay_active()
    }

    #[inline(always)]
    pub fn is_lock_overlay_animating(&self) -> bool {
        self.lock_mgr.is_animating()
    }

    pub fn is_f_active(&self) -> bool {
        self.lock_mgr.is_f_active
    }

    pub fn toggle_keypad_lock(&mut self) {
        self.lock_mgr.toggle_lock();
    }

    pub fn take_sleep_requested(&mut self) -> bool {
        let req = self.lock_mgr.sleep_requested;
        self.lock_mgr.sleep_requested = false;
        req
    }

    pub fn reset_hold_tracking(&mut self) {
        self.lock_mgr.reset_hold_tracking();
    }

    pub fn active_app_id(&self) -> usize {
        self.active_app_id
    }

    pub fn set_startup_app(&mut self, app_id: usize) {
        if self.active_app_id != app_id {
            let prev = self.active_app_id;
            self.active_app_id = app_id;
            let table = proc_table_mut();
            if let Some(pcb) = table.find_mut_by_app(prev) {
                let name = crate::AppRegistry::descriptor(app_id)
                    .map(|m| m.name)
                    .unwrap_or("App");
                pcb.name.clear();
                let _ = pcb.name.push_str(name);
                pcb.app_id = app_id as u8;
            }
        }
    }

    /// Reset auto-lock inactivity countdown on external user or hardware activity.
    pub fn notify_activity(&mut self) {
        self.lock_mgr.reset_input_tracking();
    }

    fn tick_background_services(&mut self) {
        #[cfg(feature = "krx_native")]
        crate::runtime::background::BgManager::tick_system(py32_hal::delay::millis());
    }

    fn quench_app_radio(app_id: usize) {
        let (state, radio) = crate::services::state_and_radio();
        if state.coord.is_transmitting {
            state.coord.request_ptt_stop(radio);
        }
        if app_id >= crate::AppRegistry::count()
            && let Some(pcb) = proc_table().find_by_app(app_id)
        {
            let cid = kernel::service::radio::ClientId::for_krx_pid(pcb.pid.0);
            let _ = state.coord.release_client_lock(radio, cid);
        }
    }

    fn reap_dynamic_app(app_id: usize) {
        if let Some(pcb) = proc_table().find_by_app(app_id) {
            reap_pid_only(pcb.pid);
        }
        crate::AppRegistry::unregister_dynamic();
    }

    pub fn force_close_active_app(&mut self) {
        if self.active_app_id == APP_ID_LAUNCHER {
            return;
        }
        let closing_app = self.active_app_id;
        Self::quench_app_radio(closing_app);
        self.lock_mgr.reset_input_tracking();
        self.nav_stack.clear();
        self.switch_app_internal(APP_ID_LAUNCHER, false);
        if closing_app >= crate::AppRegistry::count() {
            Self::reap_dynamic_app(closing_app);
        } else if let Some(pcb) = proc_table_mut().find_mut_by_app(closing_app) {
            pcb.set_state(ProcState::Stopped);
        }
        crate::services::play_dialog_sound(crate::services::melody::DialogSoundType::Success);
    }
}

enum FilterFlow {
    Continue,
    DirtyContinue,
    Stop(bool),
}

impl Orchestrator {
    fn handle_f_key_action(&mut self, code: KeyCode) {
        self.lock_mgr.clear_f_active();
        if code == KeyCode::Exit {
            if self.active_app_id != APP_ID_LAUNCHER {
                self.nav_stack.clear();
                self.switch_app_internal(APP_ID_LAUNCHER, false);
                crate::services::play_dialog_sound(
                    crate::services::melody::DialogSoundType::Success,
                );
            }
            return;
        }

        let slot = match code {
            KeyCode::Key0 => Some(14),
            KeyCode::Star => Some(15),
            KeyCode::Menu => Some(16),
            KeyCode::Side1 => Some(17),
            KeyCode::Side2 => Some(18),
            _ => code
                .to_digit()
                .filter(|&d| d >= 1)
                .map(|d| 5 + (d - 1) as usize),
        };
        let cur_ctx = crate::apps::shortcut::active_context(self.active_app_id);
        let target_id = slot.map_or(0, |i| {
            crate::apps::shortcut::resolve_slot(&crate::services::config().keys, i, cur_ctx)
        });
        self.execute_shortcut_target(target_id, true);
    }

    pub fn execute_shortcut_target(&mut self, target_id: u16, warn_if_empty: bool) -> bool {
        let cat = crate::apps::launcher::catalog::get_catalog();
        #[cfg(feature = "filesystem")]
        if (target_id & 0x8000) != 0 && !cat.scanned {
            cat.scan(crate::services::storage_mut());
        }
        match crate::apps::shortcut::resolve(target_id, cat) {
            crate::apps::shortcut::ResolvedShortcut::Builtin(cmd) => {
                self.process_ipc_command(cmd);
                true
            }
            crate::apps::shortcut::ResolvedShortcut::App { path, action_id } => {
                #[cfg(feature = "krx_native")]
                if self.active_app_id >= crate::AppRegistry::count()
                    && crate::runtime::native_runner::active_pkg_matches(path)
                {
                    if action_id != 0 {
                        crate::runtime::native_runner::dispatch_action(action_id);
                    }
                    return true;
                }
                launch_krx_shortcut_arg(path, action_id as u32)
            }
            crate::apps::shortcut::ResolvedShortcut::None => {
                if warn_if_empty {
                    crate::services::play_dialog_sound(
                        crate::services::melody::DialogSoundType::Warning,
                    );
                }
                false
            }
        }
    }

    #[inline(never)]
    fn filter_keypad_lock(&mut self, event: &Event) -> FilterFlow {
        let was_locked = self.lock_mgr.is_locked;
        let was_f_active = self.lock_mgr.is_f_active;
        let was_f_drawer_animating = self.lock_mgr.is_f_drawer_animating();

        let is_modal_active = crate::orchestrator::compositor::active_modal_mut().is_active();
        let can_sleep =
            !self.lock_mgr.is_locked && self.active_app_id == APP_ID_LAUNCHER && !is_modal_active;

        let cfg = crate::services::config();
        let intercepted = self.lock_mgr.handle_event(
            event,
            cfg.keys.hold_delay_ms(),
            cfg.keys.f_key_timeout_ms(),
            cfg.keys.auto_lock_ms(),
            can_sleep,
        );

        if cfg.keys.f_drawer == 0 {
            self.lock_mgr.snap_f_drawer(true);
        } else if !cfg.display.animations_enabled() {
            self.lock_mgr.snap_f_drawer(false);
        }

        if self.lock_mgr.force_close_requested {
            self.lock_mgr.force_close_requested = false;
            self.force_close_active_app();
            self.lock_mgr.reset_hold_tracking();
            return FilterFlow::Stop(true);
        }

        let state_changed = was_locked != self.lock_mgr.is_locked
            || was_f_active != self.lock_mgr.is_f_active
            || was_f_drawer_animating
            || self.lock_mgr.is_f_drawer_animating();

        if was_locked != self.lock_mgr.is_locked {
            if self.lock_mgr.is_locked {
                crate::services::play_dialog_sound(crate::services::melody::DialogSoundType::Lock);
            } else {
                crate::services::wake_display();
                crate::services::play_dialog_sound(
                    crate::services::melody::DialogSoundType::Unlock,
                );
            }
        } else if !was_f_active && self.lock_mgr.is_f_active {
            crate::services::play_dialog_sound(crate::services::melody::DialogSoundType::Prompt);
        }

        if intercepted {
            return FilterFlow::Stop(true);
        }

        // When locked, any non-intercepted key event (except Ptt, Side1, Side2) must be sunk
        if self.lock_mgr.is_locked {
            if let Event::Key { code, .. } = *event {
                if !matches!(code, KeyCode::Ptt | KeyCode::Side1 | KeyCode::Side2) {
                    return FilterFlow::Stop(state_changed);
                }
            }
        }

        if state_changed {
            FilterFlow::DirtyContinue
        } else {
            FilterFlow::Continue
        }
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

    fn handle_system_audio_events(&self, event: &Event) {
        use crate::services::melody::DialogSoundType;
        let sound = match event {
            Event::Charging(true) => Some(DialogSoundType::Charging),
            #[cfg(feature = "usb")]
            Event::Usb(usb_ev) => match usb_ev {
                kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed => {
                    Some(DialogSoundType::UsbConnect)
                }
                kernel::event::UsbEvent::Disconnected | kernel::event::UsbEvent::Suspended => {
                    Some(DialogSoundType::UsbDisconnect)
                }
                _ => None,
            },
            #[cfg(feature = "radio")]
            Event::Bus(bus_evt)
                if bus_evt.topic == kernel::ipc::Topic::RADIO
                    && crate::services::config().audio.key_beep != 0 =>
            {
                match crate::services::radio::RadioEvent::from_bus_event(bus_evt) {
                    Some(crate::services::radio::RadioEvent::TotWarning { .. }) => {
                        Some(DialogSoundType::Warning)
                    }
                    Some(crate::services::radio::RadioEvent::TotExpired) => {
                        Some(DialogSoundType::Error)
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(s) = sound {
            crate::services::play_dialog_sound(s);
        }
    }

    fn filter_hotkeys_and_system(&mut self, event: &Event) -> FilterFlow {
        if let Event::Key {
            code,
            state: KeyState::Pressed,
        } = *event
        {
            if self.lock_mgr.is_f_active {
                self.handle_f_key_action(code);
                return FilterFlow::Stop(true);
            }
            let key_beep = crate::services::config().audio.key_beep;
            if code != KeyCode::Ptt && code != KeyCode::KeyF && key_beep != 0 {
                crate::services::play_beep(crate::services::melody::BeepType::from_u8(key_beep));
            }
        }

        self.handle_system_audio_events(event);

        if matches!(event, Event::Tick) {
            self.tick_background_services();
        }

        if let Event::Sys(sys_evt) = *event {
            if self.handle_kernel_sys_event(sys_evt) {
                return FilterFlow::Stop(true);
            }
        }

        FilterFlow::Continue
    }

    pub fn dispatch_event(&mut self, event: Event) -> bool {
        if filter_sunk_keys(&event) {
            self.lock_mgr.reset_input_tracking();
            return false;
        }
        let mut handled = false;
        if matches!(event, Event::Tick)
            && crate::orchestrator::compositor::active_modal_mut()
                .tick(gfx::views::repeater::tick_dt_ms())
        {
            handled = true;
        }
        macro_rules! run_step {
            ($step:expr) => {
                match $step {
                    FilterFlow::Continue => false,
                    FilterFlow::DirtyContinue => {
                        handled = true;
                        false
                    }
                    FilterFlow::Stop(res) => {
                        handled |= res;
                        true
                    }
                }
            };
        }
        let _ = run_step!(self.filter_modal_event(&event))
            || run_step!(self.filter_keypad_lock(&event))
            || run_step!(self.filter_hotkeys_and_system(&event))
            || run_step!(self.filter_active_app(&event));
        handled
    }

    fn filter_active_app(&mut self, event: &Event) -> FilterFlow {
        let prev_app = self.active_app_id;

        let is_key = matches!(*event, Event::Key { .. });
        let is_key_owned = match *event {
            Event::Key { code, .. } => self.owned_key == Some(code),
            _ => false,
        };

        // Forward event to active foreground app
        let status = crate::AppRegistry::on_event(self.active_app_id, event);
        record_proc_event_and_stack(self.active_app_id);

        // Dynamic Fault & Error Interception
        let mut fault_handled = false;
        match status {
            EventResult::Error(code) | EventResult::RecoverableFault(code) => {
                fault_handled = true;
                self.handle_dynamic_fault(code);
            }
            _ => {}
        }

        // Track key ownership or release
        self.update_key_ownership(event, status, fault_handled);

        // If foreground app consumed the event, owns this key, or fault was handled, it is sunk
        if status == EventResult::Consumed || is_key_owned || fault_handled {
            if is_key {
                self.lock_mgr.reset_input_tracking();
            }
            return FilterFlow::Stop(
                status == EventResult::Consumed || fault_handled || self.active_app_id != prev_app,
            );
        }

        // Global navigation shortcuts & hotkeys (only if foreground app ignored the event and does not own the key)
        self.handle_global_shortcuts(*event);
        FilterFlow::Stop(self.active_app_id != prev_app)
    }

    #[cold]
    fn handle_dynamic_fault(&mut self, code: u8) {
        Self::quench_app_radio(self.active_app_id);
        if let Some(pcb) = proc_table_mut().find_mut_by_app(self.active_app_id) {
            pcb.restart_budget = pcb.restart_budget.saturating_sub(1);
            if pcb.restart_budget == 0 {
                pcb.set_state(ProcState::Stopped);
            }
        }

        crate::services::play_dialog_sound(crate::services::melody::DialogSoundType::Error);

        let err = kernel::error::KernelError::from_code(code);
        let err_desc = err.as_str();

        crate::orchestrator::compositor::active_modal_mut().show_confirm(
            TAG_FAULT_CONFIRM,
            "App Unresponsive",
            err_desc,
            "Retry",
            "Close",
        );
        sink_modal_keys();
    }

    fn pop_active_app(&mut self) {
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
                self.switch_app_internal(prev, false);
                return;
            }
        }
        self.switch_app_internal(APP_ID_LAUNCHER, false);
    }

    #[inline(never)]
    fn filter_modal_event(&mut self, event: &Event) -> FilterFlow {
        unsafe {
            let modal_mut = crate::orchestrator::compositor::active_modal_mut();
            if !matches!(
                modal_mut.modal,
                crate::orchestrator::compositor::ActiveModal::None
            ) {
                if let Event::Key {
                    code: KeyCode::Ptt,
                    state,
                } = *event
                {
                    if state == KeyState::Released {
                        crate::services::handle_system_ptt(KeyState::Released);
                    } else {
                        sink_key(KeyCode::Ptt);
                    }
                }
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
                        let is_confirmed = matches!(res, kernel::app::ModalResult::Confirmed(_));
                        modal_mut.dismiss();
                        #[cfg(feature = "krx_native")]
                        if let Some(pkg) = crate::runtime::fault::fault_pkg_path() {
                            let mut path_buf = [0u8; 32];
                            let n = pkg.len().min(32);
                            path_buf[..n].copy_from_slice(&pkg.as_bytes()[..n]);
                            crate::runtime::fault::clear_fault_pkg();
                            let retried = is_confirmed
                                && launch_krx_shortcut_arg(
                                    core::str::from_utf8_unchecked(&path_buf[..n]),
                                    0,
                                );
                            if !retried {
                                self.switch_app_internal(APP_ID_LAUNCHER, false);
                            }
                        } else {
                            self.switch_app_internal(APP_ID_LAUNCHER, false);
                        }
                        #[cfg(not(feature = "krx_native"))]
                        {
                            let _ = is_confirmed;
                            self.switch_app_internal(APP_ID_LAUNCHER, false);
                        }
                    } else if tag != crate::orchestrator::compositor::TAG_SYSTEM_ALERT {
                        crate::AppRegistry::on_modal_result(self.active_app_id, &res);
                    }

                    if !matches!(
                        modal_mut.modal,
                        crate::orchestrator::compositor::ActiveModal::None
                    ) && modal_mut.tag == tag
                    {
                        modal_mut.dismiss();
                    }

                    if tag == crate::orchestrator::compositor::TAG_SYSTEM_ALERT
                        && self.active_app_id == APP_ID_LAUNCHER
                    {
                        crate::orchestrator::compositor::present_next_system_alert();
                    }
                }
                self.lock_mgr.reset_input_tracking();
                FilterFlow::Stop(true)
            } else {
                FilterFlow::Continue
            }
        }
    }

    fn handle_global_shortcuts(&mut self, event: Event) {
        let cur_ctx = crate::apps::shortcut::active_context(self.active_app_id);
        let mut fire_slot: Option<u16> = None;
        match event {
            Event::Key {
                code: KeyCode::Ptt,
                state,
            } => {
                crate::services::handle_system_ptt(state);
                return;
            }
            Event::Key {
                code: KeyCode::Exit,
                state: KeyState::Pressed,
            } => {
                if self.active_app_id != APP_ID_LAUNCHER {
                    self.pop_active_app();
                }
                return;
            }
            Event::Key {
                code: code @ (KeyCode::Side1 | KeyCode::Side2),
                state,
            } => {
                let (held, base_slot) = if code == KeyCode::Side1 {
                    (&mut self.side1_held, 0usize)
                } else {
                    (&mut self.side2_held, 2usize)
                };
                let keys = &crate::services::config().keys;
                let long = crate::apps::shortcut::resolve_slot(keys, base_slot + 1, cur_ctx);
                let short = crate::apps::shortcut::resolve_slot(keys, base_slot, cur_ctx);
                fire_slot = Self::handle_side_key(held, state, long, short);
            }
            Event::Key {
                code: KeyCode::Menu,
                state: KeyState::Held,
            } => {
                fire_slot = Some(crate::apps::shortcut::resolve_slot(
                    &crate::services::config().keys,
                    4,
                    cur_ctx,
                ));
            }
            _ => {}
        }
        if let Some(target_id) = fire_slot {
            self.execute_shortcut_target(target_id, false);
        }
    }

    #[inline(never)]
    fn handle_kernel_sys_event(&mut self, sys_evt: kernel::ipc::SysEvent) -> bool {
        if sys_evt.target == ipc_target::KERNEL {
            #[cfg(feature = "krx_native")]
            {
                let mut msg = kernel::ipc::KrxIpcMessage::empty();
                if crate::runtime::syscall::service::ipc_channel_mut()
                    .poll_into(ipc_target::KERNEL, &mut msg)
                {
                    let mut ack_byte = [0u8; 1];
                    let ack: Option<&[u8]> = match msg.msg_type {
                        ipc_msg_type::APP_SWITCH if msg.payload_len > 0 => {
                            let target_app = msg.payload[0] as usize;
                            self.switch_app(target_app);
                            ack_byte[0] = target_app as u8;
                            Some(&ack_byte)
                        }
                        ipc_msg_type::PING => Some(b"PONG"),
                        ipc_msg_type::DAEMON_STOP if msg.payload_len > 0 => {
                            let pid = kernel::proc::Pid(msg.payload[0]);
                            crate::runtime::background::BgManager::stop_pid(pid);
                            if let Some(pcb) = proc_table_mut().find_mut(pid) {
                                pcb.set_state(ProcState::Dormant);
                            }
                            Some(&ack_byte)
                        }
                        _ => None,
                    };
                    if let Some(reply_payload) = ack
                        && (msg.flags & ipc_flags::NEED_ACK) != 0
                    {
                        msg.make_reply_in_place(reply_payload, false);
                        let _ = crate::runtime::syscall::service::ipc_channel_mut()
                            .send_from(&msg, msg.sender_pid);
                    }
                    return true;
                }
            }
        } else if sys_evt.target != 0 {
            return false;
        }

        let cmd = Shortcut::from(sys_evt);
        if cmd != Shortcut::None {
            self.process_ipc_command(cmd);
            return true;
        }
        false
    }

    fn handle_side_key(
        held_flag: &mut bool,
        state: KeyState,
        long_cfg: u16,
        short_cfg: u16,
    ) -> Option<u16> {
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
    fn swap_app_state(
        app_id: usize,
        storage: Option<&mut crate::services::SystemStorage>,
        is_out: bool,
    ) -> bool {
        if app_id != APP_ID_LAUNCHER {
            return false;
        }
        let ptr = crate::apps::AppRegistry::launcher_mut() as *mut crate::apps::LauncherApp;
        let size = core::mem::size_of::<crate::apps::LauncherApp>();
        if is_out {
            let slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, size) };
            if let Ok(comp_len) = crate::runtime::AppSwapManager::swap_out_tiered(
                app_id,
                slice,
                storage,
                crate::runtime::SwapPolicy::RamThenFlash,
            ) {
                if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
                    pcb.hibernate(comp_len);
                }
                return true;
            }
        } else {
            let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, size) };
            if crate::runtime::AppSwapManager::swap_in_tiered(app_id, slice, storage).is_ok() {
                if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
                    pcb.resume();
                }
                return true;
            }
        }
        false
    }

    #[cfg(feature = "app_hibernation")]
    #[inline(always)]
    pub fn hibernate_app_state(
        app_id: usize,
        storage: Option<&mut crate::services::SystemStorage>,
    ) -> bool {
        Self::swap_app_state(app_id, storage, true)
    }

    #[cfg(feature = "app_hibernation")]
    #[inline(always)]
    pub fn resume_app_state(
        app_id: usize,
        storage: Option<&mut crate::services::SystemStorage>,
    ) -> bool {
        Self::swap_app_state(app_id, storage, false)
    }

    pub fn switch_app(&mut self, app_id: usize) {
        self.switch_app_internal(app_id, true);
    }

    #[inline(never)]
    fn switch_app_internal(&mut self, app_id: usize, push_history: bool) {
        if app_id < crate::AppRegistry::app_count() {
            if self.active_app_id < crate::AppRegistry::app_count() {
                let is_dyn_reload =
                    self.active_app_id == app_id && app_id >= crate::apps::AppRegistry::count();
                if !is_dyn_reload {
                    if let Some(pcb) = proc_table_mut().find_mut_by_app(self.active_app_id) {
                        pcb.set_state(ProcState::Suspended);
                    }
                    crate::AppRegistry::on_exit(self.active_app_id);
                    if self.active_app_id >= crate::apps::AppRegistry::count() {
                        Self::reap_dynamic_app(self.active_app_id);
                    }
                }
            }
            if self.active_app_id != app_id {
                if push_history && self.active_app_id != APP_ID_LAUNCHER {
                    self.nav_stack.push(self.active_app_id);

                    // Dynamic Max Foreground Apps & RAM cache trimming
                    let max_apps = kernel::sys::SystemDiagnostics::dynamic_max_foreground_apps();
                    if let Some(_evicted_app) =
                        self.nav_stack.trim_oldest(max_apps.saturating_sub(1))
                    {
                        #[cfg(feature = "app_hibernation")]
                        Self::hibernate_app_state(
                            _evicted_app,
                            Some(crate::services::storage_mut()),
                        );
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
            self.lock_mgr.reset_hold_tracking();
            sink_modal_keys();

            // Resume app if hibernated in Tier-1 RAM or Tier-2 KernFS swap
            #[cfg(feature = "app_hibernation")]
            Self::resume_app_state(app_id, Some(crate::services::storage_mut()));

            // Track process activation in PCB table
            let table = proc_table_mut();
            if table.find_mut_by_app(app_id).is_none() {
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
            }
            if let Some(pcb) = table.find_mut_by_app(app_id) {
                pcb.set_state(ProcState::Running);
            }

            crate::AppRegistry::on_start(self.active_app_id);
        }
    }

    pub fn process_ipc_command(&mut self, cmd: Shortcut) {
        match cmd {
            Shortcut::None => {}
            Shortcut::LaunchApp(id) => {
                #[cfg(all(not(test), feature = "filesystem", feature = "krx_native"))]
                {
                    let pkg_path = match id as usize {
                        APP_ID_VFO => Some("/apps/vfo.krx"),
                        APP_ID_SETTINGS => Some("/apps/settings.krx"),
                        _ => None,
                    };
                    if let Some(path) = pkg_path {
                        let _ = launch_krx_shortcut(path);
                        return;
                    }
                }
                self.switch_app(id as usize);
            }
            Shortcut::CloseApp => self.pop_active_app(),
            Shortcut::LaunchPendingPkg => {
                #[cfg(all(feature = "filesystem", feature = "krx_native"))]
                if let Some(path) = crate::runtime::syscall::service::take_pending_launch_path() {
                    let _ = launch_krx_shortcut(path);
                }
            }
            Shortcut::Flashlight => {
                if bsp::pins::FLASHLIGHT.is_high() {
                    bsp::pins::FLASHLIGHT.set_low();
                } else {
                    bsp::pins::FLASHLIGHT.set_high();
                }
            }
            Shortcut::LockKeypad => self.toggle_keypad_lock(),
            Shortcut::Sleep => {
                if !crate::system::is_headless() {
                    self.lock_mgr.sleep_requested = true;
                }
            }
        }
    }

    #[inline]
    pub fn execute_shortcut(&mut self, action: Shortcut) {
        self.process_ipc_command(action);
    }

    pub fn render(&self, canvas: &mut Canvas) {
        OsCompositor::render(self.active_app_id, canvas, &self.lock_mgr);
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
            return crate::apps::AppRegistry::launcher_ref().is_animating();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_startup_app_updates_active_and_pcb() {
        let mut orch = Orchestrator::new(crate::apps::APP_ID_LAUNCHER);
        assert_eq!(orch.active_app_id(), crate::apps::APP_ID_LAUNCHER);

        orch.set_startup_app(crate::apps::APP_ID_VFO);
        assert_eq!(orch.active_app_id(), crate::apps::APP_ID_VFO);

        let table = orch.proc_table();
        assert!(table.find_by_app(crate::apps::APP_ID_LAUNCHER).is_none());
        let pcb = table
            .find_by_app(crate::apps::APP_ID_VFO)
            .expect("startup app PCB updated to new app_id");
        assert_eq!(pcb.app_id as usize, crate::apps::APP_ID_VFO);
    }

    #[test]
    fn init_in_place_initializes_active_app_without_corruption() {
        let mut uninit = core::mem::MaybeUninit::<Orchestrator>::uninit();
        unsafe {
            Orchestrator::init_in_place(uninit.as_mut_ptr(), crate::apps::APP_ID_SETTINGS);
        }
        let orch = unsafe { uninit.assume_init() };
        assert_eq!(orch.active_app_id(), crate::apps::APP_ID_SETTINGS);
    }

    #[test]
    fn reap_pid_resources_protects_core_daemons_and_cleans_user_process() {
        // Shares the global process table: keep PID lifecycle assertions in this one test.
        let mut orch = Orchestrator::new(crate::apps::APP_ID_LAUNCHER);
        // Zero fake static PCBs: only the resident Home launcher is initially spawned at PID 1.
        assert_eq!(orch.proc_table().active_count(), 1);
        let launcher_pid = orch
            .proc_table()
            .find_by_app(crate::apps::APP_ID_LAUNCHER)
            .expect("Home launcher PCB")
            .pid;
        assert!(!reap_pid_resources(0));
        assert!(!reap_pid_resources(launcher_pid.0));

        // Spawn user KRX process with an active lease.
        let pid = orch
            .proc_table_mut()
            .spawn(
                "Worker",
                77,
                kernel::sched::Priority::Ui,
                ExecType::RelocatableNative,
                CapabilitySet::empty(),
            )
            .expect("worker slot");
        assert!(pid.0 >= 2);
        assert!(
            lease_manager_mut()
                .acquire(pid, kernel::proc::LeaseKind::RadioRx, 1000, 0)
                .is_ok()
        );

        assert!(reap_pid_resources(pid.0));
        assert!(
            lease_manager()
                .holder(kernel::proc::LeaseKind::RadioRx)
                .is_none()
        );
        assert_eq!(orch.proc_table().find(pid).unwrap().state, ProcState::Dead);

        // Dynamic (KRX) apps get their Pid from the process table, never from the app index.
        let dyn_app = crate::AppRegistry::count();
        let dyn_pid = orch
            .proc_table_mut()
            .spawn_or_recycle(
                "Krx",
                dyn_app,
                kernel::sched::Priority::Ui,
                ExecType::RelocatableNative,
                CapabilitySet::empty(),
            )
            .expect("dynamic slot");
        assert!(dyn_pid.0 >= 2);
        assert_eq!(orch.proc_table().find_by_app(dyn_app).unwrap().pid, dyn_pid);

        // Transient modules run as Pid(0), which owns nothing: reaping it leaves processes intact.
        let active_before = orch.proc_table().active_count();
        reap_pid_only(kernel::proc::Pid(0));
        assert_eq!(orch.proc_table().active_count(), active_before);

        reap_pid_only(dyn_pid);
        assert_eq!(
            orch.proc_table().find(dyn_pid).unwrap().state,
            ProcState::Dead
        );
    }
}
