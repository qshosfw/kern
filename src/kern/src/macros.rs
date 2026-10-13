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

/// Declarative application registry macro that defines static uninitialized storage,
/// zero-cost initialization, instance pointers, and the immutable ROM application descriptor table.
#[macro_export]
macro_rules! define_os_apps {
    (
        context: $ctx:ty;
        $(
            $(#[$meta:meta])*
            $name:ident => $app_type:ty [ storage: $storage:ident, meta: $app_meta:expr, caps: $caps:expr ]
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
            #[allow(non_upper_case_globals)]
            pub static mut $storage: core::mem::MaybeUninit<$app_type> = core::mem::MaybeUninit::uninit();

            $(#[$meta])*
            #[allow(dead_code)]
            pub const $name: usize = OsAppId::$name as usize;
        )*

        #[inline(never)]
        pub fn init_all_apps() {
            unsafe {
                $(
                    $(#[$meta])*
                    {
                        core::ptr::write_bytes($storage.as_mut_ptr(), 0, 1);
                        <$app_type>::init_at($storage.as_mut_ptr());
                    }
                )*
            }
        }

        pub static APP_TABLE: &[crate::apps::registry::OsAppDescriptor] = &[
            $(
                $(#[$meta])*
                crate::apps::registry::OsAppDescriptor {
                    metadata: $app_meta,
                    vtable: kernel::app::AppVTable::from_app::<$app_type>(),
                    instance: || unsafe { core::ptr::addr_of_mut!($storage) as *mut () },
                    required_capabilities: $caps,
                },
            )*
        ];
    };
}
