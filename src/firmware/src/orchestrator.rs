use crate::services::display::DisplayService;
use crate::services::radio::RfSquelch;
// Microkernel OS Orchestrator & Shell Compositor
// Routes hardware events to the active foreground application, manages background execution,
// and orchestrates global microkernel services like IPC shortcut dispatching.

pub mod compositor;
pub mod keypad_lock;

#[cfg(feature = "app_tasks")]
use crate::apps::APP_ID_TASKS;
use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use compositor::OsCompositor;
use gfx::Canvas;
use kernel::app::EventResult;
use kernel::event::{Event, KeyCode, KeyState};
use kernel::nav::NavStack;
use kernel::proc::{ExecType, ProcState, ProcessTable};
use keypad_lock::KeypadLockManager;

pub struct Orchestrator {
    active_app_id: usize,
    previous_app: Option<usize>,
    nav_stack: NavStack<6>,
    owned_key: Option<KeyCode>,
    lock_mgr: KeypadLockManager,
    side1_held: bool,
    side2_held: bool,
    is_muted: bool,
    is_monitoring: bool,
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

impl Orchestrator {
    pub fn new(startup_app: usize) -> Self {
        init_proc_table();
        let table = proc_table_mut();

        // 1. Core Kernel Daemon (PID 1)
        let _ = table.spawn(
            "Kernel",
            0xF0,
            kernel::sched::Priority::Safety,
            ExecType::StaticFirmware,
            kernel::caps::CapabilitySet::from_raw(0xFFFF_FFFF),
        );
        if let Some(pcb) = table.find_mut_by_app(0xF0) {
            pcb.set_state(ProcState::Running);
            pcb.ledger.record_activation();
        }

        // 2. Radio Coordinator Service (PID 2)
        let mut radio_caps = kernel::caps::CapabilitySet::empty();
        radio_caps.insert(kernel::caps::Cap::RadioRx);
        radio_caps.insert(kernel::caps::Cap::RadioTx);
        let _ = table.spawn(
            "Radio",
            0xF1,
            kernel::sched::Priority::Radio,
            ExecType::StaticFirmware,
            radio_caps,
        );
        if let Some(pcb) = table.find_mut_by_app(0xF1) {
            pcb.set_state(ProcState::ServiceIdle);
            pcb.ledger.record_activation();
        }

        // 3. Power Management Service (PID 3)
        let _ = table.spawn(
            "Power",
            0xF2,
            kernel::sched::Priority::Safety,
            ExecType::StaticFirmware,
            kernel::caps::CapabilitySet::empty(),
        );
        if let Some(pcb) = table.find_mut_by_app(0xF2) {
            pcb.set_state(ProcState::ServiceIdle);
            pcb.ledger.record_activation();
        }

        // 4. Input & Keypad Daemon (PID 4)
        let _ = table.spawn(
            "Input",
            0xF3,
            kernel::sched::Priority::Ui,
            ExecType::StaticFirmware,
            kernel::caps::CapabilitySet::empty(),
        );
        if let Some(pcb) = table.find_mut_by_app(0xF3) {
            pcb.set_state(ProcState::ServiceIdle);
            pcb.ledger.record_activation();
        }

        // 5. Active Foreground Startup Application (PID 5)
        let name = crate::AppRegistry::descriptor(startup_app)
            .map(|m| m.name)
            .unwrap_or("Radio");
        let caps = crate::AppRegistry::required_caps(startup_app);
        let _ = table.spawn(
            name,
            startup_app,
            kernel::sched::Priority::Ui,
            ExecType::StaticFirmware,
            caps,
        );
        if let Some(pcb) = table.find_mut_by_app(startup_app) {
            pcb.set_state(ProcState::Running);
            pcb.ledger.record_activation();
        }

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

    pub fn active_app_id(&self) -> usize {
        self.active_app_id
    }

    /// Reset auto-lock inactivity countdown on external user or hardware activity.
    pub fn notify_activity(&mut self) {
        self.lock_mgr.reset_input_tracking();
    }

    #[inline(never)]
    fn tick_background_services<'a>(&mut self, ctx: &mut crate::services::FirmwareContext<'a, '_>) {
        // 1. Radio Coordinator / Dual Watch Daemon (PID 2)
        #[cfg(feature = "radio")]
        {
            if let Some(pcb) = proc_table_mut().find_mut_by_app(0xF1) {
                pcb.ledger.record_activation();
                let is_busy = ctx.state.coord.is_transmitting
                    || ctx.state.coord.carrier_active
                    || ctx.radio.is_carrier_open();
                pcb.set_state(if is_busy {
                    kernel::proc::ProcState::Running
                } else {
                    kernel::proc::ProcState::ServiceIdle
                });
            }
            let radio_ev = ctx.state.coord.tick_10ms(ctx.radio);
            match radio_ev {
                crate::services::radio::RadioEvent::None => {}
                crate::services::radio::RadioEvent::TotWarning { .. } => {
                    if ctx.config.audio.key_beep != 0 {
                        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
                    }
                }
                crate::services::radio::RadioEvent::TotExpired => {
                    ctx.state.sync_from_coord();
                    if ctx.config.audio.key_beep != 0 {
                        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
                    }
                }
                _ => {
                    ctx.state.sync_from_coord();
                }
            }
            if let Some(bus_ev) = radio_ev.to_bus_event(2) {
                crate::system::enqueue_event(kernel::event::Event::Bus(bus_ev));
            }
        }

        // 2. Power Management & Battery Supervisor (PID 3)
        if let Some(pcb) = proc_table_mut().find_mut_by_app(0xF2) {
            pcb.ledger.record_activation();
        }

        // 3. System Kernel & Watch Uptime Service (PID 1)
        if let Some(pcb) = proc_table_mut().find_mut_by_app(0xF0) {
            pcb.ledger.record_activation();
        }
    }

    #[inline(never)]
    pub fn dispatch_event<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        event: Event,
    ) -> bool {
        let prev_app = self.active_app_id;

        // Intercept and sink keys suppressed during screen/app/modal transitions
        if let Event::Key { code, state } = event {
            let bit = key_to_bit(code);
            if (unsafe { SUNK_KEYS } & bit) != 0 {
                if state == KeyState::Released {
                    unsafe {
                        SUNK_KEYS &= !bit;
                    }
                    return false;
                } else if state == KeyState::Held {
                    return false;
                } else {
                    unsafe {
                        SUNK_KEYS &= !bit;
                    }
                }
            }
        }

        // 0. If an OS modal is active, modal compositor intercepts all input
        if self.handle_modal_event(ctx, &event) {
            self.drain_ipc_commands(ctx);
            return true;
        }

        let hold_delay_ms = ctx.config.keys.hold_delay_ms();
        let f_timeout_ms = ctx.config.keys.f_key_timeout_ms();
        let auto_lock_ms = ctx.config.keys.auto_lock_ms();

        // 1. Keypad Lock & Hold Supervisor (strictly precedes all apps)
        let was_locked = self.lock_mgr.is_locked;
        let was_f_active = self.lock_mgr.is_f_active;

        let lock_intercepted =
            self.lock_mgr
                .handle_event(&event, hold_delay_ms, f_timeout_ms, auto_lock_ms);
        let lock_state_changed =
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
        }

        if !was_f_active && self.lock_mgr.is_f_active {
            ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Prompt);
        }

        if lock_intercepted {
            if self.lock_mgr.is_locked {
                if let Event::Key {
                    state: KeyState::Pressed,
                    code,
                } = event
                {
                    if code != KeyCode::Ptt {
                        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Prompt);
                    }
                }
            }
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
                self.lock_mgr.is_f_active = false;
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
                return true;
            }
        }

        if matches!(event, Event::Tick) {
            self.tick_background_services(ctx);
            if let Some(pcb) = proc_table_mut().find_mut_by_app(self.active_app_id) {
                pcb.ledger.record_activation();
            }
        }

        // Key ownership tracking: if a key was consumed on Pressed, subsequent
        // Held/Released events for that key belong strictly to the active app.
        let is_key_event = matches!(event, Event::Key { .. });
        let (key_code, key_state) = match event {
            Event::Key { code, state } => (Some(code), Some(state)),
            _ => (None, None),
        };

        // If this key is owned by the app and state is Held or Released, do not leak to global shortcuts
        let is_key_owned = key_code.is_some() && self.owned_key == key_code;

        // 3. Forward event to active foreground app
        if let Some(pcb) = proc_table_mut().find_mut_by_app(self.active_app_id) {
            pcb.ledger.record_event();
        }
        let status = crate::AppRegistry::on_event(self.active_app_id, ctx, &event);
        if let Some(pcb) = proc_table_mut().find_mut_by_app(self.active_app_id) {
            let used_stk = kernel::sys::SystemDiagnostics::stack_used_now() as u16;
            pcb.ledger.record_stack_depth(used_stk);
        }

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
        if is_key_event && !fault_handled {
            if let (Some(code), Some(state)) = (key_code, key_state) {
                if state == KeyState::Pressed {
                    if status == EventResult::Consumed {
                        self.owned_key = Some(code);
                    } else {
                        self.owned_key = None;
                    }
                } else if state == KeyState::Released {
                    if self.owned_key == Some(code) {
                        self.owned_key = None;
                    }
                }
            }
        }

        // 6. If foreground app consumed the event, owns this key, or fault was handled, it is completely sunk!
        if status == EventResult::Consumed || is_key_owned || fault_handled {
            if is_key_event {
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

    #[inline(never)]
    fn handle_dynamic_fault<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
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

    #[inline(never)]
    fn handle_modal_event<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
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

    #[inline(never)]
    fn handle_global_shortcuts<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        event: Event,
    ) {
        match event {
            Event::Key {
                code: KeyCode::Exit,
                state: KeyState::Pressed,
            } => {
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

    pub fn switch_app<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
        app_id: usize,
    ) {
        self.switch_app_internal(ctx, app_id, true);
    }

    fn switch_app_internal<'a>(
        &mut self,
        ctx: &mut crate::services::FirmwareContext<'a, '_>,
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
            sink_enter_keys();
            sink_exit_key();

            // Resume app if hibernated in swap slot
            #[cfg(feature = "app_hibernation")]
            Self::resume_app_state(app_id);

            // Track process activation in PCB table
            if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
                pcb.set_state(ProcState::Running);
                pcb.ledger.record_activation();
            } else {
                let name = crate::AppRegistry::descriptor(app_id)
                    .map(|m| m.name)
                    .unwrap_or("App");
                let caps = crate::AppRegistry::required_caps(app_id);
                let _ = proc_table_mut().spawn(
                    name,
                    app_id,
                    kernel::sched::Priority::Ui,
                    ExecType::StaticFirmware,
                    caps,
                );
                if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
                    pcb.set_state(ProcState::Running);
                    pcb.ledger.record_activation();
                }
            }

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
            Shortcut::Scanner | Shortcut::Explorer => {
                #[cfg(feature = "app_explorer")]
                crate::apps::explorer::launch_explorer(ctx);
                #[cfg(not(feature = "app_explorer"))]
                {
                    if let Err(err) =
                        crate::apps::runtime::launch_krx_package(ctx, "/apps/explorer.krx")
                    {
                        ctx.show_confirm(0, err.title(), err.message(), "OK", "EXIT");
                        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
                    }
                }
            }
            Shortcut::TaskManager => {
                #[cfg(feature = "app_tasks")]
                self.switch_app(ctx, APP_ID_TASKS);
                #[cfg(not(feature = "app_tasks"))]
                {
                    if let Err(err) =
                        crate::apps::runtime::launch_krx_package(ctx, "/apps/sysinfo.krx")
                    {
                        ctx.show_confirm(0, err.title(), err.message(), "OK", "EXIT");
                        ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
                    }
                }
            }
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
            Shortcut::Reboot => kernel::sys::SystemDiagnostics::reboot(),
            Shortcut::Fault => {
                panic!("Test panic");
            }
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
        #[cfg(feature = "app_tasks")]
        if self.active_app_id == APP_ID_TASKS {
            if let Some(ptr) = crate::apps::AppRegistry::app_instance(APP_ID_TASKS) {
                return unsafe { (*(ptr as *const crate::apps::TasksApp)).is_animating() };
            }
        }
        false
    }
}
