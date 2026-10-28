//! KVM-16 Application Runtime Adapter.
//!
//! Provides the execution bridge and AppVTable for running Class 2 (VM)
//! bytecode applications inside the microkernel sandbox.

use crate::services::FirmwareContext;
use gfx::Canvas;
use gfx::Color;
use kernel::app::{AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};
use kernel::proc::vm::{Kvm16, VmStatus};

struct VmRunnerState {
    vm: Kvm16,
    bytecode: [u16; 256],
    bytecode_len: usize,
    app_name: heapless::String<16>,
}

static mut VM_STATE: VmRunnerState = VmRunnerState {
    vm: Kvm16::new(),
    bytecode: [0u16; 256],
    bytecode_len: 0,
    app_name: heapless::String::new(),
};

#[inline(always)]
fn get_vm_state() -> &'static mut VmRunnerState {
    unsafe { &mut *core::ptr::addr_of_mut!(VM_STATE) }
}

pub fn load_vm_program(name: &str, bytecode: &[u16]) -> bool {
    if bytecode.len() > 256 {
        return false;
    }
    let state = get_vm_state();
    state.vm.reset();
    state.bytecode[..bytecode.len()].copy_from_slice(bytecode);
    state.bytecode_len = bytecode.len();
    state.app_name.clear();
    let _ = state.app_name.push_str(name);
    true
}

pub unsafe fn vm_on_start(_ptr: *mut (), _ctx: &mut FirmwareContext<'static, 'static>) {
    let state = get_vm_state();
    let code = &state.bytecode[..state.bytecode_len];
    let _ = state.vm.run_slice(code, 5000);
}

pub unsafe fn vm_on_event(
    _ptr: *mut (),
    ctx: &mut FirmwareContext<'static, 'static>,
    ev: &Event,
) -> EventResult {
    if let Event::Key {
        code,
        state: KeyState::Pressed,
    } = ev
    {
        if *code == KeyCode::Exit {
            ctx.post_event(crate::apps::Shortcut::LaunchApp(0).into());
            return EventResult::Consumed;
        }

        let state = get_vm_state();
        // Feed key code into register v0 and run execution slice
        state.vm.regs[0] = *code as u16;
        let code_slice = &state.bytecode[..state.bytecode_len];
        match state.vm.run_slice(code_slice, 2000) {
            Ok(VmStatus::SysCall { iface, op, arg }) => {
                // Handle VM substrate syscall
                if iface == 0x000A {
                    // CapAudio: play beep
                    ctx.play_tone(op * 100, (arg as u16).min(500));
                }
            }
            _ => {}
        }
        EventResult::Consumed
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn vm_on_modal_result(
    _ptr: *mut (),
    _ctx: &mut FirmwareContext<'static, 'static>,
    _r: &ModalResult<'_>,
) -> EventResult {
    EventResult::Consumed
}

pub unsafe fn vm_on_render(
    _ptr: *const (),
    canvas: &mut Canvas,
    _ctx: &FirmwareContext<'static, 'static>,
    status_bar: &mut StatusBarConfig<'_>,
) {
    *status_bar = StatusBarConfig::new("KVM-16 App");
    let state = get_vm_state();
    let name = state.app_name.as_str();
    gfx::text::draw_str(
        canvas,
        10,
        20,
        name,
        &gfx::fonts::mumatrix_regular::FONT,
        Color::Fill,
    );
    gfx::text::draw_str(
        canvas,
        10,
        35,
        "Running (KVM-16)",
        &gfx::fonts::tomthumb::FONT,
        Color::Fill,
    );
}

pub static VM_RUNNER_VTABLE: AppVTable<FirmwareContext<'static, 'static>> = AppVTable {
    on_start: vm_on_start,
    on_event: vm_on_event,
    on_modal_result: vm_on_modal_result,
    render: vm_on_render,
    on_exit: |_ptr, _ctx| {},
};
