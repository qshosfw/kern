//! Dynamic `/apps/` Installed Packages & System Modules Browser.
//!
//! - `modules_only == false` (`Apps -> Installed`): lists installed packages with size & uninstall.
//! - `modules_only == true` (`System -> Modules`): discovers installed `.mod` & `sysinfo.krx`
//!   system modules (`dfu.mod`, `recovery.mod`, `multiboot.mod`, `panic.mod`, `calib.mod`,
//!   `sysinfo.krx`) and launches them cleanly via deferred `PROC_CTL_LAUNCH_PKG`.

use kern_sdk::{KrxApiTable, KrxDirEntry};

use crate::ui;

pub const MAX_APPS: usize = 6;
pub const MODAL_DELETE_APP: u8 = 40;

pub struct AppsView {
    pub entries: [KrxDirEntry; MAX_APPS],
    pub count: u8,
    pub sel: u8,
    pub modules_only: bool,
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
            modules_only: false,
        }
    }

    #[inline(never)]
    pub fn refresh(&mut self, api: &KrxApiTable, modules_only: bool) {
        self.modules_only = modules_only;
        let total = api.list_dir(b"/apps", &mut self.entries).min(MAX_APPS);
        if modules_only {
            let mut dst = 0usize;
            let mut i = 0usize;
            while i < total {
                let name = raw_entry_name(unsafe { self.entries.get_unchecked(i) });
                let short = strip_apps_prefix(name);
                if is_system_module_file(short) {
                    if dst != i {
                        // Copy 8 u32 words manually to avoid pulling in 1.3 KB compiler_builtins::memmove
                        unsafe {
                            let sp = self.entries.as_ptr().add(i) as *const u32;
                            let dp = self.entries.as_mut_ptr().add(dst) as *mut u32;
                            let mut w = 0usize;
                            while w < 8 {
                                *dp.add(w) = *sp.add(w);
                                w += 1;
                            }
                        }
                    }
                    dst += 1;
                }
                i += 1;
            }
            self.count = dst as u8;
        } else {
            self.count = total as u8;
        }
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
        raw_entry_name(unsafe { self.entries.get_unchecked(idx as usize) })
    }

    #[inline(never)]
    pub fn on_confirm(&self, api: &KrxApiTable) {
        if self.count == 0 {
            return;
        }
        let short = strip_apps_prefix(self.entry_name(self.sel));
        if self.modules_only {
            let mut path = [0u8; 32];
            let mut pos = copy_into(&mut path, 0, b"/apps/");
            pos = copy_into(&mut path, pos, short);
            let full = unsafe { core::slice::from_raw_parts(path.as_ptr(), pos) };
            if !api.launch_pkg(full) {
                api.show_toast(b"Launch Failed", 1200);
            }
            return;
        }
        if eq_bytes(short, b"settings.krx") {
            api.show_toast(b"Core Settings App", 1200);
            return;
        }
        api.show_confirm(MODAL_DELETE_APP, b"Remove?", short, b"Hold", b"Cancel", 1000);
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
            self.refresh(api, self.modules_only);
            b"Removed"
        } else {
            b"Remove Failed"
        };
        api.show_toast(msg, 1200);
    }

    #[inline(never)]
    pub fn render(&self, api: &KrxApiTable, scroll_y: i16, anim_tick: u16) {
        if self.count == 0 {
            let empty: &[u8] = if self.modules_only {
                b"No .mod files"
            } else {
                b"No packages"
            };
            ui::draw_row(api, ui::ROW_START_Y, empty, b"/apps", 0, 0, 0);
            return;
        }
        let mut sz_buf = [0u8; 16];
        let mut idx = 0u8;
        while idx < self.count {
            let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - scroll_y;
            let short = strip_apps_prefix(self.entry_name(idx));
            let is_sel = idx == self.sel;
            if self.modules_only {
                let flags = if is_sel {
                    ui::FLAG_SEL | ui::FLAG_CHEVRON
                } else {
                    0
                };
                ui::draw_row(api, y, strip_ext(short), b"", 0, flags, anim_tick);
            } else {
                let e = unsafe { self.entries.get_unchecked(idx as usize) };
                let sz = ui::format_bytes(e.size, &mut sz_buf);
                let flags = if is_sel { ui::FLAG_SEL } else { 0 };
                ui::draw_row(api, y, short, sz, 0, flags, 0);
            }
            idx += 1;
        }
        ui::finish_list_frame(api, scroll_y, self.count);
    }
}

#[inline(always)]
fn raw_entry_name(e: &KrxDirEntry) -> &[u8] {
    let mut len = 0usize;
    while len < 24 && unsafe { *e.name.get_unchecked(len) } != 0 {
        len += 1;
    }
    unsafe { core::slice::from_raw_parts(e.name.as_ptr(), len) }
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

#[inline(always)]
fn is_system_module_file(short: &[u8]) -> bool {
    let n = short.len();
    if n > 4
        && unsafe {
            *short.get_unchecked(n - 4) == b'.'
                && *short.get_unchecked(n - 3) == b'm'
                && *short.get_unchecked(n - 2) == b'o'
                && *short.get_unchecked(n - 1) == b'd'
        }
    {
        return true;
    }
    eq_bytes(short, b"sysinfo.krx")
}

#[inline(always)]
fn strip_ext(short: &[u8]) -> &[u8] {
    let n = short.len();
    if n > 4 && unsafe { *short.get_unchecked(n - 4) } == b'.' {
        unsafe { core::slice::from_raw_parts(short.as_ptr(), n - 4) }
    } else {
        short
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

#[inline(never)]
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
