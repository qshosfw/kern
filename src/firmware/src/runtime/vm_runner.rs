//! KVM-16 Application Runtime Adapter (KRX-ABI 2.5.0 Section 5.4).
//!
//! Provides the execution bridge, load-time bytecode verification, substrate
//! capability syscall routing, and `AppVTable` for Class 2 (`VMachine`) packages.

use crate::services::SysContext;
use gfx::Canvas;
use gfx::Color;
use kernel::app::{AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};
use kernel::proc::abi::{INTERFACE_AUDIO, INTERFACE_POWER, INTERFACE_TIMER};
use kernel::proc::vm::{Kvm16, VmStatus};
use kernel::service::PowerService;

#[repr(C, align(4))]
struct VmRunnerState {
    vm: Kvm16,
    bytecode: [u16; 256],
    bytecode_len: usize,
    app_name: heapless::String<16>,
}

#[inline(always)]
fn get_vm_state() -> &'static mut VmRunnerState {
    unsafe { &mut *(super::native_runner::arena_base_ptr() as *mut VmRunnerState) }
}

pub fn load_vm_program(name: &str, bytecode: &[u16]) -> bool {
    if bytecode.is_empty() || bytecode.len() > 256 {
        return false;
    }
    if Kvm16::verify_bytecode(bytecode).is_err() {
        return false;
    }
    let state = get_vm_state();
    unsafe {
        core::ptr::write_bytes(
            state as *mut VmRunnerState as *mut u8,
            0,
            core::mem::size_of::<VmRunnerState>(),
        );
    }
    state.vm.reset();
    state.bytecode[..bytecode.len()].copy_from_slice(bytecode);
    state.bytecode_len = bytecode.len();
    state.app_name.clear();
    let _ = state.app_name.push_str(name);
    true
}

fn dispatch_vm_slice(state: &mut VmRunnerState, ctx: &mut SysContext<'static, 'static>, fuel: u32) {
    let code_slice = &state.bytecode[..state.bytecode_len];
    if let Ok(VmStatus::SysCall { iface, op, arg }) = state.vm.run_slice(code_slice, fuel) {
        match iface {
            INTERFACE_AUDIO | 0x000A => {
                ctx.play_tone(op.saturating_mul(100).max(200), arg.min(500));
            }
            INTERFACE_POWER => {
                state.vm.regs[0] = ctx.power.battery_voltage_mv();
                state.vm.regs[1] = ctx.power.battery_percentage() as u16;
            }
            INTERFACE_TIMER => {
                state.vm.regs[0] = (ctx.power.uptime_secs() & 0xFFFF) as u16;
            }
            _ => {}
        }
    }
}

pub unsafe fn vm_on_start(_ptr: *mut (), ctx: &mut SysContext<'static, 'static>) {
    let state = get_vm_state();
    dispatch_vm_slice(state, ctx, 5000);
}

pub unsafe fn vm_on_event(
    _ptr: *mut (),
    ctx: &mut SysContext<'static, 'static>,
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
        state.vm.regs[0] = *code as u16;
        if state.vm.is_halted {
            state.vm.is_halted = false;
            state.vm.pc = 0;
        }
        dispatch_vm_slice(state, ctx, 2000);
        EventResult::Consumed
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn vm_on_modal_result(
    _ptr: *mut (),
    _ctx: &mut SysContext<'static, 'static>,
    _r: &ModalResult<'_>,
) -> EventResult {
    EventResult::Consumed
}

pub unsafe fn vm_on_render(
    _ptr: *const (),
    canvas: &mut Canvas,
    _ctx: &SysContext<'static, 'static>,
    status_bar: &mut StatusBarConfig<'_>,
) {
    let state = get_vm_state();
    let name = state.app_name.as_str();
    *status_bar = StatusBarConfig::new(name);
    gfx::text::draw_str(
        canvas,
        8,
        20,
        name,
        &gfx::fonts::mumatrix_regular::FONT,
        Color::Fill,
    );
    let mut buf = [0u8; 12];
    let r0_str = gfx::text::format_u32(state.vm.regs[0] as u32, &mut buf);
    gfx::text::draw_str(
        canvas,
        8,
        34,
        "KVM-16 v0:",
        &gfx::fonts::tomthumb::FONT,
        Color::Fill,
    );
    gfx::text::draw_str(
        canvas,
        54,
        34,
        r0_str,
        &gfx::fonts::tomthumb::FONT,
        Color::Fill,
    );
}

pub static VM_RUNNER_VTABLE: AppVTable<SysContext<'static, 'static>> = AppVTable {
    on_start: vm_on_start,
    on_event: vm_on_event,
    on_modal_result: vm_on_modal_result,
    render: vm_on_render,
    on_exit: |_ptr, _ctx| {},
};
