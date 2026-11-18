//! Declarative OS Macros
//! Provides compile-time registries for Applications and Services.

/// Declarative application registry macro that defines unified foreground storage,
/// zero-cost initialization, instance pointers, and the immutable ROM application descriptor table.
#[macro_export]
macro_rules! define_os_apps {
    (
        context: $ctx:ty;
        $(
            $(#[$meta:meta])*
            $name:ident => $app_type:ty [ meta: $app_meta:expr, caps: $caps:expr ]
        ),* $(,)?
    ) => {
        #[repr(usize)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[allow(non_camel_case_types)]
        pub enum OsAppId {
            $(
                $(#[$meta])*
                $name,
            )*
        }

        $(
            $(#[$meta])*
            #[allow(dead_code)]
            pub const $name: usize = OsAppId::$name as usize;
        )*

        #[allow(non_snake_case)]
        pub struct OsAppStorage {
            $(
                $(#[$meta])*
                pub $name: core::mem::MaybeUninit<$app_type>,
            )*
        }

        pub static mut OS_APP_STORAGE: OsAppStorage = OsAppStorage {
            $(
                $(#[$meta])*
                $name: core::mem::MaybeUninit::uninit(),
            )*
        };

        #[inline(never)]
        pub fn init_app(app_id: usize) {
            if let Some(entry) = APP_TABLE.get(app_id) {
                unsafe {
                    let base = (core::ptr::addr_of_mut!(OS_APP_STORAGE) as *mut u8)
                        .add(entry.offset as usize);
                    core::ptr::write_bytes(base, 0, entry.instance_size as usize);
                    match app_id {
                        $(
                            $(#[$meta])*
                            x if x == OsAppId::$name as usize => {
                                <$app_type>::init_at(base as *mut $app_type);
                            }
                        )*
                        _ => {}
                    }
                }
            }
        }

        #[inline(never)]
        pub fn init_all_apps() {
            for id in 0..APP_TABLE.len() {
                init_app(id);
            }
        }

        pub static APP_TABLE: &[crate::apps::registry::OsAppDescriptor] = &[
            $(
                $(#[$meta])*
                crate::apps::registry::OsAppDescriptor {
                    metadata: $app_meta,
                    required_capabilities: $caps,
                    offset: core::mem::offset_of!(OsAppStorage, $name) as u16,
                    instance_size: core::mem::size_of::<$app_type>() as u16,
                },
            )*
        ];
    };
}
