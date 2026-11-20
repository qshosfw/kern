//! Navigation Stack & Menu State Synchronization for OS Settings

use super::app::{SettingsApp, settings_arena_mut, settings_arena_ref};
use super::options::{page_defs, page_spec, page_title, populate_spec_range};
use super::page::{MenuFrame, PageId, PageSpec, SettingField, TAG_BATTERY_TYPE};
use crate::SysContext;
use gfx::gui::modules::menu::MenuItemType;

impl SettingsApp {
    #[inline(never)]
    pub fn populate_current_menu(&mut self, ctx: &mut SysContext) {
        let arena = settings_arena_mut();
        #[cfg(feature = "passcode")]
        if self.current_page == PageId::Security {
            self.items_len = super::security::populate_security(&mut arena.items, ctx);
            return;
        }

        if self.current_page == PageId::ButtonsShortcuts {
            self.items_len = super::buttons::populate_shortcuts(
                &mut arena.items,
                &mut arena.item_icons,
                self.shortcut_ctx,
                ctx,
            );
            return;
        }
        if self.current_page == PageId::AppPrefs {
            self.items_len =
                super::app_prefs::populate_app_prefs(&mut arena.items, self.selected_pkg_idx, ctx);
            return;
        }

        let spec = page_spec(self.current_page);

        #[cfg(feature = "krx_native")]
        if self.current_page == PageId::System {
            self.populate_system_page(spec, ctx);
            return;
        }

        self.items_len = 0;
        self.populate_def_tags(spec, 0, spec.defs.len(), ctx);

        if self.current_page == PageId::Apps {
            let cat = crate::apps::launcher::catalog::get_catalog();
            #[cfg(feature = "filesystem")]
            cat.scan(ctx.storage);
            let mut app_idx = 0usize;
            for e in &cat.entries {
                if self.items_len >= arena.items.len() {
                    break;
                }
                if crate::apps::launcher::catalog::PackageCatalog::matches_kind(e, false) {
                    if e.has_config {
                        arena.items[self.items_len].title = e.title.as_str();
                        arena.items[self.items_len].item_type = MenuItemType::Submenu;
                        arena.item_tags[self.items_len] = 0x80 | (app_idx as u8);
                        self.items_len += 1;
                    }
                    app_idx += 1;
                }
            }
        }
    }

    #[inline(never)]
    fn populate_def_tags(
        &mut self,
        spec: &PageSpec,
        start: usize,
        end: usize,
        ctx: &mut SysContext,
    ) {
        let arena = settings_arena_mut();
        let s = self.items_len;
        let n = populate_spec_range(&mut arena.items[s..], spec, start, end, ctx);
        for i in 0..n {
            arena.item_tags[s + i] = (start + i) as u8;
        }
        self.items_len = s + n;
    }

    #[cfg(feature = "krx_native")]
    #[inline(never)]
    fn append_system_scope_items(
        &mut self,
        cat: &'static crate::apps::launcher::catalog::PackageCatalog,
        sys_caps: ::abi::CapabilitySet,
        scope: u8,
        tag_base: u8,
        item_type: MenuItemType,
    ) {
        const _: () = assert!(crate::apps::launcher::catalog::MAX_CATALOG_PACKAGES <= 64);
        let arena = settings_arena_mut();
        for (cat_idx, entry) in cat.entries.as_slice().iter().enumerate() {
            if self.items_len >= arena.items.len() {
                break;
            }
            if entry.is_system_entry(sys_caps, scope) {
                arena.items[self.items_len].title = entry.title.as_str();
                arena.items[self.items_len].item_type = item_type;
                arena.item_tags[self.items_len] = tag_base | (cat_idx as u8);
                self.items_len += 1;
            }
        }
    }

    #[cfg(feature = "krx_native")]
    fn populate_system_page(&mut self, spec: &PageSpec, ctx: &mut SysContext) {
        let cat = crate::apps::launcher::catalog::get_catalog();
        cat.scan(ctx.storage);
        let sys_caps = ctx.caps();
        let len = spec.defs.len();
        let menu_ins = super::system::system_modules_insert_idx().min(len);
        let act_ins = super::system::system_actions_insert_idx()
            .min(len)
            .max(menu_ins);

        self.items_len = 0;
        self.populate_def_tags(spec, 0, menu_ins, ctx);
        self.append_system_scope_items(
            cat,
            sys_caps,
            ::abi::scope::SYSTEM_PAGE,
            0x80,
            MenuItemType::Submenu,
        );
        self.populate_def_tags(spec, menu_ins, act_ins, ctx);
        self.append_system_scope_items(
            cat,
            sys_caps,
            ::abi::scope::SYSTEM_ACTION,
            0xC0,
            MenuItemType::Action,
        );
        self.populate_def_tags(spec, act_ins, len, ctx);
    }

    #[inline(never)]
    fn enter_page(
        &mut self,
        page: PageId,
        selected_idx: usize,
        slide_dir: i16,
        ctx: &mut SysContext,
    ) {
        self.current_page = page;
        self.selected_idx = selected_idx;
        self.title = page_title(page);
        if page == PageId::ButtonsShortcuts && slide_dir > 0 {
            self.shortcut_ctx = 0;
        }
        self.populate_current_menu(ctx);
        if self.selected_idx >= self.items_len {
            self.selected_idx = self.items_len.saturating_sub(1);
        }
        self.slide_offset_x = if ctx.config.display.animations_enabled() {
            slide_dir
        } else {
            0
        };
        self.reset_input_state();
    }

    fn commit_field(&mut self, field: SettingField, val: i32, ctx: &mut SysContext) {
        if field.set_val(val, &mut ctx.config) {
            self.dirty_timer_ms = 250;
        }
    }

    #[inline(never)]
    pub fn push_submenu(&mut self, page: PageId, ctx: &mut SysContext) {
        if self.stack_top < self.stack.len() {
            self.stack[self.stack_top] = MenuFrame {
                page: self.current_page,
                selected_idx: self.selected_idx as u8,
            };
            self.stack_top += 1;
        }
        self.enter_page(page, 0, 28, ctx);
        self.view.reset_scroll();
    }

    #[inline(never)]
    pub fn pop_submenu(&mut self, ctx: &mut SysContext) -> bool {
        if self.stack_top > 0 {
            self.stack_top -= 1;
            let frame = self.stack[self.stack_top];
            self.enter_page(frame.page, frame.selected_idx as usize, -28, ctx);
            self.view.adjust_scroll(self.selected_idx, self.items_len);
            self.view.scroller.jump((self.view.scroll_top as i16) * 13);

            if ctx.config.is_dirty() {
                let _ = ctx.flush_config();
            }
            true
        } else {
            false
        }
    }

    #[inline(never)]
    pub fn update_config_from_menu(&mut self, ctx: &mut SysContext) {
        let idx = self.selected_idx;
        if idx >= self.items_len {
            return;
        }
        let arena = settings_arena_ref();
        let val = match arena.items[idx].item_type {
            MenuItemType::Range { val, .. } => val as i32,
            MenuItemType::Toggle(b) => b as i32,
            _ => return,
        };
        #[cfg(feature = "passcode")]
        if self.current_page == PageId::Security {
            super::security::update_security_toggle(ctx, idx, val != 0);
            return;
        }
        if self.current_page == PageId::ButtonsShortcuts {
            if idx == 1 {
                self.commit_field(SettingField::F_DRAWER, val, ctx);
            }
            return;
        }
        if self.current_page == PageId::AppPrefs {
            super::app_prefs::commit_app_pref(idx, val, ctx);
            return;
        }
        let tag = arena.item_tags[idx];
        if tag < 0x80 {
            if let Some(def) = page_defs(self.current_page).get(tag as usize) {
                self.commit_field(def.field, val, ctx);
            }
        }
    }

    pub fn flush_and_refresh(&mut self, ctx: &mut SysContext) {
        let _ = ctx.flush_config();
        self.populate_current_menu(ctx);
    }

    #[inline(never)]
    pub fn handle_radio_confirmed(&mut self, ctx: &mut SysContext, sel: usize) {
        let sel_idx = self.selected_idx;
        if sel_idx < self.items_len {
            if self.current_page == PageId::AppPrefs {
                super::app_prefs::commit_app_pref(sel_idx, sel as i32, ctx);
                return;
            }
            if self.current_page == PageId::ButtonsShortcuts && sel_idx == 0 {
                self.shortcut_ctx = (sel as u8).min(4);
                return;
            }
            #[cfg(feature = "passcode")]
            if self.current_page == PageId::Security {
                super::security::update_security_option(ctx, sel_idx, sel);
                return;
            }
            let tag = settings_arena_ref().item_tags[sel_idx];
            if tag >= 0x80 {
                return;
            }
            let defs = page_defs(self.current_page);
            if let Some(def) = defs.get(tag as usize) {
                if def.field == SettingField::KEY_BEEP {
                    ctx.play_beep(crate::services::melody::BeepType::from_u8(sel as u8));
                } else if def.field == SettingField::BOOT_JINGLE {
                    if sel != 0 {
                        let jingle = crate::services::melody::BootJingleType::from_u8(sel as u8);
                        let notes =
                            crate::services::melody::MelodyEngine::boot_jingle_notes(jingle);
                        if !notes.is_empty() {
                            ctx.radio.play_tone_sequence(notes);
                        }
                    } else {
                        ctx.play_beep(crate::services::melody::BeepType::Click);
                    }
                }
                let nav_layout = ctx.config.keys.nav_layout;
                let val: i32 = if def.tag == TAG_BATTERY_TYPE {
                    let (_, types) = super::battery::battery_options_and_types(nav_layout);
                    types.get(sel).copied().unwrap_or(types[0]) as i32
                } else {
                    sel as i32
                };
                self.commit_field(def.field, val, ctx);
            }
        }
    }
}
