//! Declarative OS Macros
//! Provides compile-time registries for Applications and Services.

#[macro_export]
macro_rules! define_os {
    (
        apps: {
            $( $(#[$meta:meta])* $app_name:ident : $app_type:ty ),* $(,)?
        }
    ) => {
        $(
            $(#[$meta])*
            #[allow(non_upper_case_globals)]
            pub static mut $app_name: core::mem::MaybeUninit<$app_type> = core::mem::MaybeUninit::uninit();
        )*

        fn init_os_apps() {
            unsafe {
                $(
                    $(#[$meta])*
                    {
                        core::ptr::write_bytes($app_name.as_mut_ptr(), 0, 1);
                        <$app_type>::init_at($app_name.as_mut_ptr());
                    }
                )*
            }
        }
    };
}

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
                let ptr = (entry.instance)();
                (entry.init)(ptr);
            }
        }

        #[inline(never)]
        pub fn init_all_apps() {
            $(
                $(#[$meta])*
                init_app($name);
            )*
        }

        pub static APP_TABLE: &[crate::apps::registry::OsAppDescriptor] = &[
            $(
                $(#[$meta])*
                crate::apps::registry::OsAppDescriptor {
                    metadata: $app_meta,
                    vtable: kernel::app::AppVTable::from_app::<$app_type>(),
                    instance: || unsafe {
                        core::ptr::addr_of_mut!(OS_APP_STORAGE.$name) as *mut ()
                    },
                    init: |ptr| unsafe {
                        core::ptr::write_bytes(ptr, 0, core::mem::size_of::<$app_type>());
                        <$app_type>::init_at(ptr as *mut $app_type);
                    },
                    required_capabilities: $caps,
                    instance_size: core::mem::size_of::<$app_type>(),
                },
            )*
        ];
    };
}
