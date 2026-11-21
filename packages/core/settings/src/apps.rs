//! Dynamic `/apps/` Installed Packages Browser & Uninstaller.

use kern_sdk::{KrxApiTable, KrxDirEntry};

use crate::ui;

pub const MAX_APPS: usize = 6;
pub const MODAL_DELETE_APP: u8 = 40;

pub struct AppsView {
    pub entries: [KrxDirEntry; MAX_APPS],
    pub count: u8,
    pub sel: u8,
}

impl AppsView {
    pub const fn new() -> Self {
        Self {
            entries: [KrxDirEntry {
                name: [0; 24],
                size: 0,
                is_dir: 0,
                reserved: [0; 3],
            }; MAX_APPS],
            count: 0,
            sel: 0,
        }
    }

    #[inline(never)]
    pub fn refresh(&mut self, api: &KrxApiTable) {
        let n = api.list_dir(b"/apps", &mut self.entries);
        self.count = (n.min(MAX_APPS)) as u8;
        if self.count == 0 {
            self.sel = 0;
        } else if self.sel >= self.count {
            self.sel = self.count - 1;
        }
    }

    #[inline(never)]
    pub fn entry_name(&self, idx: u8) -> &[u8] {
        if idx >= self.count {
            return b"";
        }
        let e = unsafe { self.entries.get_unchecked(idx as usize) };
        let mut len = 0usize;
        while len < 24 && unsafe { *e.name.get_unchecked(len) } != 0 {
            len += 1;
        }
        unsafe { core::slice::from_raw_parts(e.name.as_ptr(), len) }
    }

    #[inline(never)]
    pub fn request_delete(&self, api: &KrxApiTable) {
        if self.count == 0 {
            return;
        }
        let name = strip_apps_prefix(self.entry_name(self.sel));
        if eq_bytes(name, b"settings.krx") {
            api.show_toast(b"Core Settings App", 1200);
            return;
        }
        api.show_confirm(MODAL_DELETE_APP, b"Remove?", name, b"Hold", b"Cancel", 1000);
    }

    #[inline(never)]
    pub fn confirm_delete(&mut self, api: &KrxApiTable) {
        if self.count == 0 {
            return;
        }
        let short = strip_apps_prefix(self.entry_name(self.sel));
        let mut path = [0u8; 32];
        let mut pos = copy_into(&mut path, 0, b"/apps/");
        pos = copy_into(&mut path, pos, short);
        let full = unsafe { core::slice::from_raw_parts(path.as_ptr(), pos) };
        let msg: &[u8] = if api.remove_file(full) {
            self.refresh(api);
            b"Removed"
        } else {
            b"Remove Failed"
        };
        api.show_toast(msg, 1200);
    }

    #[inline(never)]
    pub fn render(&self, api: &KrxApiTable, scroll_y: i16, tick: u8) {
        if self.count == 0 {
            ui::draw_scrolled_row(api, ui::ROW_START_Y, b"No packages", b"/apps", 0, false, false, 0, tick);
            return;
        }
        let mut sz_buf = [0u8; 16];
        let mut idx = 0u8;
        while idx < self.count {
            let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - scroll_y;
            let short = strip_apps_prefix(self.entry_name(idx));
            let e = unsafe { self.entries.get_unchecked(idx as usize) };
            let sz = ui::format_bytes(e.size, &mut sz_buf);
            ui::draw_scrolled_row(api, y, short, sz, 0, idx == self.sel, false, 0, tick);
            idx += 1;
        }
        ui::finish_list_frame(api, scroll_y, self.count);
    }
}

#[inline(always)]
fn strip_apps_prefix(name: &[u8]) -> &[u8] {
    if name.len() > 6
        && unsafe { *name.get_unchecked(0) } == b'/'
        && unsafe { *name.get_unchecked(5) } == b'/'
    {
        unsafe { core::slice::from_raw_parts(name.as_ptr().add(6), name.len() - 6) }
    } else {
        name
    }
}

#[inline(never)]
fn copy_into(dst: &mut [u8; 32], pos: usize, src: &[u8]) -> usize {
    let n = src.len().min(32usize.saturating_sub(pos));
    let mut i = 0usize;
    while i < n {
        unsafe { *dst.get_unchecked_mut(pos + i) = *src.get_unchecked(i) };
        i += 1;
    }
    pos + n
}

#[inline(always)]
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        if unsafe { *a.get_unchecked(i) != *b.get_unchecked(i) } {
            return false;
        }
        i += 1;
    }
    true
}
