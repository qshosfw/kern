//! Compact ROM Missing-Radio Fallback Controller (used when `resident_vfo` is off
//! and `/apps/vfo.krx` is not installed in storage).

use crate::SysContext;
use gfx::Canvas;
use kernel::app::{App, EventResult, ModalResult};
use kernel::event::Event;

const TAG_MISSING_VFO: u8 = 22;

#[repr(C)]
pub struct VfoApp;

impl VfoApp {
    pub unsafe fn init_at(_ptr: *mut Self) {}

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        false
    }
}

impl App<SysContext<'_, '_>, Canvas> for VfoApp {
    #[inline(never)]
    fn on_start(&mut self, ctx: &mut SysContext) {
        ctx.show_confirm(
            TAG_MISSING_VFO,
            "Radio Missing",
            "Install vfo.krx via Dock or RigWire.",
            "OK",
            "Close",
        );
    }

    fn on_event(&mut self, _ctx: &mut SysContext, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn on_modal_result(
        &mut self,
        ctx: &mut SysContext,
        result: &ModalResult<'_>,
    ) -> EventResult {
        if matches!(
            *result,
            ModalResult::Confirmed(TAG_MISSING_VFO) | ModalResult::Canceled(TAG_MISSING_VFO)
        ) {
            ctx.post_event(crate::apps::Shortcut::CloseApp.into());
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn render(&self, _canvas: &mut Canvas, _ctx: &SysContext) {}

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
