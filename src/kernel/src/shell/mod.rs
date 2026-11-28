//! Hardware-Agnostic Microkernel Shell Primitives
//!
//! Houses portable shell state machines (`ShellPlatform`, `FilterFlow`, `LockToast`,
//! `SystemAlert`, `AlertQueue`) decoupled from concrete MCU/HAL/BSP crates (`R-NO-CHIP-IN-KERNEL`).

use crate::event::Event;

/// Event-filter pipeline control flow shared by `Orchestrator` and `ModalCompositor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterFlow {
    Continue,
    DirtyContinue,
    Stop(bool),
}

/// Platform hook trait bridging portable `kernel::shell` logic to board/firmware services.
pub trait ShellPlatform {
    fn now_ms(&self) -> u32;
    fn wake_display(&mut self);
    fn dispatch_app_event(&mut self, app_id: usize, event: &Event) -> crate::app::EventResult;
}

/// Lock notification modal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LockToast {
    None = 0,
    Unlocked = 1,
    Locked = 2,
    PromptUnlock = 3,
}

/// Queued system alert banner payload.
#[derive(Clone, Copy)]
pub struct SystemAlert {
    pub title: utils::format::InlineStr<18>,
    pub message: utils::format::InlineStr<88>,
}

impl SystemAlert {
    pub const fn empty() -> Self {
        Self {
            title: utils::format::InlineStr::new(),
            message: utils::format::InlineStr::new(),
        }
    }
}

/// Single-slot zero-heap system alert queue.
pub struct AlertQueue {
    has_alert: bool,
    slot: SystemAlert,
}

impl AlertQueue {
    pub const fn new() -> Self {
        Self {
            has_alert: false,
            slot: SystemAlert::empty(),
        }
    }

    pub fn push_raw(&mut self, title: &str, wrapped_msg: &str, overwrite: bool) {
        if self.has_alert && !overwrite {
            return;
        }
        self.has_alert = true;
        self.slot.title.set(title);
        self.slot.message.set(wrapped_msg);
    }

    #[inline(always)]
    pub fn peek(&self) -> Option<&SystemAlert> {
        self.has_alert.then_some(&self.slot)
    }

    #[inline(always)]
    pub fn discard_head(&mut self) {
        self.has_alert = false;
    }

    pub fn pop(&mut self) -> Option<SystemAlert> {
        let alert = self.peek().copied()?;
        self.discard_head();
        Some(alert)
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.has_alert = false;
    }
}
