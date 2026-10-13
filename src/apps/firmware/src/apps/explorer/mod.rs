//! KernFS VFS Storage Explorer Application
//! Hierarchical file system browser, storage inspector, file actions menu, rename & delete dialogs.
//! Modeled after fagci-k1-fw VFS Explorer.

use crate::SysContext;
use gfx::Canvas;
use gfx::Color;
use gfx::fonts::mumatrix_regular::FONT as MUMATRIX;
use gfx::fonts::tomthumb::FONT as TOMTHUMB;
use gfx::gui::modules::menu::OptionSheet;
use gfx::icons;
use gfx::text;
use kernel::app::{App, AppMetadata, EventResult, ModalResult};
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntryKind {
    ParentDir = 0,
    Folder = 1,
    Vfo = 2,
    Channel = 3,
    Band = 4,
    ScanList = 5,
    Squelch = 6,
    Config = 7,
    Tone = 8,
    Image = 9,
    Log = 10,
    Security = 11,
    Generic = 12,
}

#[inline(always)]
fn entry_kind_sprite(kind: EntryKind) -> &'static [u8; 8] {
    match kind {
        EntryKind::ParentDir => &icons::ARROW_LEFT,
        EntryKind::Folder => &icons::FILE_FOLDER,
        EntryKind::Vfo => &icons::FILE_VFO,
        EntryKind::Channel => &icons::FILE_CHANNEL,
        EntryKind::Band | EntryKind::ScanList | EntryKind::Squelch => &icons::FILE_LIST,
        EntryKind::Config => &icons::FILE_SETTING,
        EntryKind::Tone => &icons::FILE_AUDIO,
        EntryKind::Image => &icons::FILE_DISPLAY,
        EntryKind::Log => &icons::FILE_LOG,
        EntryKind::Security => &icons::FILE_KEY,
        EntryKind::Generic => &icons::FILE_DOC,
    }
}

#[derive(Clone)]
pub struct DynamicVfsItem {
    pub path: heapless::String<32>,
    pub size: usize,
    pub permission: u8,
    pub kind: EntryKind,
}

impl DynamicVfsItem {
    #[allow(dead_code)]
    pub const fn empty() -> Self {
        Self {
            path: heapless::String::new(),
            size: 0,
            permission: 0,
            kind: EntryKind::Generic,
        }
    }

    pub fn name(&self) -> &str {
        if self.kind == EntryKind::ParentDir {
            ".."
        } else {
            extract_filename(self.path.as_str())
        }
    }
}

pub const MAX_ITEMS_PER_DIR: usize = 12;

pub struct ExplorerApp {
    current_dir: heapless::String<32>,
    selected_idx: usize,
    items: [DynamicVfsItem; MAX_ITEMS_PER_DIR],
    items_len: usize,
    viewer_mode: u8, // 0 = Hex View, 1 = ASCII Text View
    clipboard_path: heapless::String<32>,
}

fn format_size<'a>(size: usize, buf: &'a mut [u8; 12]) -> &'a str {
    let (val, unit) = if size < 1024 {
        (size as i32, "B")
    } else {
        ((size >> 10) as i32, " KB")
    };
    text::format_int_fixed(val, 0, "", unit, buf)
}

fn extract_filename(path: &str) -> &str {
    if let Some(pos) = gfx::text::rfind_byte(path, b'/') {
        &path[pos + 1..]
    } else {
        path
    }
}

fn parent_dir(path: &str) -> &str {
    let trimmed = gfx::text::trim_trailing_slashes(path);
    if let Some(pos) = gfx::text::rfind_byte(trimmed, b'/') {
        if pos == 0 { "/" } else { &trimmed[..pos] }
    } else {
        "/"
    }
}

fn classify_file_kind(basename: &str, is_dir: bool) -> EntryKind {
    if is_dir {
        return EntryKind::Folder;
    }
    let ext_pos = match gfx::text::rfind_byte(basename, b'.') {
        Some(pos) => pos + 1,
        None => return EntryKind::Generic,
    };
    let ext = basename[ext_pos..].as_bytes();
    match ext.first().map(|b| b.to_ascii_lowercase()) {
        Some(b'v') => EntryKind::Vfo,
        Some(b'c') => {
            if ext.get(1).map(|b| b.to_ascii_lowercase()) == Some(b'h') {
                EntryKind::Channel
            } else {
                EntryKind::Config
            }
        }
        Some(b'b') => {
            if ext.get(1).map(|b| b.to_ascii_lowercase()) == Some(b'n') {
                EntryKind::Band
            } else {
                EntryKind::Image
            }
        }
        Some(b's') => {
            if ext.get(1).map(|b| b.to_ascii_lowercase()) == Some(b'q') {
                EntryKind::Squelch
            } else {
                EntryKind::ScanList
            }
        }
        Some(b't' | b'r') => EntryKind::Tone,
        Some(b'l') => EntryKind::Log,
        Some(b'k') => EntryKind::Security,
        Some(b'i') => EntryKind::Image,
        _ => EntryKind::Generic,
    }
}

pub fn is_entry_read_only(item: &DynamicVfsItem) -> bool {
    item.path.starts_with("/sys/")
        || (item.permission
            & (crate::services::storage::PERM_SYSTEM | crate::services::storage::PERM_READ_ONLY))
            != 0
}

pub const TAG_ACTION_MENU: u8 = 1;
pub const TAG_DELETE_CONFIRM: u8 = 2;
pub const TAG_RENAME_PROMPT: u8 = 3;
pub const TAG_NEW_FILE_PROMPT: u8 = 4;
pub const TAG_VIEWER: u8 = 5;
pub const TAG_ALERT: u8 = 6;

static ACTION_OPTIONS_CUT: [&str; 6] = ["View", "Rename", "New File", "Cut", "Delete", "Refresh"];
static ACTION_OPTIONS_PASTE: [&str; 6] = [
    "View",
    "Rename",
    "New File",
    "Paste Here",
    "Delete",
    "Refresh",
];

impl ExplorerApp {
    pub unsafe fn init_at(_ptr: *mut Self) {}

    fn active_items(&self) -> &[DynamicVfsItem] {
        &self.items[..self.items_len]
    }

    fn insert_sorted_entry(
        items: &mut [DynamicVfsItem],
        items_len: &mut usize,
        sort_start: usize,
        path: &str,
        size: usize,
        perm: u8,
        kind: EntryKind,
    ) {
        if *items_len >= MAX_ITEMS_PER_DIR {
            return;
        }
        let is_dir = kind == EntryKind::Folder;
        let path_bytes = path.as_bytes();
        let mut pos = *items_len;
        for idx in sort_start..*items_len {
            let other_is_dir = items[idx].kind == EntryKind::Folder;
            let precedes = if is_dir != other_is_dir {
                is_dir
            } else {
                path_bytes < items[idx].path.as_bytes()
            };
            if precedes {
                pos = idx;
                break;
            }
        }
        let mut k = *items_len;
        while k > pos {
            items.swap(k - 1, k);
            k -= 1;
        }
        let item = &mut items[pos];
        item.path.clear();
        let _ = item.path.push_str(path);
        item.size = size;
        item.permission = perm;
        item.kind = kind;
        *items_len += 1;
    }

    fn refresh_cache(&mut self, ctx: &mut SysContext) {
        self.items_len = 0;

        if self.current_dir.is_empty() {
            let _ = self.current_dir.push_str("/user");
        }

        // 1. Add Parent Directory link if not in root
        let has_parent = self.current_dir.as_str() != "/";
        let sort_start = if has_parent {
            let p_dir = parent_dir(self.current_dir.as_str());
            Self::insert_sorted_entry(
                &mut self.items,
                &mut self.items_len,
                0,
                p_dir,
                0,
                0,
                EntryKind::ParentDir,
            );
            1
        } else {
            0
        };

        // 2. Query StorageService list_dir dynamically and insert in sorted order
        const EMPTY_DIR: crate::services::storage::DirEntryInfo =
            crate::services::storage::DirEntryInfo::empty();
        let mut raw_entries = [EMPTY_DIR; MAX_ITEMS_PER_DIR];
        if let Ok(count) = ctx
            .storage
            .list_dir(self.current_dir.as_str(), &mut raw_entries)
        {
            for entry in &raw_entries[..count] {
                let perm = entry.permission;
                if (perm & crate::services::storage::PERM_HIDDEN) == 0 {
                    let path_str = entry.path.as_str();
                    let kind = classify_file_kind(extract_filename(path_str), entry.is_dir);
                    Self::insert_sorted_entry(
                        &mut self.items,
                        &mut self.items_len,
                        sort_start,
                        path_str,
                        entry.size,
                        perm,
                        kind,
                    );
                }
            }
        }
    }

    fn selected_item(&self) -> Option<&DynamicVfsItem> {
        if self.selected_idx < self.items_len {
            Some(&self.items[self.selected_idx])
        } else {
            None
        }
    }

    fn selected_file_path(&self) -> Option<&str> {
        let item = self.selected_item()?;
        if matches!(item.kind, EntryKind::ParentDir | EntryKind::Folder) {
            None
        } else {
            Some(item.path.as_str())
        }
    }

    fn is_protected_item(&self) -> bool {
        if let Some(item) = self.selected_item() {
            item.path.starts_with("/sys/calib.")
                || (item.permission
                    & (crate::services::storage::PERM_SYSTEM
                        | crate::services::storage::PERM_READ_ONLY))
                    != 0
        } else {
            false
        }
    }

    fn set_current_dir(&mut self, dir: &str, ctx: &mut SysContext) {
        self.current_dir.clear();
        let _ = self.current_dir.push_str(dir);
        self.selected_idx = 0;
        self.refresh_cache(ctx);
    }

    fn nav_parent_dir(&mut self, ctx: &mut SysContext) {
        if self.current_dir.as_str() != "/" {
            let p_len = parent_dir(self.current_dir.as_str()).len();
            self.current_dir.truncate(p_len);
            self.selected_idx = 0;
            self.refresh_cache(ctx);
        }
    }

    fn scroll_items(&mut self, is_up: bool) {
        let total = self.items_len;
        if total > 0 {
            if is_up {
                self.selected_idx = if self.selected_idx > 0 {
                    self.selected_idx - 1
                } else {
                    total - 1
                };
            } else {
                self.selected_idx += 1;
                if self.selected_idx >= total {
                    self.selected_idx = 0;
                }
            }
        }
    }

    fn build_path(&self, filename: &str, out: &mut heapless::String<32>) {
        out.clear();
        let _ = out.push_str(self.current_dir.as_str());
        if !self.current_dir.ends_with('/') {
            let _ = out.push('/');
        }
        let _ = out.push_str(filename);
    }

    fn format_file_details(&self, item: &DynamicVfsItem, data: &[u8], buf: &mut [u8]) -> usize {
        let mut len = 0;
        let mut push = |bytes: &[u8]| {
            let n = bytes.len().min(buf.len() - len);
            buf[len..len + n].copy_from_slice(&bytes[..n]);
            len += n;
        };

        push(b"Name: ");
        push(item.name().as_bytes());
        push(b"\nPath: ");
        push(item.path.as_bytes());
        push(b"\nPerms: ");
        let mut had_flag = false;
        if (item.permission & crate::services::storage::PERM_SYSTEM) != 0 {
            push(b"[SYS] ");
            had_flag = true;
        }
        if (item.permission & crate::services::storage::PERM_READ_ONLY) != 0 {
            push(b"[RO] ");
            had_flag = true;
        }
        if (item.permission & crate::services::storage::PERM_HIDDEN) != 0 {
            push(b"[HID] ");
            had_flag = true;
        }
        if (item.permission & crate::services::storage::ATTR_ENCRYPTED) != 0 {
            push(b"[ENC] ");
            had_flag = true;
        }
        if !had_flag {
            push(b"0x00");
        }

        push(b"\nSize: ");
        let mut sz_buf = [0u8; 12];
        push(format_size(data.len(), &mut sz_buf).as_bytes());

        if data.is_empty() {
            push(b"\n(empty)");
        } else if self.viewer_mode == 0 {
            push(b"\nHEX:\n");
            for (i, &b) in data[..data.len().min(24)].iter().enumerate() {
                if i > 0 && len < buf.len() {
                    buf[len] = if (i & 7) == 0 { b'\n' } else { b' ' };
                    len += 1;
                }
                if len + 2 <= buf.len() {
                    buf[len] = gfx::text::HEX_CHARS[(b >> 4) as usize];
                    buf[len + 1] = gfx::text::HEX_CHARS[(b & 0x0F) as usize];
                    len += 2;
                }
            }
        } else {
            push(b"\nTXT:\n");
            for &b in &data[..data.len().min(48)] {
                if len < buf.len() {
                    buf[len] = if (32..=126).contains(&b) { b } else { b'.' };
                    len += 1;
                }
            }
        }
        len
    }

    fn load_viewer_content(&mut self, ctx: &mut SysContext) {
        let Some(item) = self.selected_item() else {
            return;
        };

        let mut file_buf = [0u8; 48];
        let mut read_bytes = 0;

        match ctx.storage.read_file(item.path.as_str(), &mut file_buf) {
            Ok(sz) => read_bytes = sz,
            Err(crate::services::storage::StorageError::EncryptedNoKey) => {
                ctx.show_popup(
                    TAG_ALERT,
                    "Storage Locked",
                    "File encrypted.\nKey unavailable.",
                );
                return;
            }
            Err(_) => {}
        }

        let mut buf = [0u8; 192];
        let len = self.format_file_details(item, &file_buf[..read_bytes], &mut buf);
        let msg = unsafe { core::str::from_utf8_unchecked(&buf[..len]) };
        ctx.show_popup_str(TAG_VIEWER, "File Inspector", msg);
    }

    fn handle_browsing(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        if let Event::Key { code, state } = *event {
            if state == KeyState::Pressed {
                match code {
                    KeyCode::Up | KeyCode::Key2 | KeyCode::Side1 => {
                        self.scroll_items(true);
                        return EventResult::Consumed;
                    }
                    KeyCode::Down | KeyCode::Key8 | KeyCode::Side2 => {
                        self.scroll_items(false);
                        return EventResult::Consumed;
                    }
                    KeyCode::Menu | KeyCode::Ptt => {
                        if let Some(item) = self.selected_item() {
                            match item.kind {
                                EntryKind::ParentDir | EntryKind::Folder => {
                                    let mut p = heapless::String::<32>::new();
                                    let _ = p.push_str(item.path.as_str());
                                    self.set_current_dir(p.as_str(), ctx);
                                }
                                _ => {
                                    if is_entry_read_only(item) {
                                        self.load_viewer_content(ctx);
                                    } else {
                                        let opts: &'static [&'static str] =
                                            if self.clipboard_path.is_empty() {
                                                &ACTION_OPTIONS_CUT
                                            } else {
                                                &ACTION_OPTIONS_PASTE
                                            };
                                        ctx.show_sheet(
                                            TAG_ACTION_MENU,
                                            OptionSheet::new("Options", opts, 0),
                                        );
                                    }
                                }
                            }
                        }
                        ctx.sink_enter_keys();
                        return EventResult::Consumed;
                    }
                    KeyCode::Exit => {
                        if self.current_dir.as_str() != "/" {
                            self.nav_parent_dir(ctx);
                            ctx.sink_exit_key();
                            return EventResult::Consumed;
                        }
                        return EventResult::Ignored;
                    }
                    _ => {}
                }
            } else if state == KeyState::Released {
                if matches!(
                    code,
                    KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Side1
                        | KeyCode::Side2
                        | KeyCode::Key2
                        | KeyCode::Key8
                ) {
                    return EventResult::Consumed;
                }
            }
        }
        EventResult::Ignored
    }
}

fn render_file_list(canvas: &mut Canvas, items: &[DynamicVfsItem], selected_idx: usize) {
    let total = items.len();
    let visible_count = 4;
    let page = selected_idx / visible_count;
    let start_idx = page * visible_count;
    let end_idx = (start_idx + visible_count).min(total);

    let mut sz_buf = [0u8; 12];

    for (slot, idx) in (start_idx..end_idx).enumerate() {
        let row_y = 9 + (slot as i16 * 13);
        let is_selected = idx == selected_idx;
        let entry = &items[idx];

        let tc = if is_selected {
            let ry = row_y as u8;
            canvas.fill_rect(2, ry, 120, 13, Color::Fill);
            canvas.draw_pixel(2, ry, Color::Clear);
            canvas.draw_pixel(121, ry, Color::Clear);
            canvas.draw_pixel(2, ry + 12, Color::Clear);
            canvas.draw_pixel(121, ry + 12, Color::Clear);
            Color::Clear
        } else {
            Color::Fill
        };

        icons::draw_icon_8(canvas, 5, row_y + 2, entry_kind_sprite(entry.kind), tc);
        text::draw_str_max_x(canvas, 16, row_y + 9, entry.name(), &MUMATRIX, tc, 76);

        let right_txt = match entry.kind {
            EntryKind::Folder => "<DIR>",
            EntryKind::ParentDir => "..",
            _ => format_size(entry.size, &mut sz_buf),
        };
        text::draw_str_align(
            canvas,
            118,
            row_y + 8,
            right_txt,
            &TOMTHUMB,
            tc,
            text::Align::Right,
        );
    }

    if total > visible_count {
        gfx::widgets::draw_scrollbar(canvas, 125, 9, 52, selected_idx, total, visible_count);
    }
}

impl<'a> App<crate::services::FirmwareContext<'a, '_>> for ExplorerApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Explorer",
            icon: &icons::ICON_16_SCANNER,
        }
    }

    fn on_start(&mut self, ctx: &mut SysContext) {
        self.current_dir.clear();
        let _ = self.current_dir.push_str("/user");
        self.selected_idx = 0;
        self.viewer_mode = 0;
        self.clipboard_path.clear();
        self.refresh_cache(ctx);
    }

    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.handle_browsing(ctx, event)
    }

    fn on_modal_result(&mut self, ctx: &mut SysContext, result: &ModalResult) -> EventResult {
        let mut refresh = false;
        match result {
            ModalResult::SheetSelected {
                tag: TAG_ACTION_MENU,
                index,
            } => {
                if matches!(*index, 1 | 3 | 4) && self.is_protected_item() {
                    ctx.show_popup(
                        TAG_ALERT,
                        "Protected File",
                        "System file locked.\nAction not allowed.",
                    );
                    return EventResult::Consumed;
                }
                match *index {
                    0 => self.load_viewer_content(ctx),
                    1 => {
                        if let Some(item) = self.selected_item() {
                            ctx.prompt_text(
                                TAG_RENAME_PROMPT,
                                "Rename File",
                                item.name(),
                                32,
                                false,
                            );
                        }
                    }
                    2 => {
                        ctx.prompt_text(TAG_NEW_FILE_PROMPT, "New File", "user.log", 32, false);
                    }
                    3 => {
                        if self.clipboard_path.is_empty() {
                            let idx = self.selected_idx;
                            if idx < self.items_len
                                && !matches!(
                                    self.items[idx].kind,
                                    EntryKind::ParentDir | EntryKind::Folder
                                )
                            {
                                self.clipboard_path.clear();
                                let _ = self.clipboard_path.push_str(self.items[idx].path.as_str());
                            }
                        } else {
                            let filename = extract_filename(&self.clipboard_path);
                            let mut dest_path = heapless::String::new();
                            self.build_path(filename, &mut dest_path);
                            let _ = ctx.storage.rename(&self.clipboard_path, &dest_path);
                            self.clipboard_path.clear();
                            refresh = true;
                        }
                    }
                    4 => {
                        if self.selected_file_path().is_some() {
                            ctx.show_confirm(
                                TAG_DELETE_CONFIRM,
                                "Delete File?",
                                "Confirm delete file?",
                                "Delete",
                                "Cancel",
                            );
                        }
                    }
                    _ => refresh = true,
                }
            }
            ModalResult::Confirmed(TAG_DELETE_CONFIRM) => {
                if let Some(path) = self.selected_file_path() {
                    let _ = ctx.storage.remove(path);
                    refresh = true;
                }
            }
            ModalResult::TextInputDone { tag, text } => {
                if !text.is_empty() {
                    let mut new_path = heapless::String::new();
                    self.build_path(text.as_str(), &mut new_path);
                    if *tag == TAG_RENAME_PROMPT {
                        if let Some(path) = self.selected_file_path() {
                            let _ = ctx.storage.rename(path, &new_path);
                        }
                    } else if *tag == TAG_NEW_FILE_PROMPT {
                        let _ = ctx.storage.write_file(&new_path, b"[NEW FILE CREATED]\n");
                    }
                    refresh = true;
                }
            }
            _ => return EventResult::Ignored,
        }
        if refresh {
            self.refresh_cache(ctx);
        }
        EventResult::Consumed
    }

    fn status_bar_config(&self) -> kernel::app::StatusBarConfig<'_> {
        let center_badge = if self.clipboard_path.is_empty() {
            "Explorer"
        } else {
            "CUT"
        };
        kernel::app::StatusBarConfig {
            visible: true,
            title: Some(self.current_dir.as_str()),
            center_text: Some(center_badge),
            ..Default::default()
        }
    }

    fn render(&self, canvas: &mut Canvas, _ctx: &SysContext) {
        let items = self.active_items();
        render_file_list(canvas, items, self.selected_idx);
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
