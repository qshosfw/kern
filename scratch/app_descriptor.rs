struct AppDescriptor {
    on_event: fn(ctx: &mut crate::services::FirmwareContext, event: &kernel::event::Event) -> kernel::app::EventResult,
    on_modal_result: fn(ctx: &mut crate::services::FirmwareContext, res: &kernel::app::ModalResult) -> kernel::app::EventResult,
    on_start: fn(ctx: &mut crate::services::FirmwareContext),
    on_exit: fn(ctx: &mut crate::services::FirmwareContext),
    render: fn(canvas: &mut gfx::Canvas, ctx: &crate::services::FirmwareContext) -> kernel::app::StatusBarConfig<'static>,
}

macro_rules! app_desc {
    ($getter:ident) => {
        AppDescriptor {
            on_event: |ctx, ev| unsafe { $getter().on_event(ctx, ev) },
            on_modal_result: |ctx, res| unsafe { $getter().on_modal_result(ctx, res) },
            on_start: |ctx| unsafe { $getter().on_start(ctx) },
            on_exit: |ctx| unsafe { $getter().on_exit(ctx) },
            render: |canvas, ctx| unsafe {
                let a = $getter();
                a.render(canvas, ctx);
                a.status_bar_config()
            },
        }
    }
}

fn get_app(app_id: usize) -> Option<&'static AppDescriptor> {
    static APPS: [AppDescriptor; {
        #[cfg(feature = "filesystem")] { 4 }
        #[cfg(not(feature = "filesystem"))] { 3 }
    }] = [
        app_desc!(app_launcher),
        app_desc!(app_vfo),
        app_desc!(app_settings),
        #[cfg(feature = "filesystem")]
        app_desc!(app_explorer),
    ];
    APPS.get(app_id)
}
