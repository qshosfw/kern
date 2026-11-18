//! Navigation Stack & Menu State Synchronization for OS Settings

use super::app::SettingsApp;
use super::options::{apply_page_changes, page_defs, page_title, populate_defs};
use super::page::{MenuFrame, PageId, SettingField, TAG_BATTERY_TYPE};
use crate::SysContext;
use gfx::gui::modules::menu::MenuItemType;

impl SettingsApp {
    #[inline(never)]
    pub fn populate_current_menu(&mut self, ctx: &mut SysContext) {
        #[cfg(feature = "passcode")]
        if self.current_page == PageId::Security {
            self.items_len = super::security::populate_security(&mut self.items, ctx);
            return;
        }

        if self.current_page == PageId::ManageApps || self.current_page == PageId::ManageServices {
            let is_srv = self.current_page == PageId::ManageServices;
            self.items_len = super::apps::populate_package_list(
                &mut self.items,
                &mut self.item_icons,
                ctx,
                is_srv,
            );
            return;
        }
        if self.current_page == PageId::ButtonsShortcuts {
            self.items_len = super::buttons::populate_shortcuts(
                &mut self.items,
                &mut self.item_icons,
                self.shortcut_ctx,
                ctx,
            );
            return;
        }
        if self.current_page == PageId::AppDetail || self.current_page == PageId::ServiceDetail {
            let is_srv = self.current_page == PageId::ServiceDetail;
            self.items_len = super::apps::populate_package_detail(
                &mut self.items,
                is_srv,
                self.selected_pkg_idx,
            );
            return;
        }
        if self.current_page == PageId::AppPrefs {
            self.items_len =
                super::app_prefs::populate_app_prefs(&mut self.items, self.selected_pkg_idx, ctx);
            return;
        }

        let defs = page_defs(self.current_page);

        #[cfg(feature = "krx_native")]
        if self.current_page == PageId::System {
            self.populate_system_page(defs, ctx);
            return;
        }

        populate_defs(&mut self.items, defs, ctx);
        self.items_len = defs.len().min(self.items.len());

        if self.current_page == PageId::Apps {
            let cat = crate::apps::launcher::catalog::get_catalog();
            #[cfg(feature = "filesystem")]
            cat.scan(ctx.storage);
            for e in &cat.entries {
                if self.items_len >= self.items.len() {
                    break;
                }
                if crate::apps::launcher::catalog::PackageCatalog::matches_kind(e, false)
                    && e.has_config
                {
                    self.items[self.items_len] = gfx::gui::modules::menu::MenuItem::new(
                        e.title.as_str(),
                        MenuItemType::Submenu,
                    );
                    self.items_len += 1;
                }
            }
        }
    }

    #[cfg(feature = "krx_native")]
    #[inline(never)]
    fn append_system_flag_items(
        &mut self,
        cat: &'static crate::apps::launcher::catalog::PackageCatalog,
        sys_caps: kernel::caps::CapabilitySet,
        flag: u8,
        item_type: MenuItemType,
    ) {
        let mut k = 0usize;
        while self.items_len < self.items.len() {
            let Some(entry) = cat.system_entry_at(sys_caps, flag, k) else {
                break;
            };
            self.items[self.items_len].title = entry.title.as_str();
            self.items[self.items_len].item_type = item_type;
            self.items_len += 1;
            k += 1;
        }
    }

    #[cfg(feature = "krx_native")]
    #[inline(never)]
    fn append_defs_slice(
        &mut self,
        defs: &[super::page::SettingDef],
        ctx: &mut SysContext,
    ) {
        let start = self.items_len;
        let n = defs.len().min(self.items.len() - start);
        populate_defs(&mut self.items[start..start + n], &defs[..n], ctx);
        self.items_len = start + n;
    }

    #[cfg(feature = "krx_native")]
    fn populate_system_page(
        &mut self,
        defs: &[super::page::SettingDef],
        ctx: &mut SysContext,
    ) {
        let cat = crate::apps::launcher::catalog::get_catalog();
        cat.scan(ctx.storage);
        let sys_caps = ctx.caps();
        let menu_ins = super::system::system_modules_insert_idx().min(defs.len());
        let act_ins = super::system::system_actions_insert_idx()
            .min(defs.len())
            .max(menu_ins);

        self.items_len = 0;
        self.append_defs_slice(&defs[..menu_ins], ctx);
        self.append_system_flag_items(
            cat,
            sys_caps,
            kernel::proc::krx::SectionType::FLAG_SYSTEM_MENU,
            MenuItemType::Submenu,
        );
        self.append_defs_slice(&defs[menu_ins..act_ins], ctx);
        self.append_system_flag_items(
            cat,
            sys_caps,
            kernel::proc::krx::SectionType::FLAG_SYSTEM_ACTION,
            MenuItemType::Action,
        );
        self.append_defs_slice(&defs[act_ins..], ctx);
    }

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
            apply_page_changes(self.current_page, ctx);
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
        #[cfg(feature = "passcode")]
        if self.current_page == PageId::Security {
            let idx = self.selected_idx;
            if idx < self.items_len {
                if let MenuItemType::Toggle(b) = self.items[idx].item_type {
                    super::security::update_security_toggle(ctx, idx, b);
                }
            }
            return;
        }

        let idx = self.selected_idx;
        if self.current_page == PageId::AppPrefs {
            if idx < self.items_len {
                let val = match self.items[idx].item_type {
                    MenuItemType::Range { val, .. } => Some(val as i32),
                    MenuItemType::Toggle(b) => Some(b as i32),
                    _ => None,
                };
                if let Some(v) = val {
                    super::app_prefs::commit_app_pref(idx, v, ctx);
                }
            }
            return;
        }
        let defs = page_defs(self.current_page);
        if idx < self.items_len && idx < defs.len() {
            let def = &defs[idx];
            let val = match self.items[idx].item_type {
                MenuItemType::Range { val, .. } => Some(val as i32),
                MenuItemType::Toggle(b) => Some(b as i32),
                _ => None,
            };
            if let Some(v) = val {
                self.commit_field(def.field, v, ctx);
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
            let defs = page_defs(self.current_page);
            if let Some(def) = defs.get(sel_idx) {
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
