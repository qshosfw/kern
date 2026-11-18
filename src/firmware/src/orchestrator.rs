use crate::services::display::DisplayService;
// Microkernel OS Orchestrator & Shell Compositor
// Routes hardware events to the active foreground application, manages background execution,
// and orchestrates global microkernel services like IPC shortcut dispatching.

pub mod compositor;
pub mod keypad_lock;

/// Shared monotonic tick delta helper for compositor and lock/sleep hold timers.
#[inline(never)]
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

use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use compositor::OsCompositor;
use gfx::Canvas;
use kernel::app::EventResult;
use kernel::caps::CapabilitySet;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::ipc::ipc_target;
#[cfg(feature = "krx_runtime")]
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

#[inline(never)]
pub fn init_proc_table() {
    ProcessTable::init_in_place(unsafe { &mut *core::ptr::addr_of_mut!(PROC_TABLE) });
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

pub fn sink_modal_keys() {
    unsafe {
        SUNK_KEYS |= BIT_MENU | BIT_KEY5 | BIT_EXIT;
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

#[inline(never)]
pub fn record_proc_activation(app_id: usize) {
    if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
        pcb.ledger.record_activation();
    }
}

/// Returns true if process `app_id` is currently suspended, dormant, stopped, or dead.
#[inline(never)]
pub fn is_proc_paused(app_id: usize) -> bool {
    proc_table().find_by_app(app_id).is_some_and(|pcb| {
        matches!(
            pcb.state,
            ProcState::Suspended | ProcState::Dormant | ProcState::Stopped | ProcState::Dead
        )
    })
}

pub(crate) fn reap_pid_only(pid: kernel::proc::Pid) {
    lease_manager_mut().revoke_all_for_owner(pid);
    status_tray_mut().clear_icon(pid);
    #[cfg(feature = "krx_runtime")]
    crate::runtime::syscall::service::service_registry_mut().unbind_provider(pid);
    let _ = proc_table_mut().terminate(pid);
}

/// Atomically reap a terminated non-protected process across PCB, Leases, Tray, Services, and dynamic registry.
pub fn reap_pid_resources(target_pid: u8) -> bool {
    if target_pid <= 4 {
        return false;
    }
    let Some(pcb) = proc_table().find(kernel::proc::Pid(target_pid)) else {
        return false;
    };
    let app_id = pcb.app_id;
    let pid = pcb.pid;

    reap_pid_only(pid);
    if app_id == crate::AppRegistry::count() {
        crate::AppRegistry::unregister_dynamic();
    }
    true
}

#[inline(never)]
fn record_proc_event_and_stack(app_id: usize) {
    if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
        pcb.ledger.record_event();
        let used_stk = kernel::sys::SystemDiagnostics::stack_used_now() as u16;
        pcb.ledger.record_stack_depth(used_stk);
    }
}

#[inline(never)]
pub(crate) fn launch_krx_shortcut_arg(
    ctx: &mut crate::services::SysContext,
    path: &str,
    action_id: u32,
) -> bool {
    if let Err(err) = crate::apps::runtime::launch_krx_package_with_arg(ctx, path, action_id) {
        crate::runtime::loader::show_launch_error_dialog(ctx, path, err);
        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
        return false;
    }
    true
}

#[inline(never)]
pub(crate) fn launch_krx_shortcut(ctx: &mut crate::services::SysContext, path: &str) -> bool {
    launch_krx_shortcut_arg(ctx, path, 0)
}

impl Orchestrator {
    /// Initialize an `Orchestrator` directly in caller storage without a stack temporary.
    pub unsafe fn init_in_place(dst: *mut Self, startup_app: usize) {
        init_proc_table();
        let table = proc_table_mut();

        #[inline(never)]
        fn spawn_proc(
            table: &mut ProcessTable<MAX_PROCESSES>,
            name: &str,
            app_id: usize,
            priority: kernel::sched::Priority,
            caps: CapabilitySet,
            state: ProcState,
        ) {
            if table
                .spawn(name, app_id, priority, ExecType::StaticFirmware, caps)
                .is_ok()
            {
                if let Some(pcb) = table.find_mut_by_app(app_id) {
                    pcb.set_state(state);
                    pcb.ledger.record_activation();
                }
            }
        }

        const CORE_NAMES: &[u8] = b"Kernel\0Radio\0Power\0Input\0";
        const CORE_PRIOS: [kernel::sched::Priority; 4] = [
            kernel::sched::Priority::Safety,
            kernel::sched::Priority::Radio,
            kernel::sched::Priority::Safety,
            kernel::sched::Priority::Ui,
        ];
        let radio_caps = CapabilitySet::empty()
            .with(kernel::caps::Cap::RadioRx)
            .with(kernel::caps::Cap::RadioTx)
            .raw();
        for i in 0..4u8 {
            let caps_raw = match i {
                0 => u32::MAX,
                1 => radio_caps,
                _ => 0,
            };
            spawn_proc(
                table,
                utils::format::nth_cstr(CORE_NAMES, i),
                (0xF0 + i) as usize,
                CORE_PRIOS[i as usize],
                CapabilitySet::from_raw(caps_raw),
                if i == 0 {
                    ProcState::Running
                } else {
                    ProcState::ServiceIdle
                },
            );
        }

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
            if let Some(pcb) = proc_table().find_by_app(closing_app) {
                reap_pid_resources(pcb.pid.0);
            } else {
                crate::AppRegistry::unregister_dynamic();
            }
        } else if let Some(pcb) = proc_table_mut().find_mut_by_app(closing_app) {
            pcb.set_state(ProcState::Stopped);
        }
        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Success);
    }
}

enum FilterFlow {
    Continue,
    DirtyContinue,
    Stop(bool),
}

impl Orchestrator {
    fn handle_f_key_action<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        code: KeyCode,
    ) {
        self.lock_mgr.clear_f_active();
        if code == KeyCode::Exit {
            if self.active_app_id != APP_ID_LAUNCHER {
                self.nav_stack.clear();
                self.switch_app_internal(ctx, APP_ID_LAUNCHER, false);
                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Success);
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
            crate::apps::shortcut::resolve_slot(&ctx.config.keys, i, cur_ctx)
        });
        self.execute_shortcut_target(ctx, target_id, true);
    }

    #[inline(never)]
    pub fn execute_shortcut_target<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        target_id: u16,
        warn_if_empty: bool,
    ) -> bool {
        let cat = crate::apps::launcher::catalog::get_catalog();
        #[cfg(feature = "filesystem")]
        if (target_id & 0x8000) != 0 && !cat.scanned {
            cat.scan(ctx.storage);
        }
        match crate::apps::shortcut::resolve(target_id, cat) {
            crate::apps::shortcut::ResolvedShortcut::Builtin(cmd) => {
                self.process_ipc_command(ctx, cmd);
                true
            }
            crate::apps::shortcut::ResolvedShortcut::App { path, action_id } => {
                #[cfg(feature = "krx_native")]
                if self.active_app_id >= crate::AppRegistry::count()
                    && crate::runtime::native_runner::active_pkg_matches(path)
                {
                    if action_id != 0 {
                        crate::runtime::native_runner::dispatch_action(ctx, action_id);
                    }
                    return true;
                }
                let mut path_copy = heapless::String::<24>::new();
                let _ = path_copy.push_str(path);
                launch_krx_shortcut_arg(ctx, path_copy.as_str(), action_id as u32)
            }
            crate::apps::shortcut::ResolvedShortcut::None => {
                if warn_if_empty {
                    ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
                }
                false
            }
        }
    }

    #[inline(never)]
    fn filter_keypad_lock<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) -> FilterFlow {
        let was_locked = self.lock_mgr.is_locked;
        let was_f_active = self.lock_mgr.is_f_active;
        let was_f_drawer_animating = self.lock_mgr.is_f_drawer_animating();

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

        if ctx.config.keys.f_drawer == 0 {
            self.lock_mgr.snap_f_drawer(true);
        } else if !ctx.config.display.animations_enabled() {
            self.lock_mgr.snap_f_drawer(false);
        }

        if self.lock_mgr.force_close_requested {
            self.lock_mgr.force_close_requested = false;
            self.force_close_active_app(ctx);
            self.lock_mgr.reset_hold_tracking();
            return FilterFlow::Stop(true);
        }

        let state_changed = was_locked != self.lock_mgr.is_locked
            || was_f_active != self.lock_mgr.is_f_active
            || was_f_drawer_animating
            || self.lock_mgr.is_f_drawer_animating();

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

    #[inline(never)]
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

    #[inline(never)]
    fn handle_system_audio_events<'a>(
        &self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) {
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
                if bus_evt.topic == kernel::ipc::Topic::RADIO && ctx.config.audio.key_beep != 0 =>
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
            ctx.play_dialog_sound(s);
        }
    }

    #[inline(never)]
    fn filter_hotkeys_and_system<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) -> FilterFlow {
        if let Event::Key {
            code,
            state: KeyState::Pressed,
        } = *event
        {
            if self.lock_mgr.is_f_active {
                self.handle_f_key_action(ctx, code);
                return FilterFlow::Stop(true);
            }
            if code != KeyCode::Ptt && code != KeyCode::KeyF && ctx.config.audio.key_beep != 0 {
                ctx.play_beep(crate::services::melody::BeepType::from_u8(
                    ctx.config.audio.key_beep,
                ));
            }
        }

        self.handle_system_audio_events(ctx, event);

        if matches!(event, Event::Tick) {
            self.tick_background_services(ctx);
            record_proc_activation(self.active_app_id);
        }

        if let Event::Sys(sys_evt) = *event {
            if self.handle_kernel_sys_event(ctx, sys_evt) {
                return FilterFlow::Stop(true);
            }
        }

        FilterFlow::Continue
    }

    #[inline(never)]
    pub fn dispatch_event<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: Event,
    ) -> bool {
        if filter_sunk_keys(&event) {
            self.lock_mgr.reset_input_tracking();
            return false;
        }
        let mut handled = false;
        let filters: [fn(&mut Self, &mut crate::services::SysContext<'_, '_>, &Event) -> FilterFlow;
            4] = [
            Self::filter_modal_event,
            Self::filter_keypad_lock,
            Self::filter_hotkeys_and_system,
            Self::filter_active_app,
        ];
        for f in filters {
            match f(self, ctx, &event) {
                FilterFlow::Continue => {}
                FilterFlow::DirtyContinue => handled = true,
                FilterFlow::Stop(res) => {
                    handled |= res;
                    break;
                }
            }
        }
        self.drain_ipc_commands(ctx);
        handled
    }

    #[inline(never)]
    fn filter_active_app<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) -> FilterFlow {
        let prev_app = self.active_app_id;

        let is_key = matches!(*event, Event::Key { .. });
        let is_key_owned = match *event {
            Event::Key { code, .. } => self.owned_key == Some(code),
            _ => false,
        };

        // Forward event to active foreground app
        let status = crate::AppRegistry::on_event(self.active_app_id, ctx, event);
        record_proc_event_and_stack(self.active_app_id);

        // Dynamic Fault & Error Interception
        let mut fault_handled = false;
        match status {
            EventResult::Error(code) | EventResult::RecoverableFault(code) => {
                fault_handled = true;
                self.handle_dynamic_fault(ctx, code);
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
        self.handle_global_shortcuts(ctx, *event);
        FilterFlow::Stop(self.active_app_id != prev_app)
    }

    #[cold]
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
        sink_modal_keys();
    }

    #[inline(never)]
    fn pop_active_app<'a>(&mut self, ctx: &mut crate::services::SysContext<'a, '_>) {
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
        self.switch_app_internal(ctx, APP_ID_LAUNCHER, false);
    }

    #[inline(never)]
    fn filter_modal_event<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: &Event,
    ) -> FilterFlow {
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
                        #[cfg(feature = "krx_native")]
                        if let Some(pkg) = crate::runtime::fault::fault_pkg_path() {
                            let mut path_buf = [0u8; 32];
                            let n = pkg.len().min(32);
                            path_buf[..n].copy_from_slice(&pkg.as_bytes()[..n]);
                            crate::runtime::fault::clear_fault_pkg();
                            match res {
                                kernel::app::ModalResult::Confirmed(_) => {
                                    // SAFETY: copied from valid UTF-8 `fault_pkg_path()`.
                                    let path = core::str::from_utf8_unchecked(&path_buf[..n]);
                                    if !launch_krx_shortcut_arg(ctx, path, 0) {
                                        self.switch_app_internal(ctx, APP_ID_LAUNCHER, false);
                                    }
                                }
                                _ => {
                                    self.switch_app_internal(ctx, APP_ID_LAUNCHER, false);
                                }
                            }
                            return FilterFlow::Stop(true);
                        }
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
                    } else if tag == crate::orchestrator::compositor::TAG_SYSTEM_ALERT {
                        // System notifications self-dismiss without dispatching app modal callbacks.
                    } else {
                        crate::AppRegistry::on_modal_result(self.active_app_id, ctx, &res);
                    }

                    if modal_mut.is_active() && modal_mut.tag == tag {
                        modal_mut.dismiss();
                    }

                    if tag == crate::orchestrator::compositor::TAG_SYSTEM_ALERT
                        && self.active_app_id == APP_ID_LAUNCHER
                    {
                        crate::orchestrator::compositor::present_next_system_alert(ctx);
                    }
                }
                self.lock_mgr.reset_input_tracking();
                FilterFlow::Stop(true)
            } else {
                FilterFlow::Continue
            }
        }
    }

    #[inline(never)]
    fn handle_global_shortcuts<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        event: Event,
    ) {
        let cur_ctx = crate::apps::shortcut::active_context(self.active_app_id);
        let mut fire_slot: Option<u16> = None;
        match event {
            Event::Key {
                code: KeyCode::Exit,
                state: KeyState::Pressed,
            } => {
                // When already in Launcher (root launcher screen), Exit does not navigate back.
                // This ensures holding Exit to sleep does not trigger an accidental switch to VFO.
                if self.active_app_id != APP_ID_LAUNCHER {
                    self.pop_active_app(ctx);
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
                let long =
                    crate::apps::shortcut::resolve_slot(&ctx.config.keys, base_slot + 1, cur_ctx);
                let short =
                    crate::apps::shortcut::resolve_slot(&ctx.config.keys, base_slot, cur_ctx);
                fire_slot = Self::handle_side_key(held, state, long, short);
            }
            Event::Key {
                code: KeyCode::Menu,
                state: KeyState::Held,
            } => {
                fire_slot = Some(crate::apps::shortcut::resolve_slot(
                    &ctx.config.keys,
                    4,
                    cur_ctx,
                ));
            }
            _ => {}
        }
        if let Some(target_id) = fire_slot {
            self.execute_shortcut_target(ctx, target_id, false);
        }
    }

    fn handle_kernel_sys_event<'a>(
        &mut self,
        ctx: &mut crate::services::SysContext<'a, '_>,
        sys_evt: kernel::ipc::SysEvent,
    ) -> bool {
        if sys_evt.target == ipc_target::KERNEL {
            #[cfg(feature = "krx_runtime")]
            if let Some(msg) =
                crate::runtime::syscall::service::ipc_channel_mut().poll(ipc_target::KERNEL)
            {
                let mut ack_byte = [0u8; 1];
                let ack: Option<&[u8]> = match msg.msg_type {
                    ipc_msg_type::APP_SWITCH if msg.payload_len > 0 => {
                        let target_app = msg.payload[0] as usize;
                        self.switch_app(ctx, target_app);
                        ack_byte[0] = target_app as u8;
                        Some(&ack_byte)
                    }
                    ipc_msg_type::PING => Some(b"PONG"),
                    ipc_msg_type::DAEMON_STOP if msg.payload_len > 0 => {
                        let pid = kernel::proc::Pid(msg.payload[0]);
                        if let Some(pcb) = proc_table_mut().find_mut(pid) {
                            pcb.set_state(ProcState::Dormant);
                        }
                        Some(&ack_byte)
                    }
                    _ => None,
                };
                if let Some(reply_payload) = ack {
                    if (msg.flags & ipc_flags::NEED_ACK) != 0 {
                        let rep = msg.reply_to(reply_payload, false);
                        let _ = crate::runtime::syscall::service::ipc_channel_mut().send(rep);
                    }
                }
                return true;
            }
        } else if sys_evt.target != 0 {
            return false;
        }

        let cmd = Shortcut::from(sys_evt);
        if cmd != Shortcut::None {
            self.process_ipc_command(ctx, cmd);
            return true;
        }
        false
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

    #[inline(never)]
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

                    // Dynamic Max Foreground Apps & RAM cache trimming
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
            self.lock_mgr.reset_hold_tracking();
            sink_modal_keys();

            // Resume app if hibernated in swap slot
            #[cfg(feature = "app_hibernation")]
            Self::resume_app_state(app_id);

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
            Shortcut::CloseApp => self.pop_active_app(ctx),
            Shortcut::Flashlight => {
                if bsp::pins::FLASHLIGHT.is_high() {
                    bsp::pins::FLASHLIGHT.set_low();
                } else {
                    bsp::pins::FLASHLIGHT.set_high();
                }
            }
            Shortcut::Settings => self.switch_app(ctx, APP_ID_SETTINGS),
            Shortcut::LockKeypad => self.toggle_keypad_lock(),
            Shortcut::PlayBeep {
                freq_hz,
                duration_ms,
            } => {
                ctx.play_tone(freq_hz, duration_ms);
            }
            Shortcut::Reboot => kernel::power::PowerControl::reboot(),
            Shortcut::Sleep => {
                self.lock_mgr.sleep_requested = true;
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
        assert_eq!(pcb.app_id, crate::apps::APP_ID_VFO);
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
        let mut orch = Orchestrator::new(crate::apps::APP_ID_LAUNCHER);
        // Core daemons PID 1..=4 must never be killable.
        assert!(!reap_pid_resources(1));
        assert!(!reap_pid_resources(4));

        // Spawn user process PID > 4 with an active lease.
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
        assert!(pid.0 > 4);
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
    }
}
