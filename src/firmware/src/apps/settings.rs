//! System Settings icon/dialog helpers (Settings itself lives in `/apps/settings.krx`).

pub mod apps {
    #[inline(always)]
    fn resolve_icon_id(icon_id: u16) -> u8 {
        u8::try_from(icon_id).unwrap_or(::abi::icon::APPS)
    }

    #[inline(never)]
    pub fn package_icon_8(_title: &str, icon_id: u16) -> &'static [u8; 8] {
        gfx::icons::system_icon(resolve_icon_id(icon_id)).unwrap_or(gfx::icons::ICON_APPS)
    }
}

pub mod dialog {
    pub const TAG_POPUP_INFO: u8 = 7;
}
