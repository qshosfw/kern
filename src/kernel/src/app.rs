//! Declarative modular Application system and generic dependency-injected system context.

use crate::event::Event;

/// Static metadata describing an App for the OS GUI launcher.
#[derive(Debug, Clone, Copy)]
pub struct AppMetadata {
    pub name: &'static str,
    pub icon: &'static [u8; 32],
    pub icon_8: &'static [u8; 8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    Ignored,
    Consumed,
    Error(u8),
    RecoverableFault(u8),
}

/// Status Bar Configuration requested by an application for the OS compositor.
#[derive(Debug, Clone, Copy)]
pub struct StatusBarConfig<'a> {
    pub visible: bool,
    pub title: Option<&'a str>,
    pub center_text: Option<&'a str>,
    pub signal_level: Option<u8>,
    pub left_badge: Option<&'a str>,
    pub show_divider: bool,
}

impl<'a> Default for StatusBarConfig<'a> {
    fn default() -> Self {
        Self {
            visible: true,
            title: None,
            center_text: None,
            signal_level: None,
            left_badge: None,
            show_divider: true,
        }
    }
}

impl<'a> StatusBarConfig<'a> {
    pub const fn new(title: &'a str) -> Self {
        Self {
            visible: true,
            title: Some(title),
            center_text: None,
            signal_level: None,
            left_badge: None,
            show_divider: true,
        }
    }

    pub const fn with_center(mut self, center: &'a str) -> Self {
        self.center_text = Some(center);
        self
    }

    pub const fn with_signal(mut self, level: u8) -> Self {
        self.signal_level = Some(level);
        self
    }

    pub const fn with_badge(mut self, badge: &'a str) -> Self {
        self.left_badge = Some(badge);
        self
    }

    pub const fn with_divider(mut self, show_divider: bool) -> Self {
        self.show_divider = show_divider;
        self
    }
}

/// Result returned from an OS modal dialog or input prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalResult<'a> {
    Dismissed(u8),
    Confirmed(u8),
    TextInputDone { tag: u8, text: &'a str },
    SheetSelected { tag: u8, index: usize },
    Canceled(u8),
}

/// Declarative Application interface for microkernel GUI and foreground modules.
pub trait App<C> {
    /// Static metadata for launcher registration.
    fn meta() -> AppMetadata
    where
        Self: Sized;

    /// Returns status bar configuration for the OS compositor. Defaults to visible with app name.
    fn status_bar_config(&self) -> StatusBarConfig<'_> {
        StatusBarConfig::default()
    }

    /// Called by the OS when launched into the foreground.
    fn on_start(&mut self, ctx: &mut C);

    /// Core event handler receiving ownership of events and localized context.
    fn on_event(&mut self, ctx: &mut C, event: &Event) -> EventResult;

    /// Called when an OS modal (popup, text prompt, or option sheet) produces a result.
    fn on_modal_result(&mut self, _ctx: &mut C, _result: &ModalResult<'_>) -> EventResult {
        EventResult::Ignored
    }

    /// Called during the OS render loop.
    fn render(&self, canvas: &mut crate::canvas::Canvas, ctx: &C);

    /// Called when backgrounded or closed.
    fn on_exit(&mut self, ctx: &mut C);
}

// ── Modular VTable & Dynamic Application Descriptor ─────────────────────────

/// Type-erased function-pointer VTable for modular, dynamic application dispatch.
#[derive(Debug)]
pub struct AppVTable<C> {
    pub on_start: unsafe fn(*mut (), &mut C),
    pub on_event: unsafe fn(*mut (), &mut C, &Event) -> EventResult,
    pub on_modal_result: unsafe fn(*mut (), &mut C, &ModalResult<'_>) -> EventResult,
    pub render: unsafe fn(*const (), &mut crate::canvas::Canvas, &C, &mut StatusBarConfig<'_>),
    pub on_exit: unsafe fn(*mut (), &mut C),
}

impl<C> Clone for AppVTable<C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C> Copy for AppVTable<C> {}

impl<C> AppVTable<C> {
    pub const fn from_app<A: App<C>>() -> Self {
        unsafe fn v_on_start<A: App<C>, C>(ptr: *mut (), ctx: &mut C) {
            unsafe { &mut *(ptr as *mut A) }.on_start(ctx);
        }
        unsafe fn v_on_event<A: App<C>, C>(ptr: *mut (), ctx: &mut C, ev: &Event) -> EventResult {
            unsafe { &mut *(ptr as *mut A) }.on_event(ctx, ev)
        }
        unsafe fn v_on_modal<A: App<C>, C>(
            ptr: *mut (),
            ctx: &mut C,
            res: &ModalResult<'_>,
        ) -> EventResult {
            unsafe { &mut *(ptr as *mut A) }.on_modal_result(ctx, res)
        }
        unsafe fn v_render<A: App<C>, C>(
            ptr: *const (),
            cvs: &mut crate::canvas::Canvas,
            ctx: &C,
            sb: &mut StatusBarConfig<'_>,
        ) {
            let app = unsafe { &*(ptr as *const A) };
            app.render(cvs, ctx);
            let cfg = app.status_bar_config();
            *sb = unsafe { core::mem::transmute::<StatusBarConfig<'_>, StatusBarConfig<'_>>(cfg) };
        }
        unsafe fn v_on_exit<A: App<C>, C>(ptr: *mut (), ctx: &mut C) {
            unsafe { &mut *(ptr as *mut A) }.on_exit(ctx);
        }

        Self {
            on_start: v_on_start::<A, C>,
            on_event: v_on_event::<A, C>,
            on_modal_result: v_on_modal::<A, C>,
            render: v_render::<A, C>,
            on_exit: v_on_exit::<A, C>,
        }
    }
}

/// Complete application descriptor encapsulating metadata, dispatch table, and layout.
#[derive(Debug, Clone, Copy)]
pub struct AppDescriptor<C> {
    pub metadata: AppMetadata,
    pub vtable: AppVTable<C>,
    pub required_capabilities: crate::caps::CapabilitySet,
    pub instance_size: usize,
    pub instance_align: usize,
}

impl<C> AppDescriptor<C> {
    pub const fn from_parts(
        metadata: AppMetadata,
        vtable: AppVTable<C>,
        required_capabilities: crate::caps::CapabilitySet,
        instance_size: usize,
        instance_align: usize,
    ) -> Self {
        Self {
            metadata,
            vtable,
            required_capabilities,
            instance_size,
            instance_align,
        }
    }

    pub fn for_app<A: App<C>>(required_caps: crate::caps::CapabilitySet) -> Self {
        Self {
            metadata: A::meta(),
            vtable: AppVTable::from_app::<A>(),
            required_capabilities: required_caps,
            instance_size: core::mem::size_of::<A>(),
            instance_align: core::mem::align_of::<A>(),
        }
    }
}
