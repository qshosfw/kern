//! Navigation Stack & Menu State Synchronization for OS Settings

use super::app::SettingsApp;
use super::options::{apply_page_changes, get_page, populate_defs};
use super::page::{MenuFrame, PageId, SettingField, SettingKind};
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
        if self.current_page == PageId::AppDetail || self.current_page == PageId::ServiceDetail {
            let is_srv = self.current_page == PageId::ServiceDetail;
            self.items_len = super::apps::populate_package_detail(
                &mut self.items,
                is_srv,
                self.selected_pkg_idx,
            );
            return;
        }

        let (_, defs) = get_page(self.current_page);
        populate_defs(&mut self.items, defs, ctx);
        self.items_len = defs.len().min(self.items.len());
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
        self.current_page = page;
        self.selected_idx = 0;
        self.title = get_page(page).0;
        self.populate_current_menu(ctx);
        self.view.reset_scroll();
        if ctx.config.display.animations_enabled() {
            self.slide_offset_x = 28;
        } else {
            self.slide_offset_x = 0;
        }
        self.reset_input_state();
    }

    #[inline(never)]
    pub fn pop_submenu(&mut self, ctx: &mut SysContext) -> bool {
        if self.stack_top > 0 {
            self.stack_top -= 1;
            let frame = self.stack[self.stack_top];
            self.current_page = frame.page;
            self.selected_idx = frame.selected_idx as usize;
            self.title = get_page(frame.page).0;
            self.populate_current_menu(ctx);
            if self.selected_idx >= self.items_len {
                self.selected_idx = self.items_len.saturating_sub(1);
            }
            self.view.adjust_scroll(self.selected_idx, self.items_len);
            self.view.scroller.jump((self.view.scroll_top as i16) * 13);
            if ctx.config.display.animations_enabled() {
                self.slide_offset_x = -28;
            } else {
                self.slide_offset_x = 0;
            }
            self.reset_input_state();

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

        let (_, defs) = get_page(self.current_page);
        let idx = self.selected_idx;
        if idx < self.items_len && idx < defs.len() {
            let def = &defs[idx];
            let val = match self.items[idx].item_type {
                MenuItemType::Range { val, .. } => Some(val as i32),
                MenuItemType::Toggle(b) => Some(b as i32),
                _ => None,
            };
            if let Some(v) = val {
                if def.field.set_val(v, &mut ctx.config) {
                    apply_page_changes(self.current_page, ctx);
                    self.dirty_timer_ms = 250;
                }
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
            #[cfg(feature = "passcode")]
            if self.current_page == PageId::Security {
                super::security::update_security_option(ctx, sel_idx, sel);
                return;
            }
            let (_, defs) = get_page(self.current_page);
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
                let val: i32 = if matches!(def.kind(), SettingKind::BatteryType) {
                    let (_, types) = super::battery::battery_options_and_types(nav_layout);
                    types.get(sel).copied().unwrap_or(types[0]) as i32
                } else {
                    sel as i32
                };
                if def.field.set_val(val, &mut ctx.config) {
                    apply_page_changes(self.current_page, ctx);
                    self.dirty_timer_ms = 250;
                }
            }
        }
    }
}
